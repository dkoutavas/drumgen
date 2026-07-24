use crate::engine::assembler::{self, AssembleResult};
use crate::engine::cell_library::CellLibrary;

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
        let salted = seed ^ fnv1a(style_name.as_bytes());

        // Vary floor: when no probability cell is reachable (none in the pool,
        // or a forced meter narrows selection to fixed cells only), the dice
        // would only re-roll feel, never notes. A small vary gives repeated
        // bars per-seed motion; styles that re-realize per seed are untouched.
        let vary = if self.library.style_has_prob(style_name, meter) { 0.0 } else { 0.25 };

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
        let salted = seed ^ fnv1a(style_name.as_bytes());
        let home = if meter == (0, 0) { (4, 4) } else { meter };
        // Vary floor mirrors generate(): pool-wide check (sections roam meters).
        let vary = if self.library.style_has_prob(style_name, (0, 0)) { 0.0 } else { 0.25 };

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
        // 7/8 on shellac (whose only prob cell is 4/4) selected a single fixed
        // cell with vary=0.0 — every seed produced byte-identical notes and
        // the dice only re-rolled feel. The vary floor must engage there.
        let gen = GenerationManager::new();
        let idx = gen.style_names().iter().position(|s| s == "shellac").expect("shellac exists") as i32;
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
}
