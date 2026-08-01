//! Jam export: turn a recorded run of pad switches into one playable pattern.
//!
//! The plugin records every switch with the absolute tick that pad's bar 1
//! landed on (`params::JamRing`). This module stitches those anchors and the
//! pads' baked patterns into a single `Pattern` — tiling each pad across its
//! span, re-basing the meter map onto one running bar grid — plus a cue sheet.
//!
//! The cue sheet exists because Bitwig discards MIDI time-signature meta on
//! import and no plugin (nor its controller API) can write the arranger ruler.
//! The `.mid` still carries the meter map for hosts and notation tools that
//! honor it; the `.txt` is what a Bitwig user types in by hand.
//!
//! GUI-side and allocation-heavy on purpose: it runs on a button click, never
//! on the audio thread.

use std::sync::Arc;

use crate::engine::assembler::consolidate_time_signatures;
use crate::engine::midi_math::{self, TimeSigEntry, NOTE_DURATION};
#[cfg(test)]
use crate::engine::midi_math::PPQ;
use crate::params::{BANK_SLOTS, JAM_END_EXIT, JAM_END_STOP};
use crate::pattern::{finalize_events, MidiEvent, Pattern};

/// One pad's stretch of the performance, for the cue sheet.
pub struct JamSegment {
    pub out_start_bar: i32,
    pub bars: i32,
    pub slot: usize,
    pub style_name: String,
    pub seed: u64,
    /// Distinct meters this stretch plays, in order.
    pub meters: Vec<(i32, i32)>,
}

fn is_end_marker(byte: u8) -> bool {
    byte == JAM_END_STOP || byte == JAM_END_EXIT
}

/// The last unbroken run of pad switches, plus the tick it ended on.
///
/// A pass can contain several runs: a param tweak drops out of bank mode
/// (`JAM_END_EXIT`) and a later pad press starts a new one. Exporting the
/// LAST run matches what "save the jam I just played" means.
fn last_run(entries: &[(i64, u8)]) -> (Vec<(i64, u8)>, Option<i64>) {
    // Trailing marker (if the run was closed) and where the pads stop.
    let (body, end) = match entries.last() {
        Some(&(tick, byte)) if is_end_marker(byte) => (&entries[..entries.len() - 1], Some(tick)),
        _ => (entries, None),
    };
    let start = body
        .iter()
        .rposition(|(_, b)| is_end_marker(*b))
        .map(|i| i + 1)
        .unwrap_or(0);
    (body[start..].to_vec(), end)
}

/// Meter of a pattern's bar `bar_idx` (0-based, wrapping).
fn meter_of_bar(p: &Pattern, bar_idx: usize) -> (i32, i32) {
    let bars = p.bar_starts.len().saturating_sub(1).max(1);
    let bar = (bar_idx % bars) as i32 + 1;
    midi_math::get_time_sig_for_bar(bar, &p.time_signatures)
}

/// Stitch the last recorded run into one pattern plus its segment map.
///
/// Returns `None` when nothing was recorded, or when the first pad of the run
/// no longer holds a pattern (it was cleared before the export).
pub fn stitch(
    entries: &[(i64, u8)],
    slots: &[Option<Arc<Pattern>>; BANK_SLOTS],
) -> Option<(Pattern, Vec<JamSegment>)> {
    let (run, end_marker) = last_run(entries);
    let run: Vec<(i64, usize)> = run
        .into_iter()
        .filter(|(_, b)| (*b as usize) < BANK_SLOTS)
        .map(|(t, b)| (t, b as usize))
        .collect();
    if run.is_empty() {
        return None;
    }

    // The output timeline starts at the first anchor: bar 1 of the jam is the
    // first pad's bar 1, whatever the transport position was.
    let mut raw: Vec<MidiEvent> = Vec::new();
    let mut bar_starts: Vec<i64> = vec![0];
    let mut per_bar_meters: Vec<(i32, i32)> = Vec::new();
    let mut segments: Vec<JamSegment> = Vec::new();

    for (i, &(anchor, slot)) in run.iter().enumerate() {
        let Some(pat) = slots[slot].as_ref() else {
            // A pad cleared mid-pass has no notes to stitch. Close the jam
            // here: butting the next segment against the previous one would
            // silently shorten the performance and lie about the bar map.
            break;
        };
        if pat.total_ticks <= 0 {
            break;
        }

        // Where this pad stops sounding: the next switch, or the recorded end.
        // With neither (the host died mid-pass) fall back to one full pass of
        // the pattern — enough to make the file playable, never a fabricated
        // arrangement.
        let raw_end = match run.get(i + 1) {
            Some(&(next, _)) => next,
            None => end_marker.unwrap_or(anchor + pat.total_ticks),
        };
        let span_raw = raw_end - anchor;
        if span_raw <= 0 {
            continue;
        }

        // Bars are the unit: a TimeSigEntry cannot describe a partial bar, and
        // a ruler that ends mid-bar is useless to the reader. Interior anchors
        // already sit on bar boundaries (the swap is bar-quantized), so only a
        // stop mid-bar needs rounding — up, keeping what was played.
        let pat_bars = pat.bar_starts.len().saturating_sub(1).max(1);
        let mut bars_in_span = 0i32;
        let mut span = 0i64;
        while span < span_raw {
            let b = bar_idx_ticks(pat, pat_bars, bars_in_span as usize);
            span += b;
            bars_in_span += 1;
        }

        // Tile the pad across the span, bar by bar, so each output bar carries
        // the meter of the source bar it came from.
        for b in 0..bars_in_span as usize {
            let src_bar = b % pat_bars;
            let bar_ticks = bar_idx_ticks(pat, pat_bars, b);
            let out_bar_start = *bar_starts.last().unwrap();
            let src_start = pat.bar_starts[src_bar];
            let src_end = pat.bar_starts[src_bar + 1];

            // Note-ONS only; offs are reconstructed, which makes the boundary
            // clip exact and guarantees nothing hangs past the barline.
            for e in pat.events.iter().filter(|e| e.is_note_on) {
                if e.tick < src_start || e.tick >= src_end {
                    continue;
                }
                let tick = out_bar_start + (e.tick - src_start);
                raw.push(MidiEvent { tick, note: e.note, velocity: e.velocity, is_note_on: true });
            }

            per_bar_meters.push(meter_of_bar(pat, src_bar));
            bar_starts.push(out_bar_start + bar_ticks);
        }

        let mut meters: Vec<(i32, i32)> = Vec::new();
        for b in 0..bars_in_span as usize {
            let m = meter_of_bar(pat, b % pat_bars);
            if meters.last() != Some(&m) {
                meters.push(m);
            }
        }
        segments.push(JamSegment {
            out_start_bar: (bar_starts.len() as i32 - 1) - bars_in_span + 1,
            bars: bars_in_span,
            slot,
            style_name: pat.style_name.clone(),
            seed: pat.seed,
            meters,
        });
    }

    if segments.is_empty() {
        return None;
    }
    let total_ticks = *bar_starts.last().unwrap();
    if total_ticks <= 0 {
        return None;
    }

    // Every note-on gets an off inside the pattern, clamped like from_assemble.
    let ons: Vec<MidiEvent> = raw.clone();
    for e in ons {
        let off = (e.tick + NOTE_DURATION).min(total_ticks - 1);
        raw.push(MidiEvent { tick: off, note: e.note, velocity: 0, is_note_on: false });
    }

    // One TimeSigEntry per output bar, then consolidated — which also merges
    // equal meters ACROSS a segment boundary (two 4/4 pads read as one run).
    let ts: Vec<TimeSigEntry> = per_bar_meters
        .iter()
        .enumerate()
        .map(|(i, &(n, d))| TimeSigEntry {
            bar_start: i as i32 + 1,
            bar_end: i as i32 + 1,
            numerator: n,
            denominator: d,
        })
        .collect();

    let first = slots[run[0].1].as_ref().expect("checked above");
    let pattern = Pattern {
        events: finalize_events(raw),
        total_ticks,
        bar_starts,
        time_signatures: consolidate_time_signatures(&ts),
        generation: 0,
        seed: first.seed,
        tempo: first.tempo,
        style_name: "jam".to_string(),
        cell_name: format!("jam {} segments", segments.len()),
        sections: Vec::new(),
    };
    Some((pattern, segments))
}

/// Ticks in bar `idx` of a tiled pattern (wrapping at its own length).
fn bar_idx_ticks(p: &Pattern, pat_bars: usize, idx: usize) -> i64 {
    let b = idx % pat_bars;
    p.bar_starts[b + 1] - p.bar_starts[b]
}

/// The hand-entry sheet: what to type into Bitwig's ruler, and what played.
pub fn cue_sheet(segments: &[JamSegment], pattern: &Pattern, truncated: bool) -> String {
    let bars = pattern.bar_starts.len().saturating_sub(1);
    let mut out = String::new();
    out.push_str(&format!(
        "drumgen jam - {} segments, {} bars, {:.0} BPM\n",
        segments.len(),
        bars,
        pattern.tempo
    ));
    out.push_str(
        "Bitwig discards time signatures on MIDI import. Enter the ruler lines\n\
         at the bottom of this file into the arranger by hand.\n",
    );
    if truncated {
        out.push_str("NOTE: the pass had more switches than the recorder holds; this is the last 64.\n");
    }

    out.push_str("\nSEGMENTS\n");
    for s in segments {
        let meters: Vec<String> = s.meters.iter().map(|(n, d)| format!("{n}/{d}")).collect();
        out.push_str(&format!(
            "bar {:>3}: pad {:>2} - {} - seed {:04} - {} bars - {}\n",
            s.out_start_bar,
            s.slot + 1,
            s.style_name,
            s.seed,
            s.bars,
            meters.join(" ")
        ));
    }

    out.push_str("\nTIME SIGNATURES (ruler)\n");
    for ts in &pattern.time_signatures {
        out.push_str(&format!(
            "bar {:>3}: {}/{}\n",
            ts.bar_start, ts.numerator, ts.denominator
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pad pattern: `bars` bars of `num/den`, one kick per bar start.
    fn pad(bars: i32, num: i32, den: i32, note: u8, seed: u64, style: &str) -> Arc<Pattern> {
        let bar_ticks = num as i64 * (PPQ * 4 / den as i64);
        let mut events = Vec::new();
        let mut bar_starts = vec![0i64];
        for b in 0..bars as i64 {
            events.push(MidiEvent {
                tick: b * bar_ticks,
                note,
                velocity: 100,
                is_note_on: true,
            });
            bar_starts.push((b + 1) * bar_ticks);
        }
        Arc::new(Pattern {
            events,
            total_ticks: bars as i64 * bar_ticks,
            bar_starts,
            time_signatures: vec![TimeSigEntry {
                bar_start: 1,
                bar_end: bars,
                numerator: num,
                denominator: den,
            }],
            generation: 0,
            seed,
            tempo: 168.0,
            style_name: style.to_string(),
            cell_name: String::new(),
            sections: Vec::new(),
        })
    }

    fn bank(entries: Vec<(usize, Arc<Pattern>)>) -> [Option<Arc<Pattern>>; BANK_SLOTS] {
        let mut b: [Option<Arc<Pattern>>; BANK_SLOTS] = Default::default();
        for (i, p) in entries {
            b[i] = Some(p);
        }
        b
    }

    const BAR_4_4: i64 = 4 * PPQ;

    #[test]
    fn one_pad_tiles_across_its_span_and_ends_on_a_barline() {
        // A single pad held for two and a half bars: the jam keeps what was
        // played and rounds UP to the barline, because a TimeSigEntry cannot
        // describe a partial bar and a ruler must land on one.
        let slots = bank(vec![(0, pad(1, 4, 4, 36, 7, "kidcrash"))]);
        let entries = vec![(1000, 0u8), (1000 + BAR_4_4 * 5 / 2, JAM_END_STOP)];
        let (p, segs) = stitch(&entries, &slots).expect("stitches");

        assert_eq!(p.bar_starts, vec![0, BAR_4_4, BAR_4_4 * 2, BAR_4_4 * 3]);
        assert_eq!(p.total_ticks, BAR_4_4 * 3);
        assert_eq!(segs.len(), 1);
        assert_eq!((segs[0].bars, segs[0].out_start_bar), (3, 1));
        // The pad tiled: one kick per bar, none past the end.
        let ons: Vec<i64> = p.events.iter().filter(|e| e.is_note_on).map(|e| e.tick).collect();
        assert_eq!(ons, vec![0, BAR_4_4, BAR_4_4 * 2]);
        assert!(p.events.iter().all(|e| e.tick < p.total_ticks), "nothing escapes the loop");
    }

    #[test]
    fn a_meter_change_rebases_onto_one_bar_grid() {
        // THE point of the exporter: pad A in 4/4 then pad B in 7/8 must come
        // out as one timeline whose bar map describes both, so the cue sheet
        // can tell the user exactly where to put the ruler change.
        let a = pad(1, 4, 4, 36, 1, "euro_screamo");
        let b = pad(1, 7, 8, 38, 2, "faraquet");
        let slots = bank(vec![(0, a), (7, b)]);
        let bar_7_8 = 7 * (PPQ * 4 / 8);
        let entries = vec![
            (0, 0u8),
            (BAR_4_4 * 2, 7u8),
            (BAR_4_4 * 2 + bar_7_8 * 2, JAM_END_STOP),
        ];
        let (p, segs) = stitch(&entries, &slots).expect("stitches");

        assert_eq!(
            p.time_signatures
                .iter()
                .map(|t| (t.bar_start, t.bar_end, t.numerator, t.denominator))
                .collect::<Vec<_>>(),
            vec![(1, 2, 4, 4), (3, 4, 7, 8)],
            "meter map re-based onto the stitched bar numbers"
        );
        assert_eq!(p.bar_starts.last().copied().unwrap(), p.total_ticks);
        assert_eq!(p.total_ticks, BAR_4_4 * 2 + bar_7_8 * 2);
        assert_eq!(segs.len(), 2);
        assert_eq!((segs[1].out_start_bar, segs[1].bars), (3, 2));
        assert_eq!(segs[1].meters, vec![(7, 8)]);
        // Pad B's notes start exactly at the switch, in phase from ITS bar 1.
        let ons: Vec<i64> =
            p.events.iter().filter(|e| e.is_note_on && e.note == 38).map(|e| e.tick).collect();
        assert_eq!(ons, vec![BAR_4_4 * 2, BAR_4_4 * 2 + bar_7_8]);
    }

    #[test]
    fn equal_meters_merge_across_a_pad_change() {
        // Two 4/4 pads are one meter run; the ruler must not get a pointless
        // 4/4 -> 4/4 change at the switch.
        let slots = bank(vec![
            (0, pad(1, 4, 4, 36, 1, "a")),
            (1, pad(1, 4, 4, 38, 2, "b")),
        ]);
        let entries = vec![(0, 0u8), (BAR_4_4, 1u8), (BAR_4_4 * 2, JAM_END_STOP)];
        let (p, segs) = stitch(&entries, &slots).expect("stitches");
        assert_eq!(p.time_signatures.len(), 1, "one 4/4 run, not two");
        assert_eq!(segs.len(), 2, "still two segments in the cue sheet");
    }

    #[test]
    fn the_export_takes_the_last_run_only() {
        // A param tweak drops out of bank mode mid-pass; pressing a pad again
        // starts a new run. "Save the jam I just played" means the last one.
        let slots = bank(vec![
            (0, pad(1, 4, 4, 36, 1, "a")),
            (1, pad(1, 4, 4, 38, 2, "b")),
        ]);
        let entries = vec![
            (0, 0u8),
            (BAR_4_4, JAM_END_EXIT),
            (BAR_4_4 * 4, 1u8),
            (BAR_4_4 * 6, JAM_END_STOP),
        ];
        let (p, segs) = stitch(&entries, &slots).expect("stitches");
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].slot, 1, "only the run after the exit");
        assert_eq!(p.total_ticks, BAR_4_4 * 2);
    }

    #[test]
    fn an_unclosed_run_falls_back_to_one_pass_of_the_pattern() {
        // The host died mid-pass: no end marker. Emit something playable
        // rather than nothing, but never fabricate a long arrangement.
        let slots = bank(vec![(3, pad(2, 4, 4, 36, 5, "a"))]);
        let (p, segs) = stitch(&[(500, 3u8)], &slots).expect("stitches");
        assert_eq!(segs[0].bars, 2, "exactly one pass of a 2-bar pad");
        assert_eq!(p.total_ticks, BAR_4_4 * 2);
    }

    #[test]
    fn nothing_recorded_exports_nothing() {
        let slots = bank(vec![(0, pad(1, 4, 4, 36, 1, "a"))]);
        assert!(stitch(&[], &slots).is_none());
        assert!(stitch(&[(0, JAM_END_STOP)], &slots).is_none());
        // A pad cleared before the export cannot be stitched.
        let empty: [Option<Arc<Pattern>>; BANK_SLOTS] = Default::default();
        assert!(stitch(&[(0, 0u8), (BAR_4_4, JAM_END_STOP)], &empty).is_none());
    }

    #[test]
    fn the_cue_sheet_names_every_switch_and_ruler_change() {
        let a = pad(1, 4, 4, 36, 1, "euro_screamo");
        let b = pad(1, 7, 8, 38, 2521, "faraquet");
        let slots = bank(vec![(0, a), (7, b)]);
        let bar_7_8 = 7 * (PPQ * 4 / 8);
        let entries = vec![(0, 0u8), (BAR_4_4 * 2, 7u8), (BAR_4_4 * 2 + bar_7_8, JAM_END_STOP)];
        let (p, segs) = stitch(&entries, &slots).unwrap();
        let sheet = cue_sheet(&segs, &p, false);

        assert!(sheet.contains("3 bars"), "header counts bars:\n{sheet}");
        assert!(sheet.contains("168 BPM"));
        assert!(sheet.contains("pad  1"), "1-based pad numbers:\n{sheet}");
        assert!(sheet.contains("pad  8"));
        assert!(sheet.contains("euro_screamo") && sheet.contains("faraquet"));
        assert!(sheet.contains("seed 2521"));
        // The ruler block must carry the change, at the stitched bar number.
        let ruler = sheet.split("TIME SIGNATURES").nth(1).expect("ruler block");
        assert!(ruler.contains("bar   1: 4/4"), "{ruler}");
        assert!(ruler.contains("bar   3: 7/8"), "{ruler}");
        assert!(cue_sheet(&segs, &p, true).contains("last 64"));
    }
}
