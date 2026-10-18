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
    /// 1=tempo move, 2=host meter flip (Auto pads only), 3=plugin init,
    /// 4=a slot request got no answer (build panicked or delivery dropped).
    Dirty { mask: u16, cause: u8 },
    /// A jam run was closed with an end marker. Without this an export that
    /// silently fell back to a full pattern pass could not be told apart from
    /// one that ran correctly. Not logged when no pad was playing (no run).
    JamEnd { marker: u8, tick: i64 },
}

/// Append-only session log at `~/drumgen_output/drumgen.log`. Worker-thread
/// only. Opens lazily; a failed open disables logging rather than the plugin.
struct LogSink {
    file: Option<std::io::BufWriter<std::fs::File>>,
    start: Instant,
}

impl LogSink {
    fn new() -> Self {
        // Tests spawn real workers; without this every `cargo test` run would
        // splatter session-start lines into the user's live jam log.
        if cfg!(test) {
            return Self { file: None, start: Instant::now() };
        }
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
            LogEvent::JamEnd { marker, tick } => format!(
                "JAM END ({}) at tick {tick} — recorded",
                if marker == crate::params::JAM_END_STOP { "stop" } else { "bank exit" },
            ),
            LogEvent::Dirty { mask, cause } => format!(
                "DIRTY {:04X} ({})",
                mask,
                match cause {
                    0 => "bank edited",
                    1 => "tempo moved",
                    2 => "host meter flip: Auto pads re-bake",
                    4 => "slot build never answered: re-requesting",
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
    /// FORM_SHAPES index; 0 = Off, >0 = a generated form (overrides `song`).
    pub form: i32,
    pub form_seed: u64,
    /// METER is on Auto: a generated form may turn meters (a forced meter
    /// holds the whole form). `meter` alone cannot say so: under Auto it
    /// carries the host's meter.
    pub meter_auto: bool,
    pub generation: u64,
}

/// What the worker publishes for a LIVE request: the pattern, or the news
/// that the build for `generation` failed. Without the second, a waiter
/// (`await_pending`, which Bitwig's Offline-labelled live engine hits) could
/// only learn of a failure by timing out.
pub enum Published {
    Pattern(Arc<Pattern>),
    Failed { generation: u64 },
}

enum Msg {
    Generate(GenRequest),
    /// Generate for bank slot N without making it the live pattern.
    GenerateSlot(u8, GenRequest),
    Shutdown,
}

pub struct GenWorker {
    req_tx: Sender<Msg>,
    pat_rx: Receiver<Published>,
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
        let (pat_tx, pat_rx) = bounded::<Published>(1);
        let (slot_tx, slot_rx) = bounded::<(u8, Arc<Pattern>)>(BANK_SLOTS);
        // A 16-slot refresh hands the audio thread 16 deliveries, and it
        // retires the 16 stale patterns they replace. A bin of 8 overflowed and
        // freed the rest ON the audio thread; this holds a full refresh plus
        // the live pattern and the GUI view with room to spare.
        let (gc_tx, gc_rx) = bounded::<Arc<Pattern>>(2 * BANK_SLOTS + 8);
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

    /// Audio-thread safe: drains the publish slot to the newest pattern. A
    /// failure notice is skipped: the previous pattern simply keeps playing.
    pub fn try_recv_latest(&self) -> Option<Arc<Pattern>> {
        let mut latest = None;
        while let Ok(msg) = self.pat_rx.try_recv() {
            if let Published::Pattern(p) = msg {
                latest = Some(p);
            }
        }
        latest
    }

    /// Wait for the next published pattern, at most `timeout`. `None` on a
    /// timeout, a dead worker, or a failed build (nothing is coming, so there
    /// is no point waiting longer). Used by `initialize()`.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<Arc<Pattern>> {
        match self.recv_published(timeout)? {
            Published::Pattern(p) => Some(p),
            Published::Failed { .. } => None,
        }
    }

    /// Wait for the next thing the worker publishes, pattern or failure, at
    /// most `timeout`. Never unbounded: an untimed `recv()` once hung the audio
    /// thread when a build panicked. `await_pending` uses this so a failure for
    /// the generation it waits on ends the wait at once instead of at the
    /// deadline.
    pub fn recv_published(&self, timeout: Duration) -> Option<Published> {
        self.pat_rx.recv_timeout(timeout).ok()
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
        // The song skeleton, if any: a generated FORM wins over SONG.
        let song: Option<(String, String)> = if req.form > 0 {
            gen.form_arrangement(
                req.style, crate::params::form_key(req.form), req.form_seed, req.meter,
                req.meter_auto,
            )
        } else if req.song > 0 {
            Some((crate::params::song_label(req.song), crate::params::song_str(req.song).to_string()))
        } else {
            None
        };
        let res = if let Some((_, arr)) = &song {
            gen.generate_arrangement(
                req.style, arr, req.humanize, req.seed, req.swing, req.generative, req.tempo,
                req.meter,
            )
        } else {
            gen.generate(
                req.style, req.humanize, req.bars, req.seed, req.swing, req.generative,
                req.tempo, req.meter, req.fill_every,
            )
        };
        let style_name = gen.style_name(req.style as usize).unwrap_or("").to_string();
        // The song's name and the section map that lets the GUI name the
        // viewed bar. Both stay empty in loop mode.
        let (song_name, arrangement, sections) = if let Some((name, arr)) = song {
            let home = if req.meter == (0, 0) { (4, 4) } else { req.meter };
            // Pair each section with the cell that actually won it, so the
            // GUI reports what is PLAYING rather than what the form asked
            // for (a style with no blast cell must not be labelled BLAST).
            let secs = crate::engine::assembler::parse_arrangement(&arr, home)
                .into_iter()
                .enumerate()
                .map(|(i, sec)| {
                    let cell = res.section_cells.get(i).cloned().unwrap_or_default();
                    (sec.section_type, sec.bars, cell)
                })
                .collect();
            (name, arr, secs)
        } else {
            (String::new(), String::new(), Vec::new())
        };
        let mut p =
            Pattern::from_assemble(&res, req.generation, req.seed, style_name, song_name, sections);
        p.arrangement = arrangement;
        p
    }));

    match built {
        Ok(p) => Some(p),
        Err(_) => {
            nih_plug::nih_log!(
                "drumgen: generation panicked (style {}, song {}, form {}:{}, seed {}) — keeping the previous pattern",
                req.style, req.song, req.form, req.form_seed, req.seed
            );
            None
        }
    }
}

fn worker_loop(
    gen: GenerationManager,
    req_rx: Receiver<Msg>,
    pat_tx: Sender<Published>,
    pat_rx_evict: Receiver<Published>,
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

        // Fold the queue into one batch: the LIVE request keeps latest-wins
        // (only the newest matters), while slot requests coalesce PER SLOT and
        // never across slots — a 16-slot refresh must produce 16 patterns.
        let mut live: Option<GenRequest> = None;
        let mut slot_reqs: [Option<GenRequest>; BANK_SLOTS] = [None; BANK_SLOTS];
        let mut shutdown = fold(msg, &mut live, &mut slot_reqs);

        loop {
            // Fold anything that arrived meanwhile, between EVERY generation:
            // a live request (a dice press, a knob) waits behind at most the
            // one slot build in flight, not the whole 16-slot refresh.
            while !shutdown {
                match req_rx.try_recv() {
                    Ok(m) => shutdown = fold(m, &mut live, &mut slot_reqs),
                    Err(_) => break,
                }
            }
            if shutdown {
                sink.line("=== session end (shutdown) ===");
                sink.flush();
                return;
            }

            // Live first: it is what the user is hearing right now.
            if let Some(req) = live.take() {
                if !serve_live(&mut sink, &gen, &req, &pat_tx, &pat_rx_evict) {
                    return;
                }
                continue;
            }

            // Free what the audio thread retired while we were building.
            while gc_rx.try_recv().is_ok() {}
            let Some(i) = slot_reqs.iter().position(Option::is_some) else { break };
            let Some(req) = slot_reqs[i].take() else { break };
            let label = format!("pad{}", i + 1);
            log_request(&mut sink, &gen, &label, &req);
            let Some(pattern) = build_pattern(&gen, &req) else {
                sink.line(&format!("BUILD FAILED {label} (generation panicked)"));
                continue;
            };
            log_built(&mut sink, &label, &pattern);
            match slot_tx.try_send((i as u8, Arc::new(pattern))) {
                Ok(()) => {}
                // A full slot channel means the audio thread has not drained in
                // 16 deliveries; the pattern is dropped here and the pump's
                // retry timer re-requests the pad.
                Err(TrySendError::Full(_)) => {}
                Err(TrySendError::Disconnected(_)) => return,
            }
        }
        sink.flush();
    }
    sink.line("=== session end (host dropped the queue) ===");
    sink.flush();
}

/// Fold one queued message into the pending batch. Returns true on Shutdown.
fn fold(
    msg: Msg,
    live: &mut Option<GenRequest>,
    slots: &mut [Option<GenRequest>; BANK_SLOTS],
) -> bool {
    match msg {
        Msg::Generate(r) => *live = Some(r),
        Msg::GenerateSlot(i, r) => {
            if let Some(entry) = slots.get_mut(i as usize) {
                *entry = Some(r);
            }
        }
        Msg::Shutdown => return true,
    }
    false
}

/// Build and publish the live pattern (latest-wins: a stale unclaimed pattern
/// is evicted first). Returns false when the audio side is gone.
fn serve_live(
    sink: &mut LogSink,
    gen: &GenerationManager,
    req: &GenRequest,
    pat_tx: &Sender<Published>,
    pat_rx_evict: &Receiver<Published>,
) -> bool {
    log_request(sink, gen, "live", req);
    let mut to_send = match build_pattern(gen, req) {
        Some(pattern) => {
            log_built(sink, "live", &pattern);
            Published::Pattern(Arc::new(pattern))
        }
        None => {
            sink.line("BUILD FAILED live (generation panicked — previous pattern kept)");
            // Say so, so a waiter stops now instead of at its deadline. Evicting
            // an older unclaimed pattern is fine: it was already superseded by
            // the request that just failed, and the current pattern keeps playing.
            Published::Failed { generation: req.generation }
        }
    };
    loop {
        match pat_tx.try_send(to_send) {
            Ok(()) => return true,
            Err(TrySendError::Full(p)) => {
                let _ = pat_rx_evict.try_recv();
                to_send = p;
            }
            Err(TrySendError::Disconnected(_)) => return false,
        }
    }
}

/// One log line per generation request, with the style resolved to its name.
fn log_request(sink: &mut LogSink, gen: &GenerationManager, label: &str, req: &GenRequest) {
    let style = gen.style_name(req.style as usize).unwrap_or("?");
    let meter = match req.meter {
        (0, 0) => "auto".to_string(),
        (n, d) => format!("{n}/{d}"),
    };
    sink.line(&format!(
        "GEN {label}: {style} seed={} meter={meter} bars={} fill={} song={} form={}:{} tempo={:.0}",
        req.seed, req.bars, req.fill_every, req.song, req.form, req.form_seed, req.tempo
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
    // The song or generated form, as a ready songs.txt line (decision 5 of
    // docs/generated-arrangements.md: the name is the form, the log keeps it).
    if !p.arrangement.is_empty() {
        sink.line(&format!("SONG {label}: {} | {}", p.song_name, p.arrangement));
    }
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
            form: 0,
            form_seed: 0,
            meter_auto: true,
            generation: seed,
        }
    }

    fn serve_once(worker: &GenWorker, seed: u64) -> Arc<Pattern> {
        assert!(worker.request(req(seed)), "request must be accepted on an empty queue");
        worker.recv_timeout(Duration::from_secs(5)).expect("worker delivers a pattern")
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
        let _ = worker.recv_timeout(Duration::from_secs(5));
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
        let live = worker.recv_timeout(Duration::from_secs(5)).expect("live pattern delivered");
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
        let live = worker.recv_timeout(Duration::from_secs(5)).expect("live pattern");
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

    /// A meter with a zero denominator is a reliable way to make generation
    /// panic (the host path filters it out; this simulates "one unlucky cell").
    fn poison(seed: u64) -> GenRequest {
        GenRequest { meter: (4, 0), ..req(seed) }
    }

    #[test]
    fn a_panicking_build_returns_none_not_a_dead_worker() {
        let gen = GenerationManager::new();
        assert!(build_pattern(&gen, &poison(1)).is_none());
        assert!(build_pattern(&gen, &req(1)).is_some());
    }

    /// The hang: a failed live build publishes nothing while the worker thread
    /// lives on, so an unbounded wait never returned. The bounded wait must
    /// give up, and the worker must still serve the next request.
    #[test]
    fn a_panicking_live_build_cannot_hang_the_waiter() {
        let worker = GenWorker::spawn(GenerationManager::new());
        assert!(worker.request(poison(1)));
        let t0 = Instant::now();
        assert!(worker.recv_timeout(Duration::from_millis(300)).is_none());
        assert!(t0.elapsed() < Duration::from_secs(3), "wait must be bounded");
        // The worker survived the panic and still builds.
        let p = serve_once(&worker, 2);
        assert!(!p.events.is_empty());
        worker.shutdown();
    }

    #[test]
    fn recv_timeout_is_bounded_when_nothing_was_requested() {
        let worker = GenWorker::spawn(GenerationManager::new());
        let t0 = Instant::now();
        assert!(worker.recv_timeout(Duration::from_millis(50)).is_none());
        assert!(t0.elapsed() < Duration::from_secs(3));
        worker.shutdown();
    }

    /// Slot requests coalesce per slot and every slot still arrives, even with
    /// a live request interleaved behind them.
    #[test]
    fn every_slot_and_the_live_pattern_are_delivered() {
        let worker = GenWorker::spawn(GenerationManager::new());
        for i in 0..BANK_SLOTS as u8 {
            assert!(worker.request_slot(i, req(100 + i as u64)));
        }
        assert!(worker.request(req(7)));
        let live = worker.recv_timeout(Duration::from_secs(10)).expect("live pattern");
        assert_eq!(live.generation, 7);
        let mut got = [false; BANK_SLOTS];
        let deadline = Instant::now() + Duration::from_secs(10);
        while got.iter().any(|g| !g) && Instant::now() < deadline {
            if let Some((i, p)) = worker.try_recv_slot() {
                assert_eq!(p.generation, 100 + i as u64);
                got[i as usize] = true;
            } else {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        assert!(got.iter().all(|g| *g), "a slot was lost: {got:?}");
        worker.shutdown();
    }

    /// A failed live build must END the wait, not let it run to the deadline:
    /// in Bitwig every live param change waits (lesson 12), so a 500 ms stall
    /// per failed build is an audible glitch. The failure is tagged with the
    /// request's generation so the waiter can tell it from an older one.
    #[test]
    fn a_failed_live_build_is_announced_at_once() {
        let worker = GenWorker::spawn(GenerationManager::new());
        let poisoned = GenRequest { generation: 41, ..poison(41) };
        assert!(worker.request(poisoned));
        let t0 = Instant::now();
        match worker.recv_published(Duration::from_secs(5)) {
            Some(Published::Failed { generation }) => assert_eq!(generation, 41),
            Some(Published::Pattern(_)) => panic!("a poisoned request produced a pattern"),
            None => panic!("no failure notice: the waiter would sit until its deadline"),
        }
        assert!(
            t0.elapsed() < Duration::from_millis(400),
            "the failure must arrive well inside OFFLINE_WAIT, took {:?}",
            t0.elapsed()
        );
        // The non-blocking receiver skips the notice: nothing to swap in.
        assert!(worker.try_recv_latest().is_none());
        // And the worker still builds afterwards.
        let p = serve_once(&worker, 42);
        assert_eq!(p.generation, 42);
        worker.shutdown();
    }
}
