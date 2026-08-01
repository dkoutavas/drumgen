//! Background generation worker.
//!
//! The worker thread owns the `GenerationManager` (and therefore the parsed cell
//! library) and is the ONLY place `assemble()` runs. The audio thread never
//! generates: it sends a `GenRequest` and later picks up an `Arc<Pattern>`.
//!
//! All handoffs use crossbeam channels (already a dependency). Latest-wins
//! coalescing on both ends bounds worker load under param automation.

use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, Sender, TrySendError};

use crate::generation::GenerationManager;
use crate::params::BANK_SLOTS;
use crate::pattern::Pattern;

/// A session event reported by the audio thread. Copy-only — no strings cross
/// the RT boundary; the worker formats them into `~/drumgen_output/drumgen.log`
/// so a jam session leaves a readable trace of what the plugin decided and why
/// (field debugging happened via screenshots of a hex readout once; never again).
#[derive(Clone, Copy, Debug)]
pub enum LogEvent {
    /// Transport started/stopped.
    Playing(bool),
    /// The host's reported time signature changed.
    HostMeter(i32, i32),
    /// A pad press arrived (MIDI or GUI) and was accepted into the queue or
    /// ignored (empty pad / already active).
    Trigger { slot: u8, midi: bool, accepted: bool },
    /// The queued pad took over at a bar boundary; `origin` is the absolute
    /// tick its bar 1 now sits on.
    Swap { slot: u8, origin: i64 },
    /// Bank mode ended: a knob (humanize/swing) or a discrete param tweak.
    BankExit { knob: bool },
    /// Slots marked for regeneration. cause: 0=store/clear/restore (epoch),
    /// 1=tempo move, 2=host meter flip (Auto pads only), 3=plugin init.
    Dirty { mask: u16, cause: u8 },
}

/// Append-only session log at `~/drumgen_output/drumgen.log`. Worker-thread
/// only. Opens lazily; a failed open disables logging rather than the plugin.
struct LogSink {
    file: Option<std::io::BufWriter<std::fs::File>>,
    start: Instant,
}

impl LogSink {
    fn new() -> Self {
        let path = crate::export::output_dir().join("drumgen.log");
        let file = (|| {
            std::fs::create_dir_all(crate::export::output_dir()).ok()?;
            // ponytail: crude rotation — start fresh past 1 MB, else append.
            let fresh = std::fs::metadata(&path).map(|m| m.len() > 1_000_000).unwrap_or(false);
            let f = std::fs::OpenOptions::new()
                .create(true)
                .append(!fresh)
                .truncate(fresh)
                .write(true)
                .open(&path)
                .ok()?;
            Some(std::io::BufWriter::new(f))
        })();
        if file.is_none() {
            nih_plug::nih_log!("drumgen: cannot open {} — session logging disabled", path.display());
        }
        let mut sink = Self { file, start: Instant::now() };
        sink.line(&format!("=== drumgen v{} session start ===", env!("CARGO_PKG_VERSION")));
        sink
    }

    fn line(&mut self, s: &str) {
        if let Some(f) = &mut self.file {
            let t = self.start.elapsed().as_secs_f64();
            let _ = writeln!(f, "[{t:8.2}s] {s}");
        }
    }

    fn event(&mut self, ev: LogEvent) {
        let s = match ev {
            LogEvent::Playing(on) => format!("TRANSPORT {}", if on { "play" } else { "stop" }),
            LogEvent::HostMeter(n, d) => format!("HOST METER {n}/{d}"),
            LogEvent::Trigger { slot, midi, accepted } => format!(
                "TRIGGER pad{} via {} — {}",
                slot + 1,
                if midi { "midi" } else { "click" },
                if accepted { "queued" } else { "ignored (empty or already playing)" }
            ),
            LogEvent::Swap { slot, origin } => {
                format!("SWAP -> pad{} (bar 1 anchored at tick {origin})", slot + 1)
            }
            LogEvent::BankExit { knob } => format!(
                "BANK EXIT ({} tweak — params drive playback again)",
                if knob { "knob" } else { "discrete" }
            ),
            LogEvent::Dirty { mask, cause } => format!(
                "DIRTY {:04X} ({})",
                mask,
                match cause {
                    0 => "bank edited",
                    1 => "tempo moved",
                    2 => "host meter flip: Auto pads re-bake",
                    _ => "plugin init",
                }
            ),
        };
        self.line(&s);
    }

    fn flush(&mut self) {
        if let Some(f) = &mut self.file {
            let _ = f.flush();
        }
    }
}

/// A request to generate a new pattern. `generation` is a monotonic id copied
/// onto the resulting `Pattern`.
#[derive(Clone, Copy)]
pub struct GenRequest {
    pub style: i32,
    pub humanize: f64,
    pub bars: i32,
    pub seed: u64,
    pub swing: f64,
    pub generative: bool,
    pub tempo: f64,
    pub meter: (i32, i32),
    pub fill_every: i32,
    /// SONGS index; 0 = Off (loop mode), >0 = arranged song skeleton.
    pub song: i32,
    pub generation: u64,
}

enum Msg {
    Generate(GenRequest),
    /// Generate for bank slot N without making it the live pattern.
    GenerateSlot(u8, GenRequest),
    Shutdown,
}

pub struct GenWorker {
    req_tx: Sender<Msg>,
    pat_rx: Receiver<Arc<Pattern>>,
    slot_rx: Receiver<(u8, Arc<Pattern>)>,
    gc_tx: Sender<Arc<Pattern>>,
    log_tx: Sender<LogEvent>,
    handle: Option<JoinHandle<()>>,
}

impl GenWorker {
    /// Spawn the worker, taking ownership of the (already-parsed) manager.
    pub fn spawn(gen: GenerationManager) -> Self {
        // Capacity 32: a bank refresh queues up to 16 slot requests in one
        // buffer and the live request must still fit behind them.
        let (req_tx, req_rx) = bounded::<Msg>(32);
        let (pat_tx, pat_rx) = bounded::<Arc<Pattern>>(1);
        let (slot_tx, slot_rx) = bounded::<(u8, Arc<Pattern>)>(BANK_SLOTS);
        let (gc_tx, gc_rx) = bounded::<Arc<Pattern>>(8);
        let (log_tx, log_rx) = bounded::<LogEvent>(128);
        // The worker keeps a clone of the pattern receiver purely to evict a
        // stale unclaimed pattern before publishing a fresh one (latest-wins).
        let pat_rx_evict = pat_rx.clone();
        let handle = thread::Builder::new()
            .name("drumgen-gen".into())
            .spawn(move || worker_loop(gen, req_rx, pat_tx, pat_rx_evict, slot_tx, gc_rx, log_rx))
            .expect("failed to spawn drumgen-gen worker");
        Self { req_tx, pat_rx, slot_rx, gc_tx, log_tx, handle: Some(handle) }
    }

    /// Audio-thread safe: report a session event for the log. Never blocks; a
    /// full queue just drops the line (the log is a trace, not a ledger).
    pub fn log(&self, ev: LogEvent) {
        let _ = self.log_tx.try_send(ev);
    }

    /// Hand a retired pattern to the worker so the `free()` happens off the
    /// audio thread. Audio-thread safe: never blocks. On a full bin (8 retired
    /// patterns not yet collected) the Arc simply drops here — bounded, and by
    /// then the worker is idle enough that it hardly happens.
    pub fn retire(&self, p: Arc<Pattern>) {
        let _ = self.gc_tx.try_send(p);
    }

    /// Audio-thread safe: never blocks. Returns whether the request was
    /// accepted — on a full queue (or dead worker) it is dropped and the caller
    /// must NOT record it as sent, so change detection re-sends next buffer.
    /// A Disconnected error means the worker died — log it, because a dead
    /// worker presents as "the pattern is frozen", easy to misdiagnose.
    pub fn request(&self, req: GenRequest) -> bool {
        match self.req_tx.try_send(Msg::Generate(req)) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => false,
            Err(TrySendError::Disconnected(_)) => {
                nih_plug::nih_log!("drumgen: generation worker is gone — pattern updates are frozen");
                false
            }
        }
    }

    /// Audio-thread safe: never blocks. Same contract as `request()` — a
    /// refused slot request must stay marked dirty and be re-sent next buffer.
    pub fn request_slot(&self, slot: u8, req: GenRequest) -> bool {
        self.req_tx.try_send(Msg::GenerateSlot(slot, req)).is_ok()
    }

    /// Audio-thread safe: take one finished slot pattern, if any. Slot
    /// deliveries are NOT coalesced (unlike the live pattern) — every slot
    /// must arrive, so the caller drains this in a loop.
    pub fn try_recv_slot(&self) -> Option<(u8, Arc<Pattern>)> {
        self.slot_rx.try_recv().ok()
    }

    /// Audio-thread safe: drains the publish slot to the newest pattern.
    pub fn try_recv_latest(&self) -> Option<Arc<Pattern>> {
        let mut latest = None;
        while let Ok(p) = self.pat_rx.try_recv() {
            latest = Some(p);
        }
        latest
    }

    /// Blocking — for the synchronous first generation in `initialize()` only.
    pub fn recv_blocking(&self) -> Option<Arc<Pattern>> {
        self.pat_rx.recv().ok()
    }

    /// Shut the worker down and join it.
    pub fn shutdown(mut self) {
        let _ = self.req_tx.send(Msg::Shutdown);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Build one pattern. Shared by the live path and every bank slot — a slot's
/// pattern must be indistinguishable from the live one it was stored from.
///
/// A panic anywhere in generation must not take the worker thread with it: a
/// dead worker presents as "the plugin froze on one pattern", and one unlucky
/// cell should cost a dice roll, not the session.
fn build_pattern(gen: &GenerationManager, req: &GenRequest) -> Option<Pattern> {
    let built = catch_unwind(AssertUnwindSafe(|| {
        let res = if req.song > 0 {
            gen.generate_arrangement(
                req.style, crate::params::song_str(req.song), req.humanize, req.seed,
                req.swing, req.generative, req.tempo, req.meter,
            )
        } else {
            gen.generate(
                req.style, req.humanize, req.bars, req.seed, req.swing, req.generative,
                req.tempo, req.meter, req.fill_every,
            )
        };
        let style_name = gen.style_name(req.style as usize).unwrap_or("").to_string();
        // Song label rides in cell_name; the section map lets the GUI name
        // the viewed bar. Both stay empty in loop mode.
        let (song_name, sections) = if req.song > 0 {
            let name = crate::params::song_label(req.song);
            let home = if req.meter == (0, 0) { (4, 4) } else { req.meter };
            // Pair each section with the cell that actually won it, so the
            // GUI reports what is PLAYING rather than what the form asked
            // for (a style with no blast cell must not be labelled BLAST).
            let secs = crate::engine::assembler::parse_arrangement(
                crate::params::song_str(req.song), home,
            )
            .into_iter()
            .enumerate()
            .map(|(i, sec)| {
                let cell = res.section_cells.get(i).cloned().unwrap_or_default();
                (sec.section_type, sec.bars, cell)
            })
            .collect();
            (name, secs)
        } else {
            (String::new(), Vec::new())
        };
        Pattern::from_assemble(&res, req.generation, req.seed, style_name, song_name, sections)
    }));

    match built {
        Ok(p) => Some(p),
        Err(_) => {
            nih_plug::nih_log!(
                "drumgen: generation panicked (style {}, song {}, seed {}) — keeping the previous pattern",
                req.style, req.song, req.seed
            );
            None
        }
    }
}

fn worker_loop(
    gen: GenerationManager,
    req_rx: Receiver<Msg>,
    pat_tx: Sender<Arc<Pattern>>,
    pat_rx_evict: Receiver<Arc<Pattern>>,
    slot_tx: Sender<(u8, Arc<Pattern>)>,
    gc_rx: Receiver<Arc<Pattern>>,
    log_rx: Receiver<LogEvent>,
) {
    let mut sink = LogSink::new();

    loop {
        // Timeout wake so the session log flushes while the plugin idles —
        // audio-thread events must reach the file within ~250ms, not at the
        // next generation request.
        let msg = match req_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(m) => Some(m),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };

        // Free any patterns the audio thread retired. They may sit in the bin
        // until the next request — bounded at 8, so that is just memory held,
        // not leaked.
        while gc_rx.try_recv().is_ok() {}
        while let Ok(ev) = log_rx.try_recv() {
            sink.event(ev);
        }
        let Some(msg) = msg else {
            sink.flush();
            continue;
        };

        // Drain the whole queue into one batch: the LIVE request keeps
        // latest-wins (only the newest matters, the rest are already stale),
        // while slot requests coalesce PER SLOT and never across slots — a
        // 16-slot refresh must produce 16 patterns, not one.
        let mut live: Option<GenRequest> = None;
        let mut slot_reqs: [Option<GenRequest>; BANK_SLOTS] = [None; BANK_SLOTS];
        let mut batch = Some(msg);
        loop {
            match batch.take() {
                Some(Msg::Generate(r)) => live = Some(r),
                Some(Msg::GenerateSlot(i, r)) => {
                    if let Some(entry) = slot_reqs.get_mut(i as usize) {
                        *entry = Some(r);
                    }
                }
                Some(Msg::Shutdown) => {
                    sink.line("=== session end (shutdown) ===");
                    sink.flush();
                    return;
                }
                None => {}
            }
            match req_rx.try_recv() {
                Ok(m) => batch = Some(m),
                Err(_) => break,
            }
        }

        // Live first: it is what the user is hearing right now.
        if let Some(req) = live {
            log_request(&mut sink, &gen, "live", &req);
            if let Some(pattern) = build_pattern(&gen, &req) {
                log_built(&mut sink, "live", &pattern);
                // Publish latest-wins: on a full slot, evict the stale pattern
                // and retry.
                let mut to_send = Arc::new(pattern);
                loop {
                    match pat_tx.try_send(to_send) {
                        Ok(()) => break,
                        Err(TrySendError::Full(p)) => {
                            let _ = pat_rx_evict.try_recv();
                            to_send = p;
                        }
                        Err(TrySendError::Disconnected(_)) => return,
                    }
                }
            }
        }

        // ponytail: a live request arriving mid-batch waits for the whole slot
        // batch (~16 generations, tens of ms) before it is seen. Upgrade path:
        // poll req_rx between slots and restart the batch on a live request.
        for i in 0..BANK_SLOTS {
            let Some(req) = slot_reqs[i] else { continue };
            let label = format!("pad{}", i + 1);
            log_request(&mut sink, &gen, &label, &req);
            let Some(pattern) = build_pattern(&gen, &req) else {
                sink.line(&format!("BUILD FAILED {label} (generation panicked — pad stays dirty)"));
                continue;
            };
            log_built(&mut sink, &label, &pattern);
            match slot_tx.try_send((i as u8, Arc::new(pattern))) {
                Ok(()) => {}
                // A full slot channel means the audio thread has not drained in
                // 16 deliveries; dropping is correct — the slot stays dirty and
                // is re-requested.
                Err(TrySendError::Full(_)) => {}
                Err(TrySendError::Disconnected(_)) => return,
            }
        }
        sink.flush();
    }
    sink.line("=== session end (host dropped the queue) ===");
    sink.flush();
}

/// One log line per generation request, with the style resolved to its name.
fn log_request(sink: &mut LogSink, gen: &GenerationManager, label: &str, req: &GenRequest) {
    let style = gen.style_name(req.style as usize).unwrap_or("?");
    let meter = match req.meter {
        (0, 0) => "auto".to_string(),
        (n, d) => format!("{n}/{d}"),
    };
    sink.line(&format!(
        "GEN {label}: {style} seed={} meter={meter} bars={} fill={} song={} tempo={:.0}",
        req.seed, req.bars, req.fill_every, req.song, req.tempo
    ));
}

/// One log line per finished pattern: what actually got built.
fn log_built(sink: &mut LogSink, label: &str, p: &Pattern) {
    let bars = p.bar_starts.len().saturating_sub(1);
    let meters: Vec<String> = p
        .time_signatures
        .iter()
        .map(|t| format!("{}/{}", t.numerator, t.denominator))
        .collect();
    sink.line(&format!(
        "BUILT {label}: {bars} bars [{}] {} events",
        meters.join(" "),
        p.events.len()
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(seed: u64) -> GenRequest {
        GenRequest {
            style: 0,
            humanize: 0.4,
            bars: 4,
            seed,
            swing: 0.0,
            generative: true,
            tempo: 120.0,
            meter: (0, 0),
            fill_every: 0,
            song: 0,
            generation: seed,
        }
    }

    fn serve_once(worker: &GenWorker, seed: u64) -> Arc<Pattern> {
        assert!(worker.request(req(seed)), "request must be accepted on an empty queue");
        worker.recv_blocking().expect("worker delivers a pattern")
    }

    #[test]
    fn worker_survives_a_shutdown_and_respawn() {
        // A host may deactivate and re-initialize the plugin (project close and
        // reopen). The worker owns the cell library, so a respawn that cannot
        // find a manager leaves the plugin frozen on its last pattern forever —
        // this pins that a second worker generation works exactly like the first.
        let first = GenWorker::spawn(GenerationManager::new());
        let p1 = serve_once(&first, 1);
        assert!(!p1.events.is_empty());
        first.shutdown();

        let second = GenWorker::spawn(GenerationManager::new());
        let p2 = serve_once(&second, 1);
        assert_eq!(
            p1.events.len(),
            p2.events.len(),
            "a respawned worker must generate the same pattern for the same request"
        );
        second.shutdown();
    }

    /// Collect slot deliveries until `n` have arrived (or the worker stalls).
    fn drain_slots(worker: &GenWorker, n: usize) -> Vec<(u8, Arc<Pattern>)> {
        let mut got = Vec::new();
        for _ in 0..2000 {
            while let Some(p) = worker.try_recv_slot() {
                got.push(p);
            }
            if got.len() >= n {
                break;
            }
            thread::sleep(std::time::Duration::from_millis(2));
        }
        got
    }

    #[test]
    fn slot_requests_never_coalesce_across_slots() {
        // THE reason the worker protocol changed: the old loop drained to the
        // newest request, so a 16-slot bank refresh produced ONE pattern and
        // fifteen slots stayed empty forever.
        let worker = GenWorker::spawn(GenerationManager::new());
        for i in 0..8u8 {
            assert!(worker.request_slot(i, req(i as u64 + 1)), "queue accepts the burst");
        }
        let got = drain_slots(&worker, 8);
        let mut slots: Vec<u8> = got.iter().map(|(i, _)| *i).collect();
        slots.sort_unstable();
        assert_eq!(slots, (0..8).collect::<Vec<u8>>(), "every slot delivered exactly once");
        assert!(got.iter().all(|(_, p)| !p.events.is_empty()));
        worker.shutdown();
    }

    #[test]
    fn repeated_requests_for_one_slot_keep_only_the_newest() {
        // Dragging a param while a slot is dirty must not queue a pile of
        // stale generations — per-slot latest-wins, same as the live pattern.
        let worker = GenWorker::spawn(GenerationManager::new());
        // Occupy the worker so the burst below lands in one batch.
        assert!(worker.request(req(99)));
        for seed in 1..=6u64 {
            assert!(worker.request_slot(3, req(seed)));
        }
        let _ = worker.recv_blocking();
        let got = drain_slots(&worker, 1);
        assert_eq!(got.len(), 1, "six requests for one slot collapse to one pattern");
        assert_eq!(got[0].0, 3);
        assert_eq!(got[0].1.seed, 6, "the newest request wins");
        worker.shutdown();
    }

    #[test]
    fn live_pattern_still_arrives_alongside_slot_traffic() {
        // The bank must not starve or reorder the pattern the user is hearing.
        let worker = GenWorker::spawn(GenerationManager::new());
        for i in 0..4u8 {
            assert!(worker.request_slot(i, req(i as u64 + 10)));
        }
        assert!(worker.request(req(7)));
        let live = worker.recv_blocking().expect("live pattern delivered");
        assert_eq!(live.seed, 7);
        assert_eq!(drain_slots(&worker, 4).len(), 4, "slots still complete");
        worker.shutdown();
    }

    #[test]
    fn a_stored_slot_sounds_exactly_like_what_was_stored() {
        // STORE's whole promise: the pad plays back the sound that was captured,
        // note for note. Both paths run build_pattern, so this pins that the
        // slot route never diverges (a different seed salt, a dropped param).
        let worker = GenWorker::spawn(GenerationManager::new());
        let r = req(4242);
        assert!(worker.request(r));
        let live = worker.recv_blocking().expect("live pattern");
        assert!(worker.request_slot(5, r));
        let slot = drain_slots(&worker, 1);
        assert_eq!(slot.len(), 1);
        let (idx, stored) = &slot[0];
        assert_eq!(*idx, 5);
        assert_eq!(stored.total_ticks, live.total_ticks);
        let sig = |p: &Pattern| -> Vec<(i64, u8, u8, bool)> {
            p.events.iter().map(|e| (e.tick, e.note, e.velocity, e.is_note_on)).collect()
        };
        assert_eq!(sig(stored), sig(&live), "a slot must be note-identical to its source");
        worker.shutdown();
    }

    #[test]
    fn retired_patterns_do_not_block_the_caller() {
        // The bin is bounded: past its capacity `retire` must drop the Arc
        // inline rather than block, because it is called from the audio thread.
        let worker = GenWorker::spawn(GenerationManager::new());
        let p = serve_once(&worker, 2);
        for _ in 0..64 {
            worker.retire(p.clone());
        }
        // Still alive and serving after the bin overflowed.
        assert!(!serve_once(&worker, 3).events.is_empty());
        worker.shutdown();
    }
}
