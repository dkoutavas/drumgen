use nih_plug::prelude::*;
use std::sync::Arc;

pub mod engine;
mod params;
mod playback;
mod generation;

use params::DrumgenParams;
use playback::PlaybackEngine;
use generation::GenerationManager;

/// drumgen VST3 plugin — algorithmic drum pattern generator.
///
/// Architecture: this plugin is a MIDI generator (no audio processing). It reads the
/// DAW transport position and emits MIDI note events that drive a drum sampler on
/// another track (e.g. Ugritone, Addictive Drums, or any GM-mapped drum plugin).
///
/// MIDI routing in Ableton:
///   drumgen (MIDI track) → MIDI To: drum sampler track
struct Drumgen {
    params: Arc<DrumgenParams>,
    playback: PlaybackEngine,
    generation: GenerationManager,
    /// Tracks whether transport was playing in the previous buffer.
    was_playing: bool,
    /// Last known param state — used to detect changes and trigger regeneration.
    last_style: i32,
    last_humanize: f32,
    last_bars: i32,
    last_seed: i32,
    last_swing: f32,
}

impl Default for Drumgen {
    fn default() -> Self {
        let gen = GenerationManager::new();
        let initial_pattern = gen.generate(0, 0.35, 4, 0, 0.0, false);
        let playback = PlaybackEngine::from_events(&initial_pattern.events, &initial_pattern.time_signatures);

        Self {
            params: Arc::new(DrumgenParams::default()),
            playback,
            generation: gen,
            was_playing: false,
            last_style: 0,
            last_humanize: 0.35,
            last_bars: 4,
            last_seed: 0,
            last_swing: 0.0,
        }
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

    // Some DAWs (including Ableton) require audio I/O for a plugin to load,
    // even if it's purely a MIDI effect. Dummy stereo output passes silence.
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

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.playback.set_sample_rate(buffer_config.sample_rate);
        nih_log!("drumgen v{} initialized (sample rate: {}, {} cells, {} styles)",
            Self::VERSION, buffer_config.sample_rate,
            self.generation.num_cells(), self.generation.num_styles());
        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // Check for parameter changes and regenerate if needed
        let style = self.params.style.value();
        let humanize = self.params.humanize.value();
        let bars = self.params.bars.value();
        let seed = self.params.seed.value();
        let swing = self.params.swing.value();

        if style != self.last_style
            || (humanize - self.last_humanize).abs() > 0.001
            || bars != self.last_bars
            || seed != self.last_seed
            || (swing - self.last_swing).abs() > 0.001
        {
            let result = self.generation.generate(
                style, humanize as f64, bars, seed as u64, swing as f64, false,
            );
            self.playback = PlaybackEngine::from_events(&result.events, &result.time_signatures);
            // Sample rate from transport (always available)
            let sr = context.transport().sample_rate;
            self.playback.set_sample_rate(sr);

            self.last_style = style;
            self.last_humanize = humanize;
            self.last_bars = bars;
            self.last_seed = seed;
            self.last_swing = swing;
        }

        // Read transport state — copy values before calling send_event
        let transport = context.transport();
        let playing = transport.playing;
        let tempo = transport.tempo.unwrap_or(120.0);
        let pos_samples = transport.pos_samples().unwrap_or(0);
        let sample_rate = transport.sample_rate;
        let _ = transport;

        // Handle transport stop
        if !playing {
            if self.was_playing {
                let active = self.playback.all_notes_off();
                for note in active {
                    context.send_event(NoteEvent::NoteOff {
                        timing: 0,
                        voice_id: None,
                        channel: 9,
                        note,
                        velocity: 0.0,
                    });
                }
                self.was_playing = false;
            }
            silence_buffer(buffer);
            return ProcessStatus::Normal;
        }

        self.was_playing = true;

        // Convert sample range to tick range
        let num_samples = buffer.samples() as i64;
        let ppq = self.playback.ppq as f64;

        let samples_per_tick = (sample_rate as f64 * 60.0) / (tempo * ppq);
        let start_tick = (pos_samples as f64 / samples_per_tick) as i64;
        let end_tick = ((pos_samples + num_samples) as f64 / samples_per_tick) as i64;

        // Scan for events
        let events = self.playback.scan_events(start_tick, end_tick);

        // Emit MIDI events
        for event in &events {
            let event_tick_in_range = if self.playback.total_ticks > 0 {
                let pattern_start = start_tick % self.playback.total_ticks;
                if event.tick >= pattern_start {
                    event.tick
                } else {
                    event.tick + self.playback.total_ticks
                }
            } else {
                event.tick
            };
            let event_sample = (event_tick_in_range as f64 * samples_per_tick) as i64;
            let offset = (event_sample - pos_samples).max(0) as u32;
            let timing = offset.min((num_samples - 1).max(0) as u32);

            if event.is_note_on {
                context.send_event(NoteEvent::NoteOn {
                    timing,
                    voice_id: None,
                    channel: 9,
                    note: event.note,
                    velocity: event.velocity as f32 / 127.0,
                });
            } else {
                context.send_event(NoteEvent::NoteOff {
                    timing,
                    voice_id: None,
                    channel: 9,
                    note: event.note,
                    velocity: 0.0,
                });
            }
        }

        silence_buffer(buffer);
        ProcessStatus::Normal
    }

    fn deactivate(&mut self) {
        self.playback.all_notes_off();
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
