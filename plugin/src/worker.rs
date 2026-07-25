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
    Shutdown,
}

pub struct GenWorker {
    req_tx: Sender<Msg>,
    pat_rx: Receiver<Arc<Pattern>>,
    gc_tx: Sender<Arc<Pattern>>,
    handle: Option<JoinHandle<()>>,
}

impl GenWorker {
    /// Spawn the worker, taking ownership of the (already-parsed) manager.
    pub fn spawn(gen: GenerationManager) -> Self {
        let (req_tx, req_rx) = bounded::<Msg>(4);
        let (pat_tx, pat_rx) = bounded::<Arc<Pattern>>(1);
        let (gc_tx, gc_rx) = bounded::<Arc<Pattern>>(8);
        // The worker keeps a clone of the pattern receiver purely to evict a
        // stale unclaimed pattern before publishing a fresh one (latest-wins).
        let pat_rx_evict = pat_rx.clone();
        let handle = thread::Builder::new()
            .name("drumgen-gen".into())
            .spawn(move || worker_loop(gen, req_rx, pat_tx, pat_rx_evict, gc_rx))
            .expect("failed to spawn drumgen-gen worker");
        Self { req_tx, pat_rx, gc_tx, handle: Some(handle) }
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

fn worker_loop(
    gen: GenerationManager,
    req_rx: Receiver<Msg>,
    pat_tx: Sender<Arc<Pattern>>,
    pat_rx_evict: Receiver<Arc<Pattern>>,
    gc_rx: Receiver<Arc<Pattern>>,
) {
    while let Ok(msg) = req_rx.recv() {
        // Free any patterns the audio thread retired. They may sit in the bin
        // until the next request — bounded at 8, so that is just memory held,
        // not leaked.
        while gc_rx.try_recv().is_ok() {}

        let mut req = match msg {
            Msg::Generate(r) => r,
            Msg::Shutdown => return,
        };
        // Drain to the newest pending request before doing expensive work.
        loop {
            match req_rx.try_recv() {
                Ok(Msg::Generate(r)) => req = r,
                Ok(Msg::Shutdown) => return,
                Err(_) => break,
            }
        }

        // A panic anywhere in generation must not take the worker thread with
        // it: a dead worker presents as "the plugin froze on one pattern", and
        // one unlucky cell should cost a dice roll, not the session.
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

        let pattern = match built {
            Ok(p) => Arc::new(p),
            Err(_) => {
                nih_plug::nih_log!(
                    "drumgen: generation panicked (style {}, song {}, seed {}) — keeping the previous pattern",
                    req.style, req.song, req.seed
                );
                continue;
            }
        };

        // Publish latest-wins: on a full slot, evict the stale pattern and retry.
        let mut to_send = pattern;
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
