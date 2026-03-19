use crate::engine::assembler::{self, AssembleResult};
use crate::engine::cell_library::CellLibrary;
use crate::engine::midi_math::TimeSigEntry;
use crate::engine::humanizer::Event;

/// Manages pattern generation using the cell library.
///
/// Owns the cell library and provides a high-level interface for generating
/// patterns from plugin parameters. Currently synchronous (generation is fast
/// enough for real-time parameter changes). Phase 3 will add a background
/// thread with double-buffered pattern swap at bar boundaries.
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
    pub fn generate(
        &self,
        style_index: i32,
        humanize: f64,
        bars: i32,
        seed: u64,
        swing: f64,
        generative: bool,
    ) -> AssembleResult {
        let style_name = self.library.style_by_index(style_index as usize)
            .unwrap_or("screamo");

        assembler::assemble(
            &self.library,
            Some(style_name),
            None,
            bars,
            120.0,  // Tempo doesn't affect tick positions — only timing humanization
            (4, 4),
            Some(humanize),
            swing,
            seed,
            0.0,
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
}
