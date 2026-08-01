//! Seeded song-form composition.
//!
//! `compose_arrangement(seed)` turns a seed into a whole arrangement string —
//! sections, lengths, and meter turns — so SONG=Gen composes a form instead of
//! replaying one of the fixed presets. Pure and deterministic: the same seed
//! always yields the same form, which is what lets DICE re-roll the form and
//! the notes together while SEED still names the take.
//!
//! The string never crosses a thread boundary: the worker calls this when it
//! builds a Gen pattern, and the GUI calls it for the hover display. Nothing
//! is stored, so a Gen pad in the bank is just its seed.
//!
//! Plugin-boundary only, like the SONGS table and the dice policy — the engine
//! stays a faithful port that takes an arrangement string from the caller.

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::params;

/// Emergency form if a composed one somehow fails the validator. Unreachable
/// in practice (a seed sweep pins it), kept because shipping a silent pattern
/// would be worse than shipping a plain one.
const FALLBACK: &str = "2:intro 8:build 1:fill 4:blast 1:breakdown";

/// Song shapes, drawn from what the cell library can actually play. The
/// vocabulary matrix (PROJECT.md §8) says intro/outro/atmospheric are
/// coin-flip-thin across most pools, so they garnish a form rather than carry
/// it; verse/chorus/blast/breakdown/drive are the load-bearing sections.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Archetype {
    VerseChorus,
    BuildBlast,
    QuietLoud,
    Eruption,
    ArcBreakdown,
}

/// Weighted so the two most reliable shapes dominate; Eruption and
/// ArcBreakdown are the spice.
const ARCHETYPES: [(Archetype, u32); 5] = [
    (Archetype::VerseChorus, 3),
    (Archetype::BuildBlast, 2),
    (Archetype::QuietLoud, 2),
    (Archetype::Eruption, 2),
    (Archetype::ArcBreakdown, 1),
];

/// Bars per section before jitter. Straight from the Streamlit quick-add
/// defaults (`app.py`), which are the project's existing taste for how long a
/// section wants to be.
fn base_bars(kind: &str) -> i32 {
    match kind {
        "intro" => 4,
        "build" => 4,
        "verse" => 8,
        "chorus" => 8,
        "drive" => 8,
        "blast" => 2,
        "breakdown" => 4,
        "atmospheric" => 4,
        "outro" => 4,
        "fill" => 1,
        "silence" => 1,
        _ => 4,
    }
}

/// Sections that can take a meter turn. A turn on a fill or a silence is not
/// heard as a turn, and on an intro/outro it just makes the edges unstable.
const TURNABLE: [&str; 4] = ["verse", "drive", "build", "chorus"];

/// The shipped meters minus Auto and 4/4 — a "turn" to 4/4 from 4/4 is not a
/// turn. Mirrors `params::METERS[1..]` without (4,4).
const TURN_METERS: [(i32, i32); 5] = [(3, 4), (5, 4), (6, 4), (6, 8), (7, 8)];

struct Sec {
    bars: i32,
    kind: &'static str,
    meter: Option<(i32, i32)>,
}

/// Pick one of a weighted table.
fn weighted<'a, T>(rng: &mut ChaCha8Rng, table: &'a [(T, u32)]) -> &'a T {
    let total: u32 = table.iter().map(|(_, w)| w).sum();
    let mut roll = rng.gen_range(0..total.max(1));
    for (item, w) in table {
        if roll < *w {
            return item;
        }
        roll -= w;
    }
    &table[0].0
}

/// The section skeleton for an archetype. `garnish` rolls are consumed in a
/// fixed order so the RNG stream stays stable across edits to this function.
fn skeleton(rng: &mut ChaCha8Rng, arch: Archetype) -> Vec<&'static str> {
    // Three independent garnish decisions, always drawn, never conditionally —
    // a conditional draw would shift every later roll and break determinism
    // between two forms that differ only in an early branch.
    let head = rng.gen_bool(0.6);
    let tail = rng.gen_bool(0.5);
    let extra = rng.gen_bool(0.5);

    let mut out: Vec<&'static str> = Vec::new();
    match arch {
        Archetype::VerseChorus => {
            if head {
                out.push("intro");
            }
            out.extend(["verse", "chorus", "verse"]);
            out.push(if extra { "breakdown" } else { "fill" });
            out.push("chorus");
            if tail {
                out.push("outro");
            }
        }
        Archetype::BuildBlast => {
            if head {
                out.push("intro");
            }
            out.extend(["build", "fill", "blast", "breakdown"]);
            if extra {
                out.push("blast");
            }
            if tail {
                out.push("outro");
            }
        }
        Archetype::QuietLoud => {
            out.extend(["atmospheric", "drive", "silence", "atmospheric", "blast"]);
            if extra {
                out.push("drive");
            }
            if tail {
                out.push("outro");
            }
        }
        Archetype::Eruption => {
            out.extend(["atmospheric", "silence", "blast", "breakdown", "blast"]);
            if extra {
                out.extend(["silence", "blast"]);
            }
            if tail {
                out.push("atmospheric");
            }
        }
        Archetype::ArcBreakdown => {
            if head {
                out.push("intro");
            }
            out.extend(["verse", "build", "blast", "breakdown", "verse"]);
            if tail {
                out.push("outro");
            }
        }
    }
    out
}

/// Compose the section list: shape, then lengths, then meter turns.
fn compose_inner(rng: &mut ChaCha8Rng) -> Vec<Sec> {
    let arch = *weighted(rng, &ARCHETYPES);
    let kinds = skeleton(rng, arch);

    // Lengths: jitter each section short / as-written / long. Fills and
    // silences stay at 1 — a two-bar fill is a groove, not a fill.
    let mut secs: Vec<Sec> = kinds
        .into_iter()
        .map(|kind| {
            let base = base_bars(kind);
            let bars = if base <= 1 {
                base
            } else {
                match rng.gen_range(0..3) {
                    0 => (base / 2).max(1),
                    1 => base,
                    _ => base + base / 2,
                }
            };
            Sec { bars: bars.clamp(1, 32), kind, meter: None }
        })
        .collect();

    // The validator caps a form at 64 bars. Halve the longest section until it
    // fits rather than truncating the arc — losing the outro changes the song,
    // a shorter verse does not.
    while secs.iter().map(|s| s.bars as i64).sum::<i64>() > 64 {
        let Some(longest) = secs.iter_mut().max_by_key(|s| s.bars) else { break };
        if longest.bars <= 1 {
            break;
        }
        longest.bars = (longest.bars / 2).max(1);
    }
    // And a degenerate short roll is not a song; grow the meatiest section.
    while secs.iter().map(|s| s.bars as i64).sum::<i64>() < 8 {
        let Some(target) = secs
            .iter_mut()
            .find(|s| s.kind == "verse" || s.kind == "blast" || s.kind == "drive")
        else {
            break;
        };
        if target.bars >= 32 {
            break;
        }
        target.bars = (target.bars * 2).min(32);
    }

    // Meter turns: most forms get one, a few get two, a few stay home.
    let turns = match rng.gen_range(0..10) {
        0..=3 => 0,
        4..=7 => 1,
        _ => 2,
    };
    for _ in 0..turns {
        let eligible: Vec<usize> = secs
            .iter()
            .enumerate()
            .filter(|(_, s)| TURNABLE.contains(&s.kind) && s.meter.is_none())
            .map(|(i, _)| i)
            .collect();
        if eligible.is_empty() {
            break;
        }
        let pick = eligible[rng.gen_range(0..eligible.len())];
        secs[pick].meter = Some(TURN_METERS[rng.gen_range(0..TURN_METERS.len())]);
    }

    secs
}

fn render(secs: &[Sec]) -> String {
    secs.iter()
        .map(|s| match s.meter {
            Some((n, d)) => format!("{}:{}@{}/{}", s.bars, s.kind, n, d),
            None => format!("{}:{}", s.bars, s.kind),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Compose a song form from a seed. Always returns a string that passes
/// `params::valid_arrangement`.
pub fn compose_arrangement(seed: u64) -> String {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let out = render(&compose_inner(&mut rng));
    if params::valid_arrangement(&out) {
        out
    } else {
        FALLBACK.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SWEEP: u64 = 2000;

    #[test]
    fn composing_is_deterministic() {
        // The whole feature rests on this: the seed names the take, so the
        // same seed must compose the same form forever (a stored Gen pad is
        // only its seed — the string is never persisted).
        for seed in [0u64, 1, 42, 4902, 9999] {
            assert_eq!(compose_arrangement(seed), compose_arrangement(seed));
        }
        // And it must actually move with the dice, or DICE is a dead press.
        let differing = (0..200u64)
            .filter(|s| compose_arrangement(*s) != compose_arrangement(s + 1))
            .count();
        assert!(differing > 180, "only {differing}/200 seeds changed the form");
    }

    #[test]
    fn every_seed_composes_a_playable_form() {
        // A form that fails the validator would fall back to FALLBACK, which
        // means the dice quietly stops working. Sweep to prove that path is
        // unreachable, and that the arc is a real song length.
        for seed in 0..SWEEP {
            let arr = compose_arrangement(seed);
            assert!(params::valid_arrangement(&arr), "seed {seed}: {arr}");
            assert_ne!(arr, FALLBACK, "seed {seed} fell back");
            let total: i64 = arr
                .split_whitespace()
                .filter_map(|t| t.split_once(':'))
                .filter_map(|(b, _)| b.parse::<i64>().ok())
                .sum();
            assert!((8..=64).contains(&total), "seed {seed}: {total} bars");
        }
    }

    #[test]
    fn forms_are_varied_and_turn_meters_sanely() {
        // Uniform noise would be a worse song generator than the fixed table
        // it replaces. Check the shapes spread out and the meter turns behave.
        let forms: Vec<String> = (0..SWEEP).map(compose_arrangement).collect();

        // Shape spread: fingerprint by the section sequence (meters stripped).
        let mut shapes = std::collections::BTreeMap::new();
        for f in &forms {
            let shape: Vec<&str> = f
                .split_whitespace()
                .filter_map(|t| t.split_once(':'))
                .map(|(_, rest)| rest.split('@').next().unwrap_or(rest))
                .collect();
            *shapes.entry(shape.join(" ")).or_insert(0usize) += 1;
        }
        assert!(shapes.len() >= 5, "only {} distinct shapes", shapes.len());

        let with_turns = forms.iter().filter(|f| f.contains('@')).count();
        assert!(
            with_turns * 100 / forms.len() >= 25,
            "only {with_turns}/{} forms turn meter",
            forms.len()
        );

        for f in &forms {
            let metered: Vec<&str> = f.split_whitespace().filter(|t| t.contains('@')).collect();
            assert!(metered.len() <= 2, "too many turns: {f}");
            for token in metered {
                let (_, rest) = token.split_once(':').unwrap();
                let (section, ts) = rest.split_once('@').unwrap();
                assert!(TURNABLE.contains(&section), "meter turn on {section}: {f}");
                let (n, d) = ts.split_once('/').unwrap();
                let m = (n.parse::<i32>().unwrap(), d.parse::<i32>().unwrap());
                assert!(TURN_METERS.contains(&m), "unshipped meter {m:?}: {f}");
            }
        }
    }

    #[test]
    fn the_fallback_is_itself_playable() {
        assert!(params::valid_arrangement(FALLBACK));
    }
}
