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

/// How far (in ticks) the host's reported buffer start may sit from where the
/// previous window ended and still be treated as the same continuous stretch.
pub const SEAM_TOLERANCE_TICKS: f64 = 0.05;

/// Where the next buffer's window should start.
///
/// Each window is `[p0, p0 + buffer_ticks)`, half-open. `p0` comes from the
/// host's beat clock but `buffer_ticks` is extrapolated from the buffer-start
/// tempo, so consecutive windows meet at two slightly different numbers. When
/// an event sits exactly on a seam (it does whenever a buffer edge lands on a
/// whole tick) rounding in either direction fires it TWICE (overlap) or NEVER
/// (gap). Starting the next window exactly where the last one ended closes it.
///
/// Only when the two agree to within `SEAM_TOLERANCE_TICKS`: beyond that the
/// host really moved (a locate, a loop jump, a tempo step) and its position is
/// the truth. The tolerance also bounds how far we can drift from the host
/// during a tempo ramp, because every buffer re-checks against it.
/// ponytail: a fast tempo ramp that opens a gap wider than the tolerance can
/// still lose an event sitting in the sliver; the upgrade is a two-window scan
/// bridging last end to host start.
pub fn seam_start(host_p0: f64, last_p1: Option<f64>, total: f64) -> f64 {
    let Some(last) = last_p1 else { return host_p0 };
    if total <= 0.0 {
        return host_p0;
    }
    // Circular distance, so a seam at the loop wrap compares correctly.
    let d = (host_p0 - last + total / 2.0).rem_euclid(total) - total / 2.0;
    if d.abs() <= SEAM_TOLERANCE_TICKS {
        last
    } else {
        host_p0
    }
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

    #[test]
    fn negative_p0_places_bar_one_at_its_exact_sample() {
        // The buffer a bank switch lands in starts BEFORE the new pattern's
        // origin, so p0 is negative. The downbeat must be emitted late inside
        // this buffer, not dropped and not snapped to sample 0 — a slot switch
        // that ate its own kick on 1 would be the whole feature failing.
        let p = one_bar();
        let mut out = Vec::new();
        // 480 ticks of lead-in at 1 sample per tick.
        scan(&p, -480.0, 512.0, 1.0, 512, &mut out);
        assert_eq!(out.len(), 1, "only bar 1's downbeat is in this window");
        assert_eq!(out[0].note, 36);
        assert_eq!(out[0].timing, 480);
    }

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
            content_key: 0,
            seed: 0,
            tempo: 120.0,
            style_name: String::new(),
            cell_name: String::new(),
            sections: Vec::new(),
        }
    }

    fn hits_at_480(windows: &[(f64, f64)], p: &Pattern) -> usize {
        let mut out = Vec::new();
        for &(p0, bt) in windows {
            scan(p, p0, bt, 1.0, 100, &mut out);
        }
        // Note 38 also sits at tick 1440; none of these windows reach it.
        out.iter().filter(|e| e.note == 38 && e.is_note_on).count()
    }

    /// The seam bug: window A ends a hair PAST tick 480 and the host's next
    /// start is exactly 480, so the event is in both windows.
    #[test]
    fn a_hit_on_the_seam_fires_once_not_twice() {
        let p = one_bar();
        let p1 = 470.0 + 10.000000000000057; // 480.00000000000006
        assert!(p1 > 480.0);
        // Naive: next window starts at the host's 480.0.
        assert_eq!(hits_at_480(&[(470.0, 10.000000000000057), (480.0, 10.0)], &p), 2);
        // Fixed: next window starts where the last one ended.
        let next = seam_start(480.0, Some(p1), 1920.0);
        assert_eq!(next, p1);
        assert_eq!(hits_at_480(&[(470.0, 10.000000000000057), (next, 10.0)], &p), 1);
    }

    /// The mirror: window A ends a hair SHORT of 480, the host starts at 480.0,
    /// and the event would be in neither... except the host window catches it.
    /// Continuity must not lose it either.
    #[test]
    fn a_hit_on_the_seam_is_not_lost_when_the_window_ends_short() {
        let p = one_bar();
        let p1 = 470.0 + 9.999999999999943; // 479.99999999999994
        assert!(p1 < 480.0);
        let next = seam_start(480.0, Some(p1), 1920.0);
        assert_eq!(hits_at_480(&[(470.0, 9.999999999999943), (next, 10.0)], &p), 1);
    }

    #[test]
    fn a_real_jump_is_not_smoothed_over() {
        // A locate or loop jump: the host's position is the truth.
        assert_eq!(seam_start(100.0, Some(900.0), 1920.0), 100.0);
        assert_eq!(seam_start(100.0, None, 1920.0), 100.0);
    }

    #[test]
    fn the_seam_at_the_loop_wrap_compares_circularly() {
        // Last window ended just short of the wrap, the host says the start of
        // the loop: same moment, 0.02 ticks apart.
        let last = 1919.99;
        assert_eq!(seam_start(0.0, Some(last), 1920.0), last);
        // But 2 ticks apart across the wrap is a genuine jump.
        assert_eq!(seam_start(0.0, Some(1918.0), 1920.0), 0.0);
    }

    // samples_per_tick at 120 BPM, 48kHz: 60*48000 / (120*480) = 500.
    const SPT: f64 = 500.0;

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
