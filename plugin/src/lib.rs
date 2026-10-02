use nih_plug::prelude::*;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

pub mod engine;
mod editor;
mod export;
mod generation;
mod params;
mod pattern;
mod playback;
mod worker;

use generation::GenerationManager;
use params::DrumgenParams;
use pattern::Pattern;
use worker::{GenRequest, GenWorker};

use engine::midi_math::PPQ;

pub(crate) const MIDI_CHANNEL: u8 = 9; // channel 10 (1-indexed) — GM drums
const SETTLE_SECS: f32 = 0.150; // param-change debounce before regenerating
// Generative is always on: the engine re-realizes a probability grid per seed
// (so the dice re-rolls the groove) and falls back to fixed cells otherwise.
// No user-facing toggle — see design §3.
const GENERATIVE: bool = true;

/// Snapshot of everything that determines the pattern, for change detection.
/// `meter` is the EFFECTIVE meter: a forced param value, or — when the param
/// is Auto — the host's time signature ((0,0) when the host reports none,
/// which the engine treats as "style's native meter").
#[derive(Clone, Copy)]
struct ParamSnapshot {
    style: i32,
    humanize: f32,
    bars: i32,
    seed: i32,
    swing: f32,
    meter: (i32, i32),
    /// The METER param INDEX the effective meter was resolved from. Kept so a
    /// host time-signature flip (index unchanged, effective changed) can be
    /// told apart from the user reaching for the METER stepper.
    meter_index: i32,
    fill: i32,
    song: i32,
    tempo: f32,
}

/// What kind of change separates `desired` from `requested`, in priority
/// order. Pure — the field bug it guards ("changing Bitwig's meter kicked the
/// playing pad out of the bank") is pinned by tests.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ChangeKind {
    None,
    /// Humanize/swing/tempo — debounced by the settle timer.
    Continuous,
    /// The user touched a discrete param (style/bars/seed/fill/song/METER).
    /// Regenerates immediately and exits bank mode.
    UserDiscrete,
    /// The HOST's time signature moved under an Auto meter. Regenerates
    /// immediately but must NOT exit bank mode — the host is not the user,
    /// and the playing pad survives (Auto pads re-bake to the new meter).
    HostMeter,
}

fn classify_change(desired: &ParamSnapshot, requested: &ParamSnapshot) -> ChangeKind {
    let user_discrete = desired.style != requested.style
        || desired.bars != requested.bars
        || desired.seed != requested.seed
        || desired.meter_index != requested.meter_index
        || desired.fill != requested.fill
        || desired.song != requested.song;
    if user_discrete {
        return ChangeKind::UserDiscrete;
    }
    if desired.meter != requested.meter {
        return ChangeKind::HostMeter;
    }
    let continuous = (desired.humanize - requested.humanize).abs() > 1e-4
        || (desired.swing - requested.swing).abs() > 1e-4
        // Tempo affects ms-based humanization; regenerate past a 1 BPM step.
        || (desired.tempo - requested.tempo).abs() > 1.0;
    if continuous {
        ChangeKind::Continuous
    } else {
        ChangeKind::None
    }
}

impl ParamSnapshot {
    fn changed(&self, o: &ParamSnapshot) -> bool {
        classify_change(self, o) != ChangeKind::None
    }

    fn to_request(self, generation: u64) -> GenRequest {
        GenRequest {
            style: self.style,
            humanize: self.humanize as f64,
            bars: self.bars,
            seed: self.seed as u64,
            swing: self.swing as f64,
            generative: GENERATIVE,
            tempo: self.tempo as f64,
            meter: self.meter,
            fill_every: params::fill_of(self.fill),
            song: self.song,
            generation,
        }
    }
}

/// drumgen — algorithmic drum-pattern MIDI generator (VST3 + CLAP).
///
/// No audio processing: it reads the DAW transport and emits MIDI notes that
/// drive a drum sampler on another track. Generation runs on a background
/// worker thread; the audio thread only reads the current immutable pattern.
struct Drumgen {
    params: Arc<DrumgenParams>,
    /// The pattern currently playing (owned by the audio thread).
    current: Arc<Pattern>,
    /// A newer pattern awaiting a bar-boundary swap.
    pending: Option<Arc<Pattern>>,
    /// GUI-readable snapshot of the newest pattern. The audio thread publishes
    /// with try_lock (never blocks); the editor locks briefly once per frame.
    pattern_view: Arc<Mutex<Arc<Pattern>>>,
    /// Playhead position in pattern-relative TICKS for the GUI telegraph and
    /// horizon cursor; -1 = not playing. One relaxed store per buffer — the
    /// audio thread's only extra work. The GUI derives the bar itself (and
    /// re-reduces mod its own snapshot's length, guarding the swap window).
    playhead_tick: Arc<AtomicI64>,
    /// Manager parked here between `default()` and `initialize()` (then moved
    /// into the worker so the cell library is parsed exactly once).
    gen_manager: Option<GenerationManager>,
    worker: Option<GenWorker>,

    /// Reused scan scratch — allocation-free after warmup.
    scratch: Vec<playback::Emit>,
    /// Currently-sounding notes, one bit per MIDI note (0..127).
    active: u128,

    // ── change detection / debounce ──
    gen_counter: u64,
    last_desired: ParamSnapshot,
    requested: ParamSnapshot,
    settle_remaining: i64,
    settle_samples: i64,

    // ── transport bookkeeping ──
    was_playing: bool,
    last_end_samples: Option<i64>,
    sample_rate: f32,
    /// Host is rendering faster than real time (bounce/freeze). Generation then
    /// waits inline — nothing is audible, and a stale pattern would be baked in.
    offline: bool,

    // ── pattern bank ──
    /// Pre-generated pattern per slot, owned by the audio thread. Fixed array:
    /// no allocation, and an `Arc` clone on a trigger is just a refcount bump.
    slot_patterns: [Option<Arc<Pattern>>; params::BANK_SLOTS],
    /// Bit i = slot i needs (re)generating. Set on an epoch change or a tempo
    /// move; cleared as requests are accepted by the worker.
    slot_dirty: u16,
    /// The current dirty pass regenerates only Auto-meter pads (a host meter
    /// flip): forced pads' patterns are still valid and swapping in identical
    /// notes would cost a needless flush. Cleared by any full-dirty cause.
    dirty_auto_only: bool,
    /// Last bank epoch this thread acted on.
    bank_epoch_seen: u32,
    /// Slot currently playing, -1 = bank inactive (params drive playback).
    active_slot: i32,
    /// Slot waiting for the next bar boundary, -1 = none.
    queued_slot: i32,
    /// Absolute transport tick that the current pattern's bar 1 sits on.
    /// Normally 0 (patterns are anchored to the timeline); a slot switch moves
    /// it to the boundary so the new slot starts from ITS bar 1.
    origin: f64,

    /// Number of styles, for the editor's Style picker wrap-around.
    n_styles: usize,
}

impl Default for Drumgen {
    fn default() -> Self {
        // Parse the cell library first so params can bind to the real style names.
        let gen = GenerationManager::new();
        let style_names = gen.style_names();
        let n_styles = style_names.len().max(1);
        let params = Arc::new(DrumgenParams::new(style_names));
        let snap = ParamSnapshot {
            style: params.style.value(),
            humanize: params.humanize.value(),
            bars: params.bars.value(),
            seed: params.seed.value(),
            swing: params.swing.value(),
            // Host meter unknown before process(); Auto resolves to (0,0).
            meter: params::effective_meter(params.meter.value(), (0, 0)),
            meter_index: params.meter.value(),
            fill: params.fill.value(),
            song: params.song.value(),
            tempo: 120.0,
        };

        // Generate an initial pattern so the plugin is valid before `initialize()`;
        // the manager is then handed to the worker.
        let res = gen.generate(
            snap.style, snap.humanize as f64, snap.bars, snap.seed as u64, snap.swing as f64,
            GENERATIVE, snap.tempo as f64, snap.meter,
            params::fill_of(snap.fill),
        );
        let style_name = gen.style_name(snap.style as usize).unwrap_or("").to_string();
        let current = Arc::new(Pattern::from_assemble(
            &res, 0, snap.seed as u64, style_name, String::new(), Vec::new(),
        ));

        let pattern_view = Arc::new(Mutex::new(current.clone()));

        Self {
            params,
            current,
            pending: None,
            pattern_view,
            playhead_tick: Arc::new(AtomicI64::new(-1)),
            gen_manager: Some(gen),
            worker: None,
            scratch: Vec::with_capacity(4096),
            active: 0,
            gen_counter: 0,
            last_desired: snap,
            requested: snap,
            settle_remaining: 0,
            settle_samples: 0,
            was_playing: false,
            last_end_samples: None,
            sample_rate: 44100.0,
            offline: false,
            slot_patterns: Default::default(),
            slot_dirty: 0,
            dirty_auto_only: false,
            bank_epoch_seen: 0,
            active_slot: -1,
            queued_slot: -1,
            origin: 0.0,
            n_styles,
        }
    }
}

/// Plan one pump pass over the dirty mask: what to generate, what to drop,
/// which of the dirty slots are Auto-meter. Split out of `pump_slots` so the
/// decision is testable without a DAW — returns the per-slot requests, the
/// mask of slots that are now empty (their cached patterns must be released),
/// and the mask of dirty slots whose METER is Auto (a host meter flip
/// regenerates only those). Pure.
#[allow(clippy::type_complexity)]
fn plan_pump(
    dirty: u16,
    bank: &params::BankState,
) -> ([Option<params::SlotGen>; params::BANK_SLOTS], u16, u16) {
    let mut gens: [Option<params::SlotGen>; params::BANK_SLOTS] = [None; params::BANK_SLOTS];
    let mut empty = 0u16;
    let mut auto = 0u16;
    for i in 0..params::BANK_SLOTS {
        if dirty & (1 << i) == 0 {
            continue;
        }
        match bank.slot(i) {
            Some(s) => {
                gens[i] = Some(s.gen_part());
                if s.meter == 0 {
                    auto |= 1 << i;
                }
            }
            None => empty |= 1 << i,
        }
    }
    (gens, empty, auto)
}

/// Absolute transport tick of the first bar boundary strictly after `abs`.
///
/// `bar_starts` is pattern-relative (`[0] == 0`, last entry == total_ticks), so
/// this maps the absolute position into the pattern via `origin`, finds the
/// next bar start, and maps back. `rem_euclid` keeps it correct when the host
/// plays before the origin (pre-roll reports negative positions, and a slot
/// triggered mid-song sets an origin later than bar 1 of the timeline).
fn next_bar_boundary(bar_starts: &[i64], origin: f64, abs: f64) -> f64 {
    let total = bar_starts.last().copied().unwrap_or(1).max(1) as f64;
    let rel = (abs - origin).rem_euclid(total);
    // The first bar start strictly greater than `rel`; the terminal entry
    // (== total_ticks) is the wrap back to bar 1, which is a real boundary.
    let idx = bar_starts.partition_point(|&b| (b as f64) <= rel);
    let next = bar_starts.get(idx).copied().unwrap_or(total as i64) as f64;
    abs - rel + next
}

impl Drumgen {
    fn inputs(&self, tempo: f32, host_meter: (i32, i32)) -> ParamSnapshot {
        let meter_index = self.params.meter.value();
        ParamSnapshot {
            style: self.params.style.value(),
            humanize: self.params.humanize.value(),
            bars: self.params.bars.value(),
            seed: self.params.seed.value(),
            swing: self.params.swing.value(),
            meter: params::effective_meter(meter_index, host_meter),
            meter_index,
            fill: self.params.fill.value(),
            song: self.params.song.value(),
            tempo,
        }
    }

    fn next_gen(&mut self) -> u64 {
        self.gen_counter += 1;
        self.gen_counter
    }

    /// Block until the worker delivers the pattern we just requested. ONLY for
    /// offline rendering — never call this from a real-time buffer.
    fn await_pending(&mut self) {
        let Some(p) = self.worker.as_ref().and_then(|w| w.recv_blocking()) else { return };
        let stale = self.pending.replace(p);
        if let (Some(w), Some(stale)) = (&self.worker, stale) {
            w.retire(stale);
        }
    }

    /// Leave bank mode: the params are the editing surface, so any tweak hands
    /// playback back to them. Called from the change-detection paths.
    fn exit_bank(&mut self) {
        self.active_slot = -1;
        self.queued_slot = -1;
    }

    /// Report a session event. Audio-thread safe (non-blocking try_send).
    fn log(&self, ev: worker::LogEvent) {
        if let Some(w) = &self.worker {
            w.log(ev);
        }
    }

    /// Ask the worker to (re)generate every dirty slot.
    ///
    /// Audio-thread safe: `try_lock` only (a contended buffer just retries),
    /// and each snapshot is copied out as a `SlotGen` — a plain memcpy that
    /// never touches the `String` inside. Requests the worker refuses leave
    /// their dirty bit set for the next buffer.
    // NOTE: no offline gate here, on purpose. Bitwig reports the live engine
    // as ProcessMode::Offline (observed in the field: the bank starved with
    // DFFFF and zero sends while the transport was audibly running), and the
    // pump is fire-and-forget try_sends anyway — nothing to protect a bounce
    // from. `offline` gates only await_pending, which really blocks.
    fn pump_slots(&mut self, tempo: f32, host_meter: (i32, i32)) {
        if self.slot_dirty == 0 {
            return;
        }
        let Ok(bank) = self.params.bank.state.try_lock() else { return };
        let (gens, empty, auto) = plan_pump(self.slot_dirty, &bank);
        drop(bank);

        // Host-meter pass: only Auto pads regenerate; forced pads' dirty bits
        // clear without a request (their patterns are still right).
        if self.dirty_auto_only {
            self.slot_dirty &= auto | empty;
        }

        // A slot that was cleared drops its cached pattern (via the worker's
        // bin — this thread must not run a free()).
        for i in 0..params::BANK_SLOTS {
            if empty & (1 << i) != 0 {
                if let Some(old) = self.slot_patterns[i].take() {
                    if let Some(w) = &self.worker {
                        w.retire(old);
                    }
                }
                self.slot_dirty &= !(1 << i);
            }
        }

        for i in 0..params::BANK_SLOTS {
            // A bit may have been cleared above (auto-only pass, empties).
            if self.slot_dirty & (1 << i) == 0 {
                continue;
            }
            let Some(g) = gens[i] else { continue };
            let gen_id = self.next_gen();
            let req = g.to_request(tempo, host_meter, gen_id);
            let sent = self
                .worker
                .as_ref()
                .is_some_and(|w| w.request_slot(i as u8, req));
            if sent {
                self.slot_dirty &= !(1 << i);
            } else {
                // Queue full — stop here and retry from this slot next buffer.
                break;
            }
        }
        if self.slot_dirty == 0 {
            self.dirty_auto_only = false;
        }
    }

    /// A slot's cached pattern, bounds-checked. Every index here comes off the
    /// audio thread, where an out-of-range panic would take the DAW with it.
    fn slot_at(&self, slot: i32) -> Option<Arc<Pattern>> {
        usize::try_from(slot)
            .ok()
            .and_then(|i| self.slot_patterns.get(i))
            .and_then(|p| p.clone())
    }

    /// Publish bank state for the GUI: three relaxed stores per buffer.
    fn publish_bank(&self) {
        let mut ready = 0u32;
        for (i, p) in self.slot_patterns.iter().enumerate() {
            if p.is_some() {
                ready |= 1 << i;
            }
        }
        self.params.bank.ready.store(ready, Ordering::Relaxed);
        self.params.bank.active.store(self.active_slot, Ordering::Relaxed);
        self.params.bank.queued.store(self.queued_slot, Ordering::Relaxed);
    }

    /// Emit note-offs for every sounding note and clear the active set.
    fn flush(active: &mut u128, context: &mut impl ProcessContext<Self>, timing: u32) {
        let mut a = *active;
        while a != 0 {
            let note = a.trailing_zeros() as u8;
            context.send_event(NoteEvent::NoteOff {
                timing,
                voice_id: None,
                channel: MIDI_CHANNEL,
                note,
                velocity: 0.0,
            });
            a &= a - 1;
        }
        *active = 0;
    }
}

impl Plugin for Drumgen {
    const NAME: &'static str = "drumgen";
    const VENDOR: &'static str = "drumgen";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    // Input exists purely to trigger bank slots (notes 36..51). Nothing is
    // forwarded — the plugin's output is its own generated pattern.
    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::MidiCCs;

    // Some DAWs require audio I/O for a plugin to load even if it is purely a
    // MIDI effect. Dummy stereo output passes silence.
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        aux_input_ports: &[],
        aux_output_ports: &[],
        names: PortNames::const_default(),
    }];

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(
            self.params.clone(),
            self.n_styles,
            self.pattern_view.clone(),
            self.playhead_tick.clone(),
        )
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        self.settle_samples = (SETTLE_SECS * buffer_config.sample_rate) as i64;
        self.offline = buffer_config.process_mode == ProcessMode::Offline;

        // Spawn the worker once, moving the parsed manager into it. Then do ONE
        // synchronous generation from the (possibly host-restored) param values
        // so playback starts on the correct pattern with no first-buffer regen.
        if self.worker.is_none() {
            // The manager is parked here by `default()` and MOVED into the worker,
            // so after a deactivate/initialize cycle it is gone — re-parse then.
            // ponytail: re-parsing the embedded JSON on a re-init costs a few ms
            // on the main thread; keeping a second copy alive just to avoid it
            // would double the library's memory for a once-per-session event.
            let gen = self.gen_manager.take().unwrap_or_else(GenerationManager::new);
            let worker = GenWorker::spawn(gen);
            // Real transport tempo isn't known yet; first process() will
            // regenerate if it differs (off the audio thread).
            let desired = self.inputs(120.0, (0, 0));
            let g = self.next_gen();
            worker.request(desired.to_request(g));
            if let Some(p) = worker.recv_blocking() {
                self.current = p;
                // Not the audio thread yet — a plain lock is fine here.
                // A poisoned lock (editor panicked) still yields the slot.
                *self.pattern_view.lock().unwrap_or_else(|e| e.into_inner()) =
                    self.current.clone();
            }
            self.requested = desired;
            self.last_desired = desired;
            self.worker = Some(worker);
        }

        // Warm the bank: a restored project has snapshots but no patterns, and
        // a slot with no pattern is a dead pad. Not the audio thread yet, so a
        // plain lock is fine. Warms even when the host claims Offline — Bitwig
        // labels its LIVE engine that way (see pump_slots).
        self.bank_epoch_seen = self.params.bank.epoch.load(Ordering::Relaxed);
        let filled = self.params.bank.lock().filled_mask();
        // Republish which pads hold a snapshot: the trigger gate reads this, and
        // a restored project must accept a press before anything else happens.
        self.params.bank.stored.store(filled as u32, Ordering::Relaxed);
        self.slot_dirty = filled;
        self.dirty_auto_only = false;
        if filled != 0 {
            self.log(worker::LogEvent::Dirty { mask: filled, cause: 3 });
        }

        nih_log!("drumgen v{} initialized (sr {})", Self::VERSION, buffer_config.sample_rate);
        true
    }

    fn reset(&mut self) {
        self.active = 0;
        self.was_playing = false;
        self.last_end_samples = None;
        self.playhead_tick.store(-1, Ordering::Relaxed);
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // 0. Drain MIDI input first — `next_event` borrows the context mutably,
        // and the transport snapshot below borrows it too. Notes 36..51 queue a
        // bank slot; everything else is consumed and dropped (this plugin has
        // never forwarded its input).
        let mut midi_trigger: Option<i32> = None;
        while let Some(event) = context.next_event() {
            if let NoteEvent::NoteOn { note, velocity, .. } = event {
                // nih-plug velocity is 0.0..1.0; a zero-velocity NoteOn is a
                // NoteOff in disguise and must not fire a slot.
                let slot = note.wrapping_sub(params::FIRST_TRIGGER_NOTE) as usize;
                if note >= params::FIRST_TRIGGER_NOTE
                    && slot < params::BANK_SLOTS
                    && velocity > 0.0
                {
                    midi_trigger = Some(slot as i32);
                }
            }
        }

        // 1. Pick up any freshly generated pattern (audio-thread safe, drains to newest).
        // Anything this thread lets go of is handed to the worker's bin so the
        // free() happens over there — see GenWorker::retire.
        if let Some(w) = &self.worker {
            if let Some(p) = w.try_recv_latest() {
                if let Some(stale) = self.pending.replace(p) {
                    w.retire(stale);
                }
            }
            // Bank deliveries are drained in full — unlike the live pattern,
            // every slot matters, so nothing here is latest-wins.
            while let Some((i, p)) = w.try_recv_slot() {
                let i = i as usize;
                if i >= params::BANK_SLOTS {
                    continue;
                }
                if let Some(stale) = self.slot_patterns[i].replace(p) {
                    w.retire(stale);
                }
            }
        }
        // A regenerated ACTIVE slot (tempo moved, or it was re-stored) takes
        // over at once, keeping its origin so the phrase does not re-phase.
        let mut bank_swapped = false;
        if self.active_slot >= 0 {
            let i = self.active_slot as usize;
            if let Some(p) = self.slot_patterns.get(i).and_then(|p| p.as_ref()) {
                if !Arc::ptr_eq(p, &self.current) {
                    let fresh = p.clone();
                    let stale = std::mem::replace(&mut self.current, fresh);
                    if let Some(w) = &self.worker {
                        w.retire(stale);
                    }
                    bank_swapped = true;
                }
            }
        }
        // Publish the newest pattern for the GUI. try_lock so the audio thread
        // never blocks on the editor; a contended buffer just retries next
        // buffer. In bank mode the pending params pattern is NOT what is
        // sounding, so the GUI must see `current` instead.
        {
            let newest = if self.active_slot >= 0 {
                &self.current
            } else {
                self.pending.as_ref().unwrap_or(&self.current)
            };
            let mut slot = match self.pattern_view.try_lock() {
                Ok(v) => Some(v),
                // Editor thread panicked while holding the guard: the data is
                // just an Arc, still valid — keep publishing instead of dying.
                Err(std::sync::TryLockError::Poisoned(p)) => Some(p.into_inner()),
                Err(std::sync::TryLockError::WouldBlock) => None,
            };
            if let Some(view) = slot.as_deref_mut() {
                // Skip the clone (and the retire) when the slot already holds
                // the newest — the common case, once per buffer.
                if !Arc::ptr_eq(view, newest) {
                    let stale = std::mem::replace(view, newest.clone());
                    if let Some(w) = &self.worker {
                        w.retire(stale);
                    }
                }
            }
        }

        // 2. Snapshot transport (scope the borrow so it releases before emitting).
        let (playing, tempo, sample_rate, pos_beats, pos_samples, host_meter) = {
            let transport = context.transport();
            // Fall back to the last-known tempo (not a hard 120) when the host
            // reports none, so a stopped-transport SAVE .MID keeps the real BPM.
            let t = transport.tempo.unwrap_or(self.requested.tempo as f64);
            // Host time signature — drives METER Auto so the pattern follows
            // the project's meter changes live.
            let hm = match (transport.time_sig_numerator, transport.time_sig_denominator) {
                (Some(n), Some(d)) if n > 0 && d > 0 => (n, d),
                _ => (0, 0),
            };
            (
                transport.playing,
                if t <= 0.0 { 120.0 } else { t },
                transport.sample_rate as f64,
                transport.pos_beats(),
                transport.pos_samples(),
                hm,
            )
        };
        let num_samples = buffer.samples() as i64;

        // 3. Change detection (runs whether or not playing).
        // Discrete params (style/bars/seed/meter) regenerate IMMEDIATELY so the
        // dice and pickers feel instant. Only the continuous knobs (humanize/
        // swing) and tempo use the settle debounce to avoid a regen storm on drag.
        //
        // `requested` is only advanced when the worker ACCEPTED the request; a
        // dropped send (full queue) leaves it stale so the change is re-detected
        // and re-sent next buffer instead of silently ignored forever.
        let desired = self.inputs(tempo as f32, host_meter);
        let knob_changed = (desired.humanize - self.requested.humanize).abs() > 1e-4
            || (desired.swing - self.requested.swing).abs() > 1e-4;
        let tempo_changed = (desired.tempo - self.requested.tempo).abs() > 1.0;

        let mut sent_now = false;
        match classify_change(&desired, &self.requested) {
            ChangeKind::UserDiscrete => {
                let g = self.next_gen();
                let sent =
                    self.worker.as_ref().is_some_and(|w| w.request(desired.to_request(g)));
                if sent {
                    self.requested = desired;
                    self.settle_remaining = 0;
                    sent_now = true;
                    // The user reached for the params: playback goes back to them.
                    if self.active_slot >= 0 || self.queued_slot >= 0 {
                        self.log(worker::LogEvent::BankExit { knob: false });
                    }
                    self.exit_bank();
                }
            }
            ChangeKind::HostMeter => {
                // Bitwig moved its time signature. The live pattern follows it
                // (immediately — a meter is a promise), but the HOST is not the
                // user: the playing pad survives, and Auto pads regenerate so
                // they re-bake to the new meter. Forced pads keep their
                // patterns — regenerating them would swap in identical notes
                // with a needless flush.
                let g = self.next_gen();
                let sent =
                    self.worker.as_ref().is_some_and(|w| w.request(desired.to_request(g)));
                if sent {
                    self.requested = desired;
                    self.settle_remaining = 0;
                    sent_now = true;
                    self.slot_dirty = u16::MAX;
                    self.dirty_auto_only = true;
                    self.log(worker::LogEvent::HostMeter(desired.meter.0, desired.meter.1));
                    self.log(worker::LogEvent::Dirty { mask: u16::MAX, cause: 2 });
                }
            }
            ChangeKind::Continuous => {
                if desired.changed(&self.last_desired) {
                    self.settle_remaining = self.settle_samples;
                }
                self.settle_remaining -= num_samples;
                if self.settle_remaining <= 0 {
                    let g = self.next_gen();
                    let sent =
                        self.worker.as_ref().is_some_and(|w| w.request(desired.to_request(g)));
                    if sent {
                        self.requested = desired;
                        sent_now = true;
                        if knob_changed {
                            // A knob is the user; tempo alone is the host and
                            // must not kill the slot being jammed.
                            if self.active_slot >= 0 || self.queued_slot >= 0 {
                                self.log(worker::LogEvent::BankExit { knob: true });
                            }
                            self.exit_bank();
                        }
                        if tempo_changed {
                            // Patterns bake tempo (humanization is ms-based), so
                            // every stored slot is now wrong. Empty bits clear
                            // themselves in the pump.
                            self.slot_dirty = u16::MAX;
                            self.dirty_auto_only = false;
                            self.log(worker::LogEvent::Dirty { mask: u16::MAX, cause: 1 });
                        }
                    }
                }
            }
            ChangeKind::None => {}
        }
        self.last_desired = desired;

        // 3b. Bank: a changed epoch means the GUI stored, cleared or restored a
        // slot — re-generate everything rather than tracking which cell moved.
        let epoch = self.params.bank.epoch.load(Ordering::Relaxed);
        if epoch != self.bank_epoch_seen {
            self.bank_epoch_seen = epoch;
            self.slot_dirty = u16::MAX;
            self.dirty_auto_only = false;
            self.log(worker::LogEvent::Dirty { mask: u16::MAX, cause: 0 });
        }
        self.pump_slots(desired.tempo, host_meter);

        // 3c. Trigger intake. A GUI click and a MIDI note are the same event;
        // MIDI wins when both land in one buffer (the pads are the live surface).
        let gui_trigger = self.params.bank.trigger.swap(0, Ordering::Relaxed);
        let candidate = midi_trigger.or_else(|| {
            (gui_trigger > 0).then(|| gui_trigger - 1)
        });
        if let Some(slot) = candidate {
            // Gate on STORED, not on "the pattern has arrived": a press during
            // the generation window must still take, or the pad looks dead for
            // no visible reason. The swap below simply waits for the pattern.
            let stored = self.params.bank.stored.load(Ordering::Relaxed) & (1 << slot) != 0;
            // An empty pad and a re-press of what is already playing both do
            // nothing — silence would be a worse answer to a mis-hit pad.
            let accepted = stored && slot != self.active_slot;
            if accepted {
                self.queued_slot = slot;
            }
            self.log(worker::LogEvent::Trigger {
                slot: slot as u8,
                midi: midi_trigger.is_some(),
                accepted,
            });
        }

        // Offline render: this thread is not real time (the host is rendering
        // as fast as it can), so wait for the pattern we just asked for instead
        // of baking the previous one into the next few buffers of the bounce.
        if sent_now && self.offline {
            self.await_pending();
        }

        // 4. Stopped: flush once, apply any pending swap immediately, silence.
        if !playing {
            self.playhead_tick.store(-1, Ordering::Relaxed);
            if self.was_playing {
                Self::flush(&mut self.active, context, 0);
                self.was_playing = false;
                self.log(worker::LogEvent::Playing(false));
            }
            // A slot armed while stopped applies at once — there is no bar to
            // wait for, and it anchors to the timeline like any other pattern.
            if self.queued_slot >= 0 {
                if let Some(p) = self.slot_at(self.queued_slot) {
                    let stale = std::mem::replace(&mut self.current, p);
                    if let Some(w) = &self.worker {
                        w.retire(stale);
                    }
                    self.active_slot = self.queued_slot;
                    self.origin = 0.0;
                    self.queued_slot = -1;
                    self.log(worker::LogEvent::Swap { slot: self.active_slot as u8, origin: 0 });
                }
                // Still generating: stay queued rather than eating the press.
            } else if self.active_slot < 0 {
                if let Some(p) = self.pending.take() {
                    let stale = std::mem::replace(&mut self.current, p);
                    if let Some(w) = &self.worker {
                        w.retire(stale);
                    }
                }
            }
            self.publish_bank();
            self.last_end_samples = None;
            silence_buffer(buffer);
            return ProcessStatus::Normal;
        }

        // 5. Musical position from the DAW beat clock (falls back to samples).
        let ppq = PPQ as f64;
        let ticks_per_sample = tempo * ppq / (60.0 * sample_rate);
        let samples_per_tick = if ticks_per_sample > 0.0 { 1.0 / ticks_per_sample } else { 0.0 };
        let abs_tick_start = match pos_beats {
            Some(b) => b * ppq,
            None => pos_samples.unwrap_or(0) as f64 * ticks_per_sample,
        };
        let buffer_ticks = num_samples as f64 * ticks_per_sample;

        // 6. Discontinuity (locate / loop jump): expected start == last buffer's end.
        let just_started = !self.was_playing;
        self.was_playing = true;
        if just_started {
            self.log(worker::LogEvent::Playing(true));
        }
        let discontinuity = match (pos_samples, self.last_end_samples) {
            (Some(cur), Some(prev)) => (cur - prev).abs() > 8,
            _ => false,
        };

        // 7. Swap in a pending pattern immediately (with a note flush) so style/
        // dice/meter changes are heard right away. A mid-bar swap is fine — the
        // new pattern is anchored to the same absolute transport position.
        // ponytail: immediate swap over bar-boundary gating — the gating could
        // strand a pending pattern on 1-bar/looping material. Bar-quantized swap
        // is a musical nicety to revisit, not a correctness need.
        // Flush on a locate/loop jump. Also flush on transport start if the mask
        // is dirty: the stop branch clears it, but a host that yanks the
        // transport (or a swap mid-buffer right before a stop) can leak a bit,
        // and a stuck drum note is silent-but-real state the sampler holds.
        let mut need_flush = (discontinuity && !just_started) || (just_started && self.active != 0)
            || bank_swapped;
        // In bank mode a freshly generated params pattern waits in `pending`:
        // the slot is what the user asked to hear, and it takes over the moment
        // a knob move exits bank mode.
        if self.active_slot < 0 {
            if let Some(p) = self.pending.take() {
                let stale = std::mem::replace(&mut self.current, p);
                if let Some(w) = &self.worker {
                    w.retire(stale);
                }
                need_flush = true;
                // Params patterns are anchored to the timeline, as before.
                self.origin = 0.0;
            }
        }

        // 7b. Bank switch, quantized to the next bar of what is playing now.
        // Recomputed every buffer (never latched) so a loop jump or locate
        // cannot strand a queued slot on a boundary that no longer arrives.
        let mut swap_timing = 0u32;
        if self.queued_slot >= 0 {
            let boundary = next_bar_boundary(&self.current.bar_starts, self.origin, abs_tick_start);
            if boundary < abs_tick_start + buffer_ticks {
                if let Some(p) = self.slot_at(self.queued_slot) {
                    let stale = std::mem::replace(&mut self.current, p);
                    if let Some(w) = &self.worker {
                        w.retire(stale);
                    }
                    // The new slot's bar 1 lands exactly on the boundary.
                    self.origin = boundary;
                    self.active_slot = self.queued_slot;
                    need_flush = true;
                    // ponytail: the swap happens at buffer granularity, so the
                    // outgoing bar loses up to one buffer (~5ms) of its tail and
                    // the flush lands at the boundary's sample rather than
                    // splitting the window. Upgrade path: two playback::scan
                    // calls, one per pattern, split at `swap_timing`.
                    swap_timing = (((boundary - abs_tick_start) * samples_per_tick) as i64)
                        .clamp(0, num_samples.saturating_sub(1))
                        as u32;
                    self.queued_slot = -1;
                    self.log(worker::LogEvent::Swap {
                        slot: self.active_slot as u8,
                        origin: boundary as i64,
                    });
                }
                // No pattern yet (still generating): stay queued and take the
                // NEXT boundary. Dropping the press here is what made a pad
                // look dead — the press was real, it just had nothing to play.
            }
        }
        if need_flush {
            Self::flush(&mut self.active, context, swap_timing);
        }
        self.publish_bank();

        // 8. Compute the buffer's pattern window against the (possibly new) pattern.
        let total_ticks = self.current.total_ticks.max(1);
        // `origin` is where this pattern's bar 1 sits on the timeline. On the
        // buffer a slot switch lands in, `abs - origin` is negative (the
        // boundary is still ahead of the buffer start) — playback::scan handles
        // a negative p0 and places bar 1's downbeat at its exact sample, so the
        // kick on 1 is neither dropped nor early. Every other buffer wraps.
        let raw = abs_tick_start - self.origin;
        let p0 = if raw < 0.0 { raw } else { raw.rem_euclid(total_ticks as f64) };

        // Telegraph/cursor: publish the playhead tick. One relaxed store —
        // nothing else is allowed on this thread (the GUI derives the bar).
        self.playhead_tick.store(p0.max(0.0).round() as i64, Ordering::Relaxed);

        // 9. Scan events into the reused scratch (no allocation after warmup).
        self.scratch.clear();
        playback::scan(&self.current, p0, buffer_ticks, samples_per_tick, num_samples, &mut self.scratch);

        // 10. Emit and track active notes.
        for e in &self.scratch {
            if e.is_note_on {
                context.send_event(NoteEvent::NoteOn {
                    timing: e.timing,
                    voice_id: None,
                    channel: MIDI_CHANNEL,
                    note: e.note,
                    velocity: e.velocity as f32 / 127.0,
                });
                if e.note < 128 {
                    self.active |= 1u128 << e.note;
                }
            } else {
                context.send_event(NoteEvent::NoteOff {
                    timing: e.timing,
                    voice_id: None,
                    channel: MIDI_CHANNEL,
                    note: e.note,
                    velocity: 0.0,
                });
                if e.note < 128 {
                    self.active &= !(1u128 << e.note);
                }
            }
        }

        self.last_end_samples = pos_samples.map(|s| s + num_samples);
        silence_buffer(buffer);
        ProcessStatus::Normal
    }

    fn deactivate(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.shutdown();
        }
        self.active = 0;
        nih_log!("drumgen deactivated");
    }
}

impl Vst3Plugin for Drumgen {
    const VST3_CLASS_ID: [u8; 16] = *b"drumgenDRUMGEN00";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Instrument,
        Vst3SubCategory::Generator,
        Vst3SubCategory::Drum,
    ];
}

impl ClapPlugin for Drumgen {
    const CLAP_ID: &'static str = "com.drumgen.drumgen-vst";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Algorithmic drum pattern generator");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::NoteEffect,
        ClapFeature::Drum,
    ];
}

fn silence_buffer(buffer: &mut Buffer) {
    for channel_samples in buffer.iter_samples() {
        for sample in channel_samples {
            *sample = 0.0;
        }
    }
}

nih_export_vst3!(Drumgen);
nih_export_clap!(Drumgen);

#[cfg(test)]
mod tests {
    use super::*;

    /// Two 4/4 bars: starts at 0 and 1920, terminal entry at 3840.
    const TWO_BARS: [i64; 3] = [0, 1920, 3840];

    fn snap_444() -> ParamSnapshot {
        ParamSnapshot {
            style: 3,
            humanize: 0.4,
            bars: 4,
            seed: 7,
            swing: 0.0,
            meter: (4, 4),
            meter_index: 0, // Auto, resolved against a 4/4 host
            fill: 2,
            song: 1,
            tempo: 168.0,
        }
    }

    #[test]
    fn host_meter_flip_is_not_a_user_tweak() {
        // THE field bug: Bitwig 4/4 → 3/4 under METER=Auto changed the
        // effective meter, which the old code read as a discrete tweak and
        // used to kick the playing pad out of the bank. The flip must be its
        // own kind so the pad survives and Auto pads re-bake.
        let requested = snap_444();
        let flipped = ParamSnapshot { meter: (3, 4), ..requested };
        assert_eq!(classify_change(&flipped, &requested), ChangeKind::HostMeter);

        // The user touching the METER stepper is a tweak, even when it lands
        // on the same effective meter (Auto@4/4 host → forced 4/4).
        let forced = ParamSnapshot { meter_index: 2, ..requested };
        assert_eq!(classify_change(&forced, &requested), ChangeKind::UserDiscrete);

        // And a user meter change WITH a different effective meter is still
        // the user, not the host.
        let forced34 = ParamSnapshot { meter_index: 1, meter: (3, 4), ..requested };
        assert_eq!(classify_change(&forced34, &requested), ChangeKind::UserDiscrete);

        // Knobs and tempo stay continuous; identical snapshots are None.
        assert_eq!(
            classify_change(&ParamSnapshot { tempo: 172.0, ..requested }, &requested),
            ChangeKind::Continuous
        );
        assert_eq!(classify_change(&requested, &requested), ChangeKind::None);
    }

    #[test]
    fn plan_pump_reports_which_dirty_slots_are_auto_meter() {
        // A host meter flip regenerates ONLY Auto pads: forced pads' patterns
        // are still valid, and swapping in identical notes costs a flush.
        let mut bank = params::BankState::empty();
        let auto_pad = params::SlotSnapshot {
            style_name: "a".into(),
            style_index: 0,
            humanize: 0.4,
            bars: 4,
            seed: 1,
            swing: 0.0,
            meter: 0, // Auto
            fill: 0,
            song: 0,
        };
        bank.slots[0] = Some(auto_pad.clone());
        bank.slots[1] = Some(params::SlotSnapshot { meter: 2, ..auto_pad }); // forced 4/4
        let (gens, empty, auto) = plan_pump(u16::MAX, &bank);
        assert!(gens[0].is_some() && gens[1].is_some());
        assert_eq!(auto, 0b01, "only pad 1 follows the host");
        assert_eq!(empty, u16::MAX & !0b11);
    }

    /// Repro of the field failure (S0003 R0000): pads stored while SONG MODE
    /// was on never became ready. Slots whose snapshot carries song > 0 build
    /// through generate_arrangement — the first cycle test only covered loop
    /// mode, and the screenshots that caught this both had a song active.
    #[test]
    fn a_slot_stored_in_song_mode_still_delivers() {
        use crate::generation::GenerationManager;
        use crate::worker::GenWorker;

        let gen = GenerationManager::new();
        let styles = gen.style_names();
        let euro = styles.iter().position(|s| s == "euro_screamo").unwrap_or(0);
        let bank = params::BankShared::new(styles.clone());
        // The user's exact stored state: euro_screamo, Verse/Chor, humanize
        // 0.75, fill Every 8, host 4/4, seed 2521.
        bank.lock().slots[1] = Some(params::SlotSnapshot {
            style_name: styles[euro].clone(),
            style_index: euro as i32,
            humanize: 0.75,
            bars: 4,
            seed: 2521,
            swing: 0.0,
            meter: 0,
            fill: 1,
            song: 1, // Verse/Chor — the difference from the loop-mode test
        });
        bank.bump();

        let (gens, _, _) = plan_pump(u16::MAX, &bank.lock());
        let g = gens[1].expect("song-mode slot must produce a request");
        let worker = GenWorker::spawn(gen);
        assert!(worker.request_slot(1, g.to_request(168.0, (4, 4), 1)));

        let mut got = None;
        for _ in 0..2000 {
            if let Some((i, p)) = worker.try_recv_slot() {
                got = Some((i, p));
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let (i, p) = got.expect("song-mode slot pattern must be delivered — a silent panic here is the R0000 bug");
        assert_eq!(i, 1);
        assert!(!p.events.is_empty());
        worker.shutdown();
    }

    /// The full store → pump → worker → deliver → trigger cycle, minus the DAW.
    /// This is the path a pad click actually takes: if it breaks, every trigger
    /// is silently swallowed by the `filled` check and the pads look dead.
    #[test]
    fn a_stored_slot_becomes_a_triggerable_pattern() {
        use crate::generation::GenerationManager;
        use crate::worker::GenWorker;

        let gen = GenerationManager::new();
        let styles = gen.style_names();
        let bank = params::BankShared::new(styles.clone());

        // What the GUI does on STORE.
        bank.lock().slots[0] = Some(params::SlotSnapshot {
            style_name: styles[0].clone(),
            style_index: 0,
            humanize: 0.4,
            bars: 4,
            seed: 7,
            swing: 0.0,
            meter: 0,
            fill: 2,
            song: 0,
        });
        bank.bump();

        // What the audio thread does on the epoch change.
        let (gens, empty, _) = plan_pump(u16::MAX, &bank.lock());
        assert!(gens[0].is_some(), "the stored slot must produce a request");
        assert_eq!(empty & 1, 0, "a filled slot is never treated as empty");
        assert_eq!(empty, u16::MAX - 1, "the other fifteen pads are empty");

        let worker = GenWorker::spawn(gen);
        for (i, g) in gens.iter().enumerate() {
            let Some(g) = g else { continue };
            assert!(worker.request_slot(i as u8, g.to_request(120.0, (4, 4), i as u64 + 1)));
        }

        // What the audio thread does when the worker delivers.
        let mut slot_patterns: [Option<Arc<Pattern>>; params::BANK_SLOTS] = Default::default();
        for _ in 0..2000 {
            while let Some((i, p)) = worker.try_recv_slot() {
                slot_patterns[i as usize] = Some(p);
            }
            if slot_patterns[0].is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        // The gate every trigger passes through.
        let filled = slot_patterns.first().is_some_and(|p| p.is_some());
        assert!(filled, "a stored pad must end up holding a pattern, or it can never fire");
        assert!(!slot_patterns[0].as_ref().unwrap().events.is_empty());
        worker.shutdown();
    }

    #[test]
    fn boundary_is_the_next_bar_from_mid_bar() {
        assert_eq!(next_bar_boundary(&TWO_BARS, 0.0, 100.0), 1920.0);
        assert_eq!(next_bar_boundary(&TWO_BARS, 0.0, 2000.0), 3840.0);
    }

    #[test]
    fn boundary_on_a_barline_is_the_following_bar() {
        // Strictly after: a trigger that lands exactly on the downbeat waits
        // for the NEXT one, otherwise the swap races the notes at that tick.
        assert_eq!(next_bar_boundary(&TWO_BARS, 0.0, 1920.0), 3840.0);
        assert_eq!(next_bar_boundary(&TWO_BARS, 0.0, 0.0), 1920.0);
    }

    #[test]
    fn boundary_wraps_past_the_end_of_the_pattern() {
        // Bar 2 of loop 1 must give the start of loop 2, not a tick inside it.
        assert_eq!(next_bar_boundary(&TWO_BARS, 0.0, 3900.0), 5760.0);
        assert_eq!(next_bar_boundary(&TWO_BARS, 0.0, 7000.0), 7680.0);
    }

    #[test]
    fn boundary_respects_a_nonzero_origin() {
        // After a slot switch at tick 1920, the pattern's bars sit at 1920,
        // 3840, ... — boundaries must follow the slot, not the timeline.
        assert_eq!(next_bar_boundary(&TWO_BARS, 1920.0, 2000.0), 3840.0);
        assert_eq!(next_bar_boundary(&TWO_BARS, 500.0, 600.0), 2420.0);
    }

    #[test]
    fn boundary_handles_positions_before_the_origin() {
        // Host pre-roll reports negative beats, and a slot triggered mid-song
        // has an origin later than a locate back to the top.
        assert_eq!(next_bar_boundary(&TWO_BARS, 0.0, -100.0), 0.0);
        assert_eq!(next_bar_boundary(&TWO_BARS, 2000.0, -100.0), 80.0);
        // Never returns a boundary at or before `abs`.
        for abs in [-5000.0, -1.0, 0.0, 1.0, 1919.9, 12345.6] {
            assert!(next_bar_boundary(&TWO_BARS, 777.0, abs) > abs, "abs {abs}");
        }
    }

    #[test]
    fn boundary_follows_a_mixed_meter_bar_map() {
        // Song mode patterns change meter mid-pattern: 7/8 (1680) then 4/4.
        let mixed = [0i64, 1680, 3600, 5520];
        assert_eq!(next_bar_boundary(&mixed, 0.0, 10.0), 1680.0);
        assert_eq!(next_bar_boundary(&mixed, 0.0, 1700.0), 3600.0);
        assert_eq!(next_bar_boundary(&mixed, 0.0, 5000.0), 5520.0);
    }
}
