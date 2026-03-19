use std::collections::HashSet;
use crate::engine::humanizer::Event;
use crate::engine::midi_math::{self, TimeSigEntry, PPQ, NOTE_DURATION};

/// A single MIDI event at an absolute tick position within the pattern.
#[derive(Debug, Clone)]
pub struct MidiEvent {
    /// Absolute tick position within the pattern (0-based).
    pub tick: i64,
    /// MIDI note number (GM drum map).
    pub note: u8,
    /// MIDI velocity (0-127). 0 is used for note-off events.
    pub velocity: u8,
    /// True for note-on, false for note-off.
    pub is_note_on: bool,
}

/// Manages the event buffer and playback state for the drum pattern.
pub struct PlaybackEngine {
    /// All events in the pattern, sorted by tick.
    events: Vec<MidiEvent>,
    /// Total pattern length in ticks.
    pub total_ticks: i64,
    /// Currently sounding MIDI notes.
    active_notes: HashSet<u8>,
    /// DAW sample rate.
    pub sample_rate: f32,
    /// Pulses per quarter note (always 480).
    pub ppq: i64,
}

impl PlaybackEngine {
    /// Create a PlaybackEngine from assembler events and time signatures.
    pub fn from_events(events: &[Event], time_signatures: &[TimeSigEntry]) -> Self {
        let total_bars = time_signatures.last()
            .map(|ts| ts.bar_end)
            .unwrap_or(4);
        let total_ticks = midi_math::total_pattern_ticks(total_bars, time_signatures, PPQ);

        let mut midi_events = Vec::new();

        for event in events {
            let note = event.instrument.midi_note();
            let velocity = event.velocity.clamp(1, 127) as u8;

            // Note on
            midi_events.push(MidiEvent {
                tick: event.tick,
                note,
                velocity,
                is_note_on: true,
            });

            // Note off (clamped to pattern boundary)
            let off_tick = (event.tick + NOTE_DURATION).min(total_ticks);
            midi_events.push(MidiEvent {
                tick: off_tick,
                note,
                velocity: 0,
                is_note_on: false,
            });
        }

        // Sort: by tick, then note-off before note-on at same tick
        midi_events.sort_by(|a, b| {
            a.tick.cmp(&b.tick)
                .then_with(|| a.is_note_on.cmp(&b.is_note_on))
        });

        PlaybackEngine {
            events: midi_events,
            total_ticks,
            active_notes: HashSet::new(),
            sample_rate: 44100.0,
            ppq: PPQ,
        }
    }

    /// Create a new PlaybackEngine with a hardcoded test pattern (Phase 1 fallback).
    pub fn new() -> Self {
        let num_bars: i64 = 4;
        let beats_per_bar: i64 = 4;
        let total_ticks = num_bars * beats_per_bar * PPQ;

        let mut events = Vec::new();

        for bar in 0..num_bars {
            let bar_offset = bar * beats_per_bar * PPQ;

            for beat in 0..beats_per_bar {
                let beat_tick = bar_offset + beat * PPQ;

                // Kick on beats 1, 3
                if beat == 0 || beat == 2 {
                    events.push(MidiEvent { tick: beat_tick, note: 36, velocity: 100, is_note_on: true });
                    events.push(MidiEvent { tick: beat_tick + NOTE_DURATION, note: 36, velocity: 0, is_note_on: false });
                }

                // Snare on beats 2, 4
                if beat == 1 || beat == 3 {
                    events.push(MidiEvent { tick: beat_tick, note: 38, velocity: 110, is_note_on: true });
                    events.push(MidiEvent { tick: beat_tick + NOTE_DURATION, note: 38, velocity: 0, is_note_on: false });
                }

                // Hi-hat on every 8th
                for sub in 0..2 {
                    let hh_tick = beat_tick + sub * (PPQ / 2);
                    let hh_vel = if sub == 0 { 90 } else { 75 };
                    events.push(MidiEvent { tick: hh_tick, note: 42, velocity: hh_vel, is_note_on: true });
                    events.push(MidiEvent { tick: hh_tick + NOTE_DURATION, note: 42, velocity: 0, is_note_on: false });
                }
            }
        }

        events.sort_by(|a, b| a.tick.cmp(&b.tick).then_with(|| a.is_note_on.cmp(&b.is_note_on)));

        PlaybackEngine {
            events,
            total_ticks,
            active_notes: HashSet::new(),
            sample_rate: 44100.0,
            ppq: PPQ,
        }
    }

    /// Scan for events in the tick range [start_tick, end_tick).
    /// Handles loop wrap-around.
    pub fn scan_events(&mut self, start_tick: i64, end_tick: i64) -> Vec<MidiEvent> {
        let mut result = Vec::new();

        if end_tick <= start_tick || self.total_ticks <= 0 {
            return result;
        }

        let start = start_tick % self.total_ticks;
        let end = end_tick % self.total_ticks;

        if start < end {
            self.collect_events_in_range(start, end, &mut result);
        } else {
            // Wrap-around
            self.collect_events_in_range(start, self.total_ticks, &mut result);
            self.collect_events_in_range(0, end, &mut result);
        }

        // Update active note tracking
        for event in &result {
            if event.is_note_on {
                self.active_notes.insert(event.note);
            } else {
                self.active_notes.remove(&event.note);
            }
        }

        result
    }

    fn collect_events_in_range(&self, start: i64, end: i64, out: &mut Vec<MidiEvent>) {
        let first = self.events.partition_point(|e| e.tick < start);
        for event in &self.events[first..] {
            if event.tick >= end {
                break;
            }
            out.push(event.clone());
        }
    }

    /// Returns currently active MIDI notes and clears the tracking set.
    pub fn all_notes_off(&mut self) -> Vec<u8> {
        let notes: Vec<u8> = self.active_notes.iter().copied().collect();
        self.active_notes.clear();
        notes
    }

    pub fn set_sample_rate(&mut self, rate: f32) {
        self.sample_rate = rate;
    }
}
