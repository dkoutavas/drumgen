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
// dead_code: bar_starts/time_signatures/generation/seed/style_name/cell_name are
// not read yet — they feed the planned GUI status line, step-grid preview, and
// SAVE .MID export. Remove the allow when those land.
#[allow(dead_code)]
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
    /// The raw dice seed (the SEED param value), NOT the style-salted seed the
    /// engine consumed — this is what the GUI and .mid export display, and it
    /// always matches the events in THIS pattern (no param-vs-snapshot race).
    pub seed: u64,
    pub tempo: f64,
    pub style_name: String,
    pub cell_name: String,
    /// Song-mode section map: (section_type, bars) in order. Empty in loop
    /// mode. The GUI walks this to label the viewed bar.
    pub sections: Vec<(String, i32)>,
}

impl Pattern {
    /// Bake an `AssembleResult` into a `Pattern`.
    pub fn from_assemble(
        res: &AssembleResult,
        generation: u64,
        display_seed: u64,
        style_name: String,
        cell_name: String,
        sections: Vec<(String, i32)>,
    ) -> Self {
        let total_bars = res
            .time_signatures
            .last()
            .map(|ts| ts.bar_end)
            .unwrap_or(res.total_bars.max(1));
        let total_ticks = midi_math::total_pattern_ticks(total_bars, &res.time_signatures, PPQ);

        let mut raw: Vec<MidiEvent> = Vec::with_capacity(res.events.len() * 2);
        for ev in &res.events {
            let note = ev.instrument.midi_note();
            let velocity = ev.velocity.clamp(1, 127) as u8;
            raw.push(MidiEvent { tick: ev.tick, note, velocity, is_note_on: true });
            // Note-off clamped to *inside* the loop (total_ticks - 1) so it is never
            // dropped by the half-open scan; drum one-shots make the exact off tick
            // cosmetic anyway, and loop-wrap flushes any straggler.
            // ponytail: total_ticks-1 clamp instead of exact-boundary wrap; upgrade to
            // wrap-to-tick-0 only if a sustaining instrument ever needs a true gate.
            let off_tick = (ev.tick + NOTE_DURATION).min((total_ticks - 1).max(0));
            raw.push(MidiEvent { tick: off_tick, note, velocity: 0, is_note_on: false });
        }
        // Scan order: note-ON before note-OFF at the same tick, so a re-trigger
        // is seen while the previous note is still counted as sounding.
        raw.sort_by(|a, b| {
            a.tick
                .cmp(&b.tick)
                .then_with(|| b.is_note_on.cmp(&a.is_note_on))
                .then_with(|| a.note.cmp(&b.note))
        });

        // Overlap prevention: two hits of the same instrument less than
        // NOTE_DURATION apart would emit on,on,off,off — and the FIRST off cuts
        // the second note short. Insert an early note-off just before the
        // re-trigger. Port of midi_engine.write_midi (Python is the reference);
        // an array keyed by note number, never a HashMap, so it stays deterministic.
        let mut active = [i64::MIN; 128];
        let mut events: Vec<MidiEvent> = Vec::with_capacity(raw.len());
        for e in raw {
            let n = e.note as usize;
            if n >= 128 {
                events.push(e);
                continue;
            }
            if e.is_note_on {
                if e.tick < active[n] {
                    events.push(MidiEvent { tick: e.tick, note: e.note, velocity: 0, is_note_on: false });
                }
                active[n] = e.tick + NOTE_DURATION;
            } else {
                active[n] = i64::MIN;
            }
            events.push(e);
        }

        // Emission order: note-OFF before note-ON at the same tick, so the
        // inserted off lands ahead of the re-trigger it protects.
        events.sort_by(|a, b| {
            a.tick
                .cmp(&b.tick)
                .then_with(|| a.is_note_on.cmp(&b.is_note_on))
                .then_with(|| a.note.cmp(&b.note))
        });

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
            seed: display_seed,
            tempo: res.tempo,
            style_name,
            cell_name,
            sections,
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::cell::Instrument;
    use crate::engine::humanizer::Event;

    fn ts(bar_start: i32, bar_end: i32, numerator: i32, denominator: i32) -> TimeSigEntry {
        TimeSigEntry { bar_start, bar_end, numerator, denominator }
    }

    fn result(events: Vec<Event>, time_signatures: Vec<TimeSigEntry>, total_bars: i32) -> AssembleResult {
        AssembleResult { events, tempo: 120.0, time_signatures, seed: 0, total_bars }
    }

    fn hit(tick: i64, instrument: Instrument, velocity: i32) -> Event {
        Event { tick, instrument, velocity }
    }

    fn bake(res: &AssembleResult) -> Pattern {
        Pattern::from_assemble(res, 0, 0, "test".into(), String::new(), Vec::new())
    }

    #[test]
    fn total_ticks_follows_the_meter() {
        // A bar is PPQ * 4 / denominator * numerator ticks.
        let four_four = bake(&result(vec![hit(0, Instrument::Kick, 100)], vec![ts(1, 4, 4, 4)], 4));
        assert_eq!(four_four.total_ticks, 4 * 4 * PPQ);

        let three_four = bake(&result(vec![hit(0, Instrument::Kick, 100)], vec![ts(1, 4, 3, 4)], 4));
        assert_eq!(three_four.total_ticks, 4 * 3 * PPQ);

        // Mixed: 2 bars of 7/8 (7 * PPQ/2 each) then 2 bars of 4/4.
        let mixed = bake(&result(
            vec![hit(0, Instrument::Kick, 100)],
            vec![ts(1, 2, 7, 8), ts(3, 4, 4, 4)],
            4,
        ));
        assert_eq!(mixed.total_ticks, 2 * 7 * PPQ / 2 + 2 * 4 * PPQ);
        // bar_starts has one entry per bar plus a terminal == total_ticks.
        assert_eq!(mixed.bar_starts.len(), 5);
        assert_eq!(mixed.bar_starts[0], 0);
        assert_eq!(*mixed.bar_starts.last().unwrap(), mixed.total_ticks);
    }

    #[test]
    fn velocities_are_clamped_to_the_midi_range() {
        let p = bake(&result(
            vec![
                hit(0, Instrument::Kick, 0),
                hit(240, Instrument::Snare, -5),
                hit(480, Instrument::Crash1, 999),
            ],
            vec![ts(1, 1, 4, 4)],
            1,
        ));
        for e in p.events.iter().filter(|e| e.is_note_on) {
            assert!(
                (1..=127).contains(&e.velocity),
                "note-on velocity {} out of range at tick {}",
                e.velocity,
                e.tick
            );
        }
    }

    #[test]
    fn note_offs_never_escape_the_loop() {
        // A hit right at the end of the last bar would otherwise place its
        // note-off past total_ticks, where the half-open scan drops it — a
        // hung note for the rest of the session.
        let p = bake(&result(
            vec![hit(0, Instrument::Kick, 100), hit(4 * PPQ - 2, Instrument::Snare, 100)],
            vec![ts(1, 1, 4, 4)],
            1,
        ));
        for e in &p.events {
            assert!(e.tick < p.total_ticks, "event at {} escapes {}", e.tick, p.total_ticks);
        }
    }

    #[test]
    fn re_triggers_get_an_early_note_off() {
        // Two kicks 10 ticks apart, inside the 30-tick note length. Without
        // overlap prevention the stream is on,on,off,off and the FIRST off
        // silences the second kick.
        let p = bake(&result(
            vec![hit(0, Instrument::Kick, 100), hit(10, Instrument::Kick, 100)],
            vec![ts(1, 1, 4, 4)],
            1,
        ));
        let kick = Instrument::Kick.midi_note();
        let stream: Vec<(i64, bool)> = p
            .events
            .iter()
            .filter(|e| e.note == kick)
            .map(|e| (e.tick, e.is_note_on))
            .collect();
        // The first note's own off (tick 30) still lands, clipping the second
        // note to 20 ticks. That is exactly what midi_engine.write_midi does,
        // and for drum one-shots the note length is cosmetic — what matters is
        // the clean off/on pair at tick 10 so the second kick actually speaks.
        assert_eq!(
            stream,
            vec![(0, true), (10, false), (10, true), (30, false), (40, false)],
            "expected an early note-off at the re-trigger"
        );
    }

    #[test]
    fn spaced_hits_are_left_alone() {
        // Same instrument well beyond NOTE_DURATION: no extra note-off.
        let p = bake(&result(
            vec![hit(0, Instrument::Kick, 100), hit(240, Instrument::Kick, 100)],
            vec![ts(1, 1, 4, 4)],
            1,
        ));
        let kick = Instrument::Kick.midi_note();
        let offs = p.events.iter().filter(|e| e.note == kick && !e.is_note_on).count();
        assert_eq!(offs, 2, "one note-off per hit, no spurious extras");
    }
}
