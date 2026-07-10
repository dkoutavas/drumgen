//! Immutable, shareable drum pattern — the single data type handed from the
//! generation worker to the audio thread (as `Arc<Pattern>`) and read by the GUI.
//!
//! A `Pattern` is fully baked: MIDI events sorted by tick, bar-start ticks for
//! boundary detection, and the metadata needed for the status line / export.

use crate::engine::assembler::AssembleResult;
use crate::engine::midi_math::{self, TimeSigEntry, NOTE_DURATION, PPQ};

/// A single MIDI event at an absolute tick within the pattern loop.
#[derive(Debug, Clone)]
pub struct MidiEvent {
    /// Tick position within the pattern loop `[0, total_ticks)`.
    pub tick: i64,
    /// MIDI note number (GM drum map).
    pub note: u8,
    /// Velocity 1..127 for note-on, 0 for note-off.
    pub velocity: u8,
    pub is_note_on: bool,
}

/// A baked, immutable pattern ready for playback.
pub struct Pattern {
    /// All events, sorted by (tick, note-off before note-on).
    pub events: Vec<MidiEvent>,
    /// Total loop length in ticks.
    pub total_ticks: i64,
    /// Absolute tick of each bar start, plus a terminal entry == total_ticks.
    /// Length is `total_bars + 1`; always starts with 0.
    pub bar_starts: Vec<i64>,
    pub time_signatures: Vec<TimeSigEntry>,
    /// Monotonic generation counter — GUI rebakes its display when this changes.
    pub generation: u64,
    // ── metadata (status line / export) ──
    pub seed: u64,
    pub style_name: String,
    pub cell_name: String,
}

impl Pattern {
    /// Bake an `AssembleResult` into a `Pattern`.
    pub fn from_assemble(
        res: &AssembleResult,
        generation: u64,
        style_name: String,
        cell_name: String,
    ) -> Self {
        let total_bars = res
            .time_signatures
            .last()
            .map(|ts| ts.bar_end)
            .unwrap_or(res.total_bars.max(1));
        let total_ticks = midi_math::total_pattern_ticks(total_bars, &res.time_signatures, PPQ);

        let mut events: Vec<MidiEvent> = Vec::with_capacity(res.events.len() * 2);
        for ev in &res.events {
            let note = ev.instrument.midi_note();
            let velocity = ev.velocity.clamp(1, 127) as u8;
            events.push(MidiEvent { tick: ev.tick, note, velocity, is_note_on: true });
            // Note-off clamped to *inside* the loop (total_ticks - 1) so it is never
            // dropped by the half-open scan; drum one-shots make the exact off tick
            // cosmetic anyway, and loop-wrap flushes any straggler.
            // ponytail: total_ticks-1 clamp instead of exact-boundary wrap; upgrade to
            // wrap-to-tick-0 only if a sustaining instrument ever needs a true gate.
            let off_tick = (ev.tick + NOTE_DURATION).min((total_ticks - 1).max(0));
            events.push(MidiEvent { tick: off_tick, note, velocity: 0, is_note_on: false });
        }
        events.sort_by(|a, b| a.tick.cmp(&b.tick).then_with(|| a.is_note_on.cmp(&b.is_note_on)));

        // bar_starts[i] = start tick of bar (i+1); terminal entry == total_ticks.
        let mut bar_starts = Vec::with_capacity(total_bars as usize + 1);
        for bar in 1..=(total_bars + 1) {
            bar_starts.push(midi_math::calculate_bar_start_ticks(bar, &res.time_signatures, PPQ));
        }

        Pattern {
            events,
            total_ticks,
            bar_starts,
            time_signatures: res.time_signatures.clone(),
            generation,
            seed: res.seed,
            style_name,
            cell_name,
        }
    }

    /// Zero-based bar index containing `tick` (clamped into the pattern).
    pub fn bar_index_at(&self, tick: i64) -> usize {
        // bar_starts is ascending; last valid bar index is bar_starts.len()-2.
        let n = self.bar_starts.len();
        if n < 2 {
            return 0;
        }
        let idx = self.bar_starts.partition_point(|&b| b <= tick);
        idx.saturating_sub(1).min(n - 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> Pattern {
        // 4 bars of 4/4 → total 7680, bars at 0,1920,3840,5760,(7680).
        Pattern {
            events: vec![],
            total_ticks: 7680,
            bar_starts: vec![0, 1920, 3840, 5760, 7680],
            time_signatures: vec![TimeSigEntry { bar_start: 1, bar_end: 4, numerator: 4, denominator: 4 }],
            generation: 0,
            seed: 0,
            style_name: String::new(),
            cell_name: String::new(),
        }
    }

    #[test]
    fn bar_index_maps_ticks_to_bars() {
        let pat = p();
        assert_eq!(pat.bar_index_at(0), 0);
        assert_eq!(pat.bar_index_at(1919), 0);
        assert_eq!(pat.bar_index_at(1920), 1);
        assert_eq!(pat.bar_index_at(5760), 3);
        assert_eq!(pat.bar_index_at(7679), 3); // clamped to last bar
    }
}
