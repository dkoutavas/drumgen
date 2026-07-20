//! Background generation worker.
//!
//! The worker thread owns the `GenerationManager` (and therefore the parsed cell
//! library) and is the ONLY place `assemble()` runs. The audio thread never
//! generates: it sends a `GenRequest` and later picks up an `Arc<Pattern>`.
//!
//! All handoffs use crossbeam channels (already a dependency). Latest-wins
//! coalescing on both ends bounds worker load under param automation.

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
    pub generation: u64,
}

enum Msg {
    Generate(GenRequest),
    Shutdown,
}

pub struct GenWorker {
    req_tx: Sender<Msg>,
    pat_rx: Receiver<Arc<Pattern>>,
    handle: Option<JoinHandle<()>>,
}

impl GenWorker {
    /// Spawn the worker, taking ownership of the (already-parsed) manager.
    pub fn spawn(gen: GenerationManager) -> Self {
        let (req_tx, req_rx) = bounded::<Msg>(4);
        let (pat_tx, pat_rx) = bounded::<Arc<Pattern>>(1);
        // The worker keeps a clone of the pattern receiver purely to evict a
        // stale unclaimed pattern before publishing a fresh one (latest-wins).
        let pat_rx_evict = pat_rx.clone();
        let handle = thread::Builder::new()
            .name("drumgen-gen".into())
            .spawn(move || worker_loop(gen, req_rx, pat_tx, pat_rx_evict))
            .expect("failed to spawn drumgen-gen worker");
        Self { req_tx, pat_rx, handle: Some(handle) }
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
) {
    while let Ok(msg) = req_rx.recv() {
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

        let res = gen.generate(
            req.style, req.humanize, req.bars, req.seed, req.swing, req.generative, req.tempo,
            req.meter, req.fill_every,
        );
        let style_name = gen.style_name(req.style as usize).unwrap_or("").to_string();
        let pattern = Arc::new(Pattern::from_assemble(
            &res, req.generation, req.seed, style_name, String::new(),
        ));

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
