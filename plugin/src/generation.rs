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

    /// Generate an arrangement pattern.
    pub fn generate_arrangement(
        &self,
        style_index: i32,
        arrangement_str: &str,
        humanize: f64,
        seed: u64,
        swing: f64,
        generative: bool,
    ) -> AssembleResult {
        let style_name = self.library.style_by_index(style_index as usize)
            .unwrap_or("screamo");

        assembler::assemble_arrangement(
            &self.library,
            style_name,
            arrangement_str,
            120.0,
            (4, 4),
            Some(humanize),
            swing,
            seed,
            0.0,
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
