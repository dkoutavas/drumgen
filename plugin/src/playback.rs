//! Stateless, allocation-free event scanning over an immutable `Pattern`.
//!
//! The audio thread calls [`scan`] once per buffer into a reused scratch `Vec`.
//! Event timings are computed **pattern-relative** to the buffer's start position
//! `p0`, so they stay correct on every loop (the old code subtracted the absolute
//! sample position and collapsed all offsets to 0 after the first loop).

use crate::pattern::Pattern;

/// An event ready to emit: sample offset within the current buffer.
pub struct Emit {
    pub timing: u32,
    pub note: u8,
    pub velocity: u8,
    pub is_note_on: bool,
}

/// Scan the pattern for events in the tick window `[p0, p0 + buffer_ticks)`,
/// wrapping at `total_ticks`, appending `Emit`s to `out` (which the caller
/// clears and reuses — no allocation once its capacity has stabilized).
///
/// `p0` is the fractional pattern-relative tick at the buffer's first sample.
pub fn scan(
    pattern: &Pattern,
    p0: f64,
    buffer_ticks: f64,
    samples_per_tick: f64,
    num_samples: i64,
    out: &mut Vec<Emit>,
) {
    let t = pattern.total_ticks;
    if t <= 0 || num_samples <= 0 || buffer_ticks <= 0.0 {
        return;
    }
    let tf = t as f64;
    let p1 = p0 + buffer_ticks;

    // First (possibly only) range: [p0, min(p1, tf)), offset base = p0.
    collect(pattern, p0, p1.min(tf), p0, samples_per_tick, num_samples, out);

    // Wrapped range: [0, p1 - tf), offset base = p0 - tf so delta = et + (tf - p0).
    if p1 > tf {
        // ponytail: single-wrap only. A buffer longer than the whole pattern
        // (tiny loop + huge block at slow tempo) would need multiple wraps;
        // clamped here and left as an upgrade path — never happens live.
        let end2 = (p1 - tf).min(tf);
        collect(pattern, 0.0, end2, p0 - tf, samples_per_tick, num_samples, out);
    }
}

fn collect(
    pattern: &Pattern,
    lo: f64,
    hi: f64,
    base: f64,
    samples_per_tick: f64,
    num_samples: i64,
    out: &mut Vec<Emit>,
) {
    if hi <= lo {
        return;
    }
    let first = pattern.events.partition_point(|e| (e.tick as f64) < lo);
    let max_timing = (num_samples - 1).max(0) as f64;
    for e in &pattern.events[first..] {
        let etf = e.tick as f64;
        if etf >= hi {
            break;
        }
        let timing = ((etf - base) * samples_per_tick).round().clamp(0.0, max_timing) as u32;
        out.push(Emit { timing, note: e.note, velocity: e.velocity, is_note_on: e.is_note_on });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::{MidiEvent, Pattern};
    use crate::engine::midi_math::TimeSigEntry;

    /// One-bar 4/4 pattern (1920 ticks) with a note-on at each quarter.
    fn one_bar() -> Pattern {
        let events = vec![
            MidiEvent { tick: 0, note: 36, velocity: 100, is_note_on: true },
            MidiEvent { tick: 480, note: 38, velocity: 100, is_note_on: true },
            MidiEvent { tick: 960, note: 36, velocity: 100, is_note_on: true },
            MidiEvent { tick: 1440, note: 38, velocity: 100, is_note_on: true },
        ];
        Pattern {
            events,
            total_ticks: 1920,
            bar_starts: vec![0, 1920],
            time_signatures: vec![TimeSigEntry { bar_start: 1, bar_end: 1, numerator: 4, denominator: 4 }],
            generation: 0,
            seed: 0,
            tempo: 120.0,
            style_name: String::new(),
            cell_name: String::new(),
        }
    }

    // samples_per_tick at 120 BPM, 48kHz: 60*48000 / (120*480) = 500.
    const SPT: f64 = 500.0;
    const TPS: f64 = 1.0 / SPT;

    #[test]
    fn offset_is_pattern_relative_on_first_loop() {
        let p = one_bar();
        let mut out = Vec::new();
        // Buffer covering ticks [480, 960): the note at 480 must land at offset 0.
        let buffer_ticks = 480.0;
        scan(&p, 480.0, buffer_ticks, SPT, (buffer_ticks * SPT) as i64, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].note, 38);
        assert_eq!(out[0].timing, 0);
    }

    #[test]
    fn offset_does_not_collapse_after_looping() {
        // The verified bug: past the first loop, offsets collapsed to 0.
        // Here p0 is pattern-relative (already wrapped), so the offset must be
        // the same as loop 0 for the same in-pattern position.
        let p = one_bar();
        // Buffer of 240 ticks starting 120 ticks before the note at 480.
        let buffer_ticks = 240.0;
        let num_samples = (buffer_ticks * SPT) as i64;
        let mut out = Vec::new();
        scan(&p, 360.0, buffer_ticks, SPT, num_samples, &mut out);
        assert_eq!(out.len(), 1);
        // note at 480 is 120 ticks into the buffer → 120 * 500 = 60000 samples.
        assert_eq!(out[0].timing, 60000);
        let _ = TPS;
    }

    #[test]
    fn scan_wraps_across_loop_boundary() {
        let p = one_bar();
        // Window [1800, 2040) wraps: covers tick 1800..1920 then 0..120.
        // Events in range: note at 0 (== 1920), landing 120 ticks in.
        let buffer_ticks = 240.0;
        let num_samples = (buffer_ticks * SPT) as i64;
        let mut out = Vec::new();
        scan(&p, 1800.0, buffer_ticks, SPT, num_samples, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].note, 36); // the tick-0 event, seen at the wrap
        assert_eq!(out[0].timing, 120 * 500);
    }

    #[test]
    fn empty_when_no_events_in_window() {
        let p = one_bar();
        let mut out = Vec::new();
        scan(&p, 1.0, 100.0, SPT, (100.0 * SPT) as i64, &mut out);
        assert!(out.is_empty());
    }
}
