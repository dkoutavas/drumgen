use crate::engine::assembler::{self, AssembleResult};
use crate::engine::cell_library::CellLibrary;

fn gcd(a: i32, b: i32) -> i32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

fn lcm(a: i32, b: i32) -> i32 {
    a / gcd(a, b) * b
}

/// FNV-1a 64-bit — tiny deterministic hash for the style-name seed salt.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Manages pattern generation using the cell library.
///
/// Owns the cell library and provides a high-level interface for generating
/// patterns from plugin parameters. Runs inside the `drumgen-gen` worker
/// thread (worker.rs); the audio thread only swaps in finished `Pattern`s.
pub struct GenerationManager {
    library: CellLibrary,
}

impl GenerationManager {
    pub fn new() -> Self {
        Self {
            library: CellLibrary::new(),
        }
    }

    /// Generate a pattern from plugin parameters.
    ///
    /// - `style_index`: index into sorted style names (0-based)
    /// - `humanize`: 0.0-1.0
    /// - `bars`: 1-16
    /// - `seed`: RNG seed
    /// - `swing`: 0.0-1.0
    /// - `generative`: prefer probability grid cells
    #[allow(clippy::too_many_arguments)]
    pub fn generate(
        &self,
        style_index: i32,
        humanize: f64,
        bars: i32,
        seed: u64,
        swing: f64,
        generative: bool,
        tempo: f64,
        meter: (i32, i32),
        fill_every: i32,
    ) -> AssembleResult {
        let style_name = self.library.style_by_index(style_index as usize)
            .unwrap_or("screamo");

        // Salt the seed with the style name so two styles never share an RNG
        // stream (or a rotation index). Without this, styles whose pools share
        // a cell produced byte-identical MIDI at the same seed. Plugin-boundary
        // only — the engine stays a faithful port that takes a raw seed.
        //
        // ADDITION, not XOR: the salted seed also drives `rotate_pick`, and
        // DICE is seed+1. Under XOR, consecutive seeds do not give consecutive
        // rotation indices — preoccupations landed on the same cell for seeds
        // 1&2 and again for 3&4, so four dice presses changed nothing audible.
        // Adding keeps +1 on the seed as +1 on the rotation while still giving
        // every style its own offset into the stream.
        let salted = seed.wrapping_add(fnv1a(style_name.as_bytes()));

        // The looped pattern must CONTAIN the fill cycle. Fills fire at
        // `bar % fill_every == 0`, so a 4-bar loop with FILL=Every 8 never
        // reached bar 8: no fill ever played, and the telegraph counted
        // 7..4 and wrapped with the loop. Extend the loop to the least
        // common multiple: BARS is the figure length, FILL the cycle, the
        // audible loop is both (4 bars @ Every 8 = the figure twice with a
        // fill closing bar 8). Worst case (BARS 15, Every 8) is 120 bars —
        // still millisecond generation. Plugin-boundary only: the CLI keeps
        // bars as an exact file length.
        let bars = if fill_every > 0 { lcm(bars, fill_every) } else { bars };

        // Vary floor is UNCONDITIONAL, matching Song Mode. Two reasons a press
        // can land on the same FIXED cell as the last one: a forced meter
        // narrowing the pool, and — now that DICE is a scrambled jump instead
        // of seed+1 — two successive rolls agreeing mod the pool length
        // (~1/len of presses). `vary` only mutates repeated bars of a fixed
        // cell, so probability/euclidean realization is untouched, and any
        // seed change is guaranteed to move notes, not just feel.
        let vary = 0.25;

        assembler::assemble(
            &self.library,
            Some(style_name),
            None,
            bars,
            tempo, // real transport tempo — ms-based humanization scales with it
            meter, // (0,0) = Auto (style's native meter)
            Some(humanize),
            swing,
            fill_every,
            salted,
            vary,
            generative,
        )
    }

    /// Generate an arranged song pattern (Song Mode). Same style-name seed
    /// salt and vary floor as generate(). `meter` is the HOME meter for
    /// sections without an @N/M override; (0,0) = Auto maps to 4/4 (the
    /// engine divides by the denominator, so a real meter is mandatory).
    #[allow(clippy::too_many_arguments)]
    pub fn generate_arrangement(
        &self,
        style_index: i32,
        arrangement_str: &str,
        humanize: f64,
        seed: u64,
        swing: f64,
        generative: bool,
        tempo: f64,
        meter: (i32, i32),
    ) -> AssembleResult {
        let style_name = self.library.style_by_index(style_index as usize)
            .unwrap_or("screamo");
        let salted = seed.wrapping_add(fnv1a(style_name.as_bytes()));
        let home = if meter == (0, 0) { (4, 4) } else { meter };
        // Vary floor is UNCONDITIONAL in Song Mode. Section intent outranks
        // generativity when picking a cell, so a blast section rightly lands on
        // a fixed blast cell — which would then be identical on every dice
        // press. `vary` only touches repeated bars of a fixed cell (the
        // is_prob/is_euclid guard in assemble_arrangement), so probability
        // sections are unaffected and every section gets per-seed motion.
        let vary = 0.25;

        assembler::assemble_arrangement(
            &self.library,
            style_name,
            arrangement_str,
            tempo,
            home,
            Some(humanize),
            swing,
            salted,
            vary,
            generative,
        )
    }

    /// Number of loaded cells.
    pub fn num_cells(&self) -> usize {
        self.library.num_cells()
    }

    /// Number of available styles.
    pub fn num_styles(&self) -> usize {
        self.library.num_styles()
    }

    /// Get style name by index.
    pub fn style_name(&self, index: usize) -> Option<&str> {
        self.library.style_by_index(index)
    }

    /// Sorted list of style names (index order matches the Style param).
    pub fn style_names(&self) -> Vec<String> {
        self.library.style_names().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn test_all_styles_produce_distinct_patterns() {
        // THE user-facing bug: "changing styles produces the same beat".
        // Pre-fix, 13 of 31 styles emitted bit-identical MIDI to another style
        // (26 identical pairs at seed 0) because generative selection collapsed
        // to a shared single probability cell and the RNG seed carried no style
        // component. Every style must produce a distinct event stream at the
        // plugin defaults (humanize 0.40, bars 4, seed 0, meter Auto).
        //
        // The alias styles (math/blood_brothers/dry_cleaning) are deduped at
        // export time. After the Phase-7 character pass most styles have a
        // probability cell or unique material; black_metal/oxbow/unwound still
        // lean on the seed salt + vary floor for their distinctness here.
        let gen = GenerationManager::new();
        let n = gen.num_styles();
        let streams: Vec<(String, Vec<(i64, crate::engine::cell::Instrument, i32)>)> = (0..n as i32)
            .map(|i| {
                let name = gen.style_name(i as usize).unwrap_or("?").to_string();
                // fill_every=4 matches the shipped FILL default ("Every 4").
                let res = gen.generate(i, 0.40, 4, 0, 0.0, true, 120.0, (0, 0), 4);
                (name, res.events.iter().map(|e| (e.tick, e.instrument, e.velocity)).collect())
            })
            .collect();

        let mut collisions = Vec::new();
        for a in 0..streams.len() {
            for b in (a + 1)..streams.len() {
                if streams[a].1 == streams[b].1 {
                    collisions.push(format!("{} == {}", streams[a].0, streams[b].0));
                }
            }
        }
        assert!(
            collisions.is_empty(),
            "{} identical style pairs at seed 0:\n{}",
            collisions.len(),
            collisions.join("\n")
        );
    }

    #[test]
    fn forced_meter_still_varies_per_seed() {
        // Regression: style_has_prob used to ignore the meter, so forcing e.g.
        // 7/8 on a style whose only prob cell is 4/4 selected a single fixed
        // cell with vary=0.0 — every seed produced byte-identical notes and
        // the dice only re-rolled feel. The vary floor must engage there.
        // fugazi: prob_fugazi_4_4 is its only prob cell; driving_7_8 is fixed.
        let gen = GenerationManager::new();
        let idx = gen.style_names().iter().position(|s| s == "fugazi").expect("fugazi exists") as i32;
        let key = |seed: u64| -> Vec<(i64, crate::engine::cell::Instrument)> {
            // humanize 0.0 isolates note content from feel.
            gen.generate(idx, 0.0, 4, seed, 0.0, true, 120.0, (7, 8), 0)
                .events.iter().map(|e| (e.tick, e.instrument)).collect()
        };
        let base = key(0);
        assert!(
            (1..8).any(|s| key(s) != base),
            "dice must change notes under a forced meter with no matching prob cell"
        );
    }

    #[test]
    fn song_presets_assemble_across_styles() {
        // Every SONGS preset must produce a sane full-length song for
        // representative styles: non-empty, correct total length, silence
        // sections actually silent, and deterministic per seed.
        use crate::params::{song_str, SONGS};
        let gen = GenerationManager::new();
        let styles: Vec<i32> = ["screamo", "euro_screamo", "posthardcore"]
            .iter()
            .map(|n| gen.style_names().iter().position(|s| s == n).expect("style exists") as i32)
            .collect();
        // (preset index, expected bars) — keep in sync with params::SONGS.
        let expected = [
            (1, 20), (2, 16), (3, 16), (4, 20), (5, 16), (6, 32), (7, 24),
            (8, 24), (9, 14),
        ];
        for &(song, bars) in &expected {
            for &style in &styles {
                let run = |seed: u64| {
                    gen.generate_arrangement(style, song_str(song), 0.0, seed, 0.0, true, 120.0, (0, 0))
                };
                let r = run(0);
                assert!(!r.events.is_empty(), "song {} empty for style {}", song, style);
                assert_eq!(r.total_bars, bars, "song {} bar count", song);
                let total_ticks = bars as i64 * 4 * 480;
                assert!(
                    r.events.iter().all(|e| e.tick < total_ticks),
                    "song {} events past the end", song
                );
                let key = |r: &AssembleResult| -> Vec<(i64, crate::engine::cell::Instrument, i32)> {
                    r.events.iter().map(|e| (e.tick, e.instrument, e.velocity)).collect()
                };
                assert_eq!(key(&r), key(&run(0)), "song {} must be deterministic per seed", song);
                assert_ne!(key(&r), key(&run(1)), "song {} dice must change the song", song);
            }
        }
        // Stop/Go silence bars (3, 6, 9) must contain zero events at humanize 0.
        let stopgo = gen.generate_arrangement(styles[0], song_str(3), 0.0, 0, 0.0, true, 120.0, (0, 0));
        let bar_ticks = 4 * 480i64;
        for silent_bar in [3i64, 6, 9] {
            let (lo, hi) = ((silent_bar - 1) * bar_ticks, silent_bar * bar_ticks);
            let count = stopgo.events.iter().filter(|e| e.tick >= lo && e.tick < hi).count();
            assert_eq!(count, 0, "silence bar {} must be empty", silent_bar);
        }
        assert_eq!(SONGS[0].1, "", "index 0 must stay Off");
    }

    #[test]
    fn song_sections_match_engine_output() {
        // The GUI's section map (parse_arrangement) must agree with the bars
        // the engine actually renders for every preset.
        use crate::engine::assembler::parse_arrangement;
        use crate::params::{song_str, SONGS};
        let gen = GenerationManager::new();
        let style = gen.style_names().iter().position(|s| s == "posthardcore").unwrap() as i32;
        for song in 1..SONGS.len() as i32 {
            let secs = parse_arrangement(song_str(song), (4, 4));
            let sum: i32 = secs.iter().map(|x| x.bars).sum();
            let r = gen.generate_arrangement(style, song_str(song), 0.0, 0, 0.0, true, 120.0, (0, 0));
            assert_eq!(sum, r.total_bars, "preset {} section sum vs engine bars", song);
        }
    }

    #[test]
    fn section_dynamics_make_quiet_sections_quiet() {
        // Quiet/Loud: atmospheric bars (1-4, vel_base -14) must average well
        // below the blast bars (15-18, vel_base +7). humanize 0 isolates the
        // section dynamics from feel jitter.
        use crate::params::song_str;
        let gen = GenerationManager::new();
        let idx = gen.style_names().iter().position(|s| s == "euro_screamo").unwrap() as i32;
        let r = gen.generate_arrangement(idx, song_str(4), 0.0, 0, 0.0, true, 120.0, (0, 0));
        let bar_ticks = 4 * 480i64;
        let mean = |lo_bar: i64, hi_bar: i64| -> f64 {
            let vals: Vec<i32> = r
                .events
                .iter()
                .filter(|e| {
                    let bar = e.tick / bar_ticks + 1;
                    bar >= lo_bar && bar <= hi_bar
                })
                .map(|e| e.velocity)
                .collect();
            assert!(!vals.is_empty(), "bars {}-{} must have events", lo_bar, hi_bar);
            vals.iter().sum::<i32>() as f64 / vals.len() as f64
        };
        let quiet = mean(1, 4);
        let loud = mean(15, 18);
        assert!(
            loud > quiet + 8.0,
            "blast ({loud:.1}) must be audibly louder than atmospheric ({quiet:.1})"
        );
    }

    #[test]
    fn generate_is_fast_enough_to_stay_off_the_audio_thread() {
        // The bar-boundary swap story assumes generation is well under a bar.
        // Pin a generous ceiling on the worst case (16 bars, generative, humanize
        // on) across every style so a pathological regression is caught. Real
        // times are sub-millisecond; 50ms is a safety net, not a target.
        let gen = GenerationManager::new();
        let n = gen.num_styles();
        for i in 0..n as i32 {
            let t = Instant::now();
            let res = gen.generate(i, 0.7, 16, 3, 0.0, true, 160.0, (0, 0), 2);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            assert!(!res.events.is_empty() || res.total_bars == 16);
            assert!(ms < 50.0, "style {} took {:.2}ms to generate", i, ms);
        }
    }

    #[test]
    fn dice_is_audible_for_every_style() {
        // The dice is the hero interaction: one press must ALWAYS change what
        // you hear. test_all_styles_produce_distinct_patterns compares styles
        // against each other at a fixed seed — this sweeps the other axis:
        // consecutive DICE presses (the dice_roll path the button actually
        // walks) and seed+1 (the drag-scrub path), within each style.
        // Humanize 0.0 so a changed groove is proved, not changed feel. A
        // roll can land on the same rotation slot as the previous seed
        // (~1/pool-len of presses), which is exactly what the unconditional
        // vary floor exists to cover — this test is what holds it honest.
        let gen = GenerationManager::new();
        let notes = |style: i32, seed: u64| -> Vec<(i64, crate::engine::cell::Instrument)> {
            gen.generate(style, 0.0, 4, seed, 0.0, true, 140.0, (0, 0), 0)
                .events
                .iter()
                .map(|e| (e.tick, e.instrument))
                .collect()
        };

        let mut dead = Vec::new();
        for i in 0..gen.num_styles() as i32 {
            let name = gen.style_name(i as usize).unwrap_or("?").to_string();
            // Eight consecutive dice presses from seed 0.
            let mut s: i32 = 0;
            for press in 0..8 {
                let next = crate::params::dice_roll(s);
                if notes(i, s as u64) == notes(i, next as u64) {
                    dead.push(format!("{name} press {press}: roll {s} == {next}"));
                }
                s = next;
            }
            // Scrub path: adjacent seeds.
            for seed in 0..4u64 {
                if notes(i, seed) == notes(i, seed + 1) {
                    dead.push(format!("{name} scrub {seed} == {}", seed + 1));
                }
            }
        }
        assert!(dead.is_empty(), "dice presses that change nothing:\n{}", dead.join("\n"));
    }

    /// No two cells in the library may hold identical material: the dice
    /// rotates through a style's pool, so a duplicate is a press that changes
    /// nothing. This is what caught motorik_pulse (== postpunk_machine) and
    /// prob_shellac_4_4 (a "probability" cell whose every entry sat at
    /// 0.98–1.0, so it realized to shellac_floor_tom_drive); both were deleted.
    #[test]
    fn no_two_cells_hold_the_same_material() {
        use std::collections::BTreeMap;
        let lib = CellLibrary::new();
        let mut by_material: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for style in lib.style_names().to_vec() {
            for cell in lib.get_pool(&style) {
                // Euclidean cells carry their material in `limbs`, not hits or
                // grid — leaving it out made all four of them collide.
                let key = format!(
                    "{:?}|{:?}|{:?}|{:?}",
                    cell.time_sig, cell.hits, cell.grid, cell.limbs
                );
                let names = by_material.entry(key).or_default();
                if !names.contains(&cell.name) {
                    names.push(cell.name.clone());
                }
            }
        }
        let dupes: Vec<String> = by_material
            .values()
            .filter(|v| v.len() > 1)
            .map(|v| v.join(" == "))
            .collect();
        assert!(dupes.is_empty(), "duplicate cell material:\n{}", dupes.join("\n"));
    }

    #[test]
    fn meter_promise_across_styles_and_meters() {
        // A requested meter (forced, or Auto resolved from the host transport)
        // is a PROMISE: the stamped meter must equal it for every style, even
        // when the pool has no cell in that meter (the fallback cell is
        // adapted). Anything else plays against the host's bar grid — the
        // "Auto shows 4/4 under a 3/4 host" screenshot bug. Only (0,0)
        // (Auto with no host info) may take the cell's native meter. Every
        // pattern must also be non-empty and fit its own grid.
        let gen = GenerationManager::new();
        let meters = [(0, 0), (3, 4), (4, 4), (5, 4), (6, 4), (6, 8), (7, 8)];

        for i in 0..gen.num_styles() as i32 {
            for meter in meters {
                let res = gen.generate(i, 0.0, 4, 1, 0.0, true, 120.0, meter, 0);
                assert_eq!(
                    res.time_signatures.len(),
                    1,
                    "loop mode is a single meter for the whole pattern"
                );
                let got = (res.time_signatures[0].numerator, res.time_signatures[0].denominator);
                if meter != (0, 0) {
                    assert_eq!(
                        got, meter,
                        "style {}: requested meter must be stamped",
                        gen.style_name(i as usize).unwrap_or("?")
                    );
                } else {
                    assert!(
                        got.0 > 0 && (got.1 == 4 || got.1 == 8),
                        "style {}: nonsense native meter {:?}",
                        gen.style_name(i as usize).unwrap_or("?"), got
                    );
                }
                let total = crate::engine::midi_math::total_pattern_ticks(
                    4, &res.time_signatures, crate::engine::midi_math::PPQ,
                );
                assert!(
                    !res.events.is_empty(),
                    "style {} meter {:?}: adapted pattern went silent",
                    gen.style_name(i as usize).unwrap_or("?"), meter
                );
                assert!(
                    res.events.iter().all(|e| e.tick < total),
                    "style {} meter {:?}: event past the {}-tick grid",
                    gen.style_name(i as usize).unwrap_or("?"), meter, total
                );
            }
        }
    }

    #[test]
    fn fill_cycle_longer_than_bars_extends_the_loop() {
        // FILL=Every 8 with BARS=4: fills fire at bar % 8 == 0, which a 4-bar
        // loop never reaches — no fill ever played and the telegraph counted
        // "FILL IN 7..4" and wrapped. The loop must extend to lcm(bars, fill)
        // so the cycle completes (user bug report, 2026-07-31).
        let gen = GenerationManager::new();
        let res = gen.generate(0, 0.0, 4, 1, 0.0, true, 120.0, (4, 4), 8);
        assert_eq!(res.total_bars, 8, "4-bar figure @ Every 8 = an 8-bar loop");
        let bar8_start = crate::engine::midi_math::calculate_bar_start_ticks(
            8, &res.time_signatures, crate::engine::midi_math::PPQ,
        );
        assert!(
            res.events.iter().any(|e| e.tick >= bar8_start),
            "the fill bar must not be silent"
        );
        // Divisor and off cases stay untouched.
        assert_eq!(gen.generate(0, 0.0, 8, 1, 0.0, true, 120.0, (4, 4), 4).total_bars, 8);
        assert_eq!(gen.generate(0, 0.0, 4, 1, 0.0, true, 120.0, (4, 4), 0).total_bars, 4);
    }

    #[test]
    fn song_mode_keeps_the_requested_section_meters() {
        // A song's `@7/8` section is a declaration, not a filter: the bar grid
        // stays 7/8 even when the pool has no 7/8 cell (the fallback cell fills
        // it). Pins the Post-Rock preset, whose one @6/8 section is the only
        // meter change in it.
        let gen = GenerationManager::new();
        let idx = gen.style_names().iter().position(|s| s == "fugazi").expect("fugazi exists") as i32;
        let res = gen.generate_arrangement(
            idx, crate::params::song_str(6), 0.0, 5, 0.0, true, 120.0, (4, 4),
        );
        let meters: Vec<(i32, i32)> = res
            .time_signatures
            .iter()
            .map(|t| (t.numerator, t.denominator))
            .collect();
        assert!(
            meters.contains(&(6, 8)),
            "the @6/8 section must survive into the bar grid, got {meters:?}"
        );
        assert_eq!(res.total_bars, 32, "Post-Rock is 32 bars");
    }
}


