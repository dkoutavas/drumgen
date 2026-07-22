use nih_plug::prelude::*;
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
    fill: i32,
    tempo: f32,
}

impl ParamSnapshot {
    fn changed(&self, o: &ParamSnapshot) -> bool {
        self.style != o.style
            || self.bars != o.bars
            || self.seed != o.seed
            || self.meter != o.meter
            || self.fill != o.fill
            || (self.humanize - o.humanize).abs() > 1e-4
            || (self.swing - o.swing).abs() > 1e-4
            // Tempo affects ms-based humanization; regenerate past a 1 BPM step.
            || (self.tempo - o.tempo).abs() > 1.0
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
            fill: params.fill.value(),
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
            &res, 0, snap.seed as u64, style_name, String::new(),
        ));

        let pattern_view = Arc::new(Mutex::new(current.clone()));

        Self {
            params,
            current,
            pending: None,
            pattern_view,
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
            n_styles,
        }
    }
}

impl Drumgen {
    fn inputs(&self, tempo: f32, host_meter: (i32, i32)) -> ParamSnapshot {
        ParamSnapshot {
            style: self.params.style.value(),
            humanize: self.params.humanize.value(),
            bars: self.params.bars.value(),
            seed: self.params.seed.value(),
            swing: self.params.swing.value(),
            meter: params::effective_meter(self.params.meter.value(), host_meter),
            fill: self.params.fill.value(),
            tempo,
        }
    }

    fn next_gen(&mut self) -> u64 {
        self.gen_counter += 1;
        self.gen_counter
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

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
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
        editor::create(self.params.clone(), self.n_styles, self.pattern_view.clone())
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        self.settle_samples = (SETTLE_SECS * buffer_config.sample_rate) as i64;

        // Spawn the worker once, moving the parsed manager into it. Then do ONE
        // synchronous generation from the (possibly host-restored) param values
        // so playback starts on the correct pattern with no first-buffer regen.
        if self.worker.is_none() {
            if let Some(gen) = self.gen_manager.take() {
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
        }

        nih_log!("drumgen v{} initialized (sr {})", Self::VERSION, buffer_config.sample_rate);
        true
    }

    fn reset(&mut self) {
        self.active = 0;
        self.was_playing = false;
        self.last_end_samples = None;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // 1. Pick up any freshly generated pattern (audio-thread safe, drains to newest).
        if let Some(w) = &self.worker {
            if let Some(p) = w.try_recv_latest() {
                self.pending = Some(p);
            }
        }
        // Publish the newest pattern for the GUI. try_lock so the audio thread
        // never blocks on the editor; a contended buffer just retries next
        // buffer. Runs unconditionally — an Arc clone when the slot is stale,
        // effectively a refcount touch when it already holds the newest.
        {
            let newest = self.pending.as_ref().unwrap_or(&self.current);
            match self.pattern_view.try_lock() {
                Ok(mut view) => *view = newest.clone(),
                // Editor thread panicked while holding the guard: the data is
                // just an Arc, still valid — keep publishing instead of dying.
                Err(std::sync::TryLockError::Poisoned(p)) => *p.into_inner() = newest.clone(),
                Err(std::sync::TryLockError::WouldBlock) => {}
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
        let desired = self.inputs(tempo as f32, host_meter);
        let discrete_changed = desired.style != self.requested.style
            || desired.bars != self.requested.bars
            || desired.seed != self.requested.seed
            || desired.meter != self.requested.meter
            || desired.fill != self.requested.fill;
        let continuous_changed = (desired.humanize - self.requested.humanize).abs() > 1e-4
            || (desired.swing - self.requested.swing).abs() > 1e-4
            || (desired.tempo - self.requested.tempo).abs() > 1.0;

        // `requested` is only advanced when the worker ACCEPTED the request; a
        // dropped send (full queue) leaves it stale so the change is re-detected
        // and re-sent next buffer instead of silently ignored forever.
        if discrete_changed {
            let g = self.next_gen();
            let sent = self.worker.as_ref().is_some_and(|w| w.request(desired.to_request(g)));
            if sent {
                self.requested = desired;
                self.settle_remaining = 0;
            }
        } else if continuous_changed {
            if desired.changed(&self.last_desired) {
                self.settle_remaining = self.settle_samples;
            }
            self.settle_remaining -= num_samples;
            if self.settle_remaining <= 0 {
                let g = self.next_gen();
                let sent = self.worker.as_ref().is_some_and(|w| w.request(desired.to_request(g)));
                if sent {
                    self.requested = desired;
                }
            }
        }
        self.last_desired = desired;

        // 4. Stopped: flush once, apply any pending swap immediately, silence.
        if !playing {
            if self.was_playing {
                Self::flush(&mut self.active, context, 0);
                self.was_playing = false;
            }
            if let Some(p) = self.pending.take() {
                self.current = p;
            }
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
        let mut need_flush = discontinuity && !just_started;
        if let Some(p) = self.pending.take() {
            self.current = p;
            need_flush = true;
        }
        if need_flush {
            Self::flush(&mut self.active, context, 0);
        }

        // 8. Compute the buffer's pattern window against the (possibly new) pattern.
        let total_ticks = self.current.total_ticks.max(1);
        let p0 = abs_tick_start.rem_euclid(total_ticks as f64);

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
