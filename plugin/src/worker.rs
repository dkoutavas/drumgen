//! Background generation worker.
//!
//! The worker thread owns the `GenerationManager` (and therefore the parsed cell
//! library) and is the ONLY place `assemble()` runs. The audio thread never
//! generates: it sends a `GenRequest` and later picks up an `Arc<Pattern>`.
//!
//! All handoffs use crossbeam channels (already a dependency). Latest-wins
//! coalescing on both ends bounds worker load under param automation.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};

use crate::generation::GenerationManager;
use crate::params::BANK_SLOTS;
use crate::pattern::Pattern;

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
        // The worker keeps a clone of the pattern receiver purely to evict a
        // stale unclaimed pattern before publishing a fresh one (latest-wins).
        let pat_rx_evict = pat_rx.clone();
        let handle = thread::Builder::new()
            .name("drumgen-gen".into())
            .spawn(move || worker_loop(gen, req_rx, pat_tx, pat_rx_evict, slot_tx, gc_rx))
            .expect("failed to spawn drumgen-gen worker");
        Self { req_tx, pat_rx, slot_rx, gc_tx, handle: Some(handle) }
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
) {
    while let Ok(msg) = req_rx.recv() {
        // Free any patterns the audio thread retired. They may sit in the bin
        // until the next request — bounded at 8, so that is just memory held,
        // not leaked.
        while gc_rx.try_recv().is_ok() {}

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
                Some(Msg::Shutdown) => return,
                None => {}
            }
            match req_rx.try_recv() {
                Ok(m) => batch = Some(m),
                Err(_) => break,
            }
        }

        // Live first: it is what the user is hearing right now.
        if let Some(req) = live {
            if let Some(pattern) = build_pattern(&gen, &req) {
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
            let Some(pattern) = build_pattern(&gen, &req) else { continue };
            match slot_tx.try_send((i as u8, Arc::new(pattern))) {
                Ok(()) => {}
                // A full slot channel means the audio thread has not drained in
                // 16 deliveries; dropping is correct — the slot stays dirty and
                // is re-requested.
                Err(TrySendError::Full(_)) => {}
                Err(TrySendError::Disconnected(_)) => return,
            }
        }
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
