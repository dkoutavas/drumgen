use nih_plug::prelude::*;

/// Plugin parameters exposed to the DAW for automation.
///
/// Phase 1: parameters are defined but only `bars` affects the hardcoded pattern
/// (loop length). The rest are wired up in Phase 2 when the full engine is ported.
#[derive(Params)]
pub struct DrumgenParams {
    /// Style index (0-27). Maps to STYLE_POOLS entries in Phase 2.
    #[id = "style"]
    pub style: IntParam,

    /// Master humanize amount (0.0 = robotic, 1.0 = loose).
    /// Controls velocity variance, timing drift, flam, ghost clustering.
    #[id = "humanize"]
    pub humanize: FloatParam,

    /// Pattern length in bars (1-16).
    #[id = "bars"]
    pub bars: IntParam,

    /// RNG seed for generative mode (0-9999).
    /// Different seeds produce different probability grid realizations.
    #[id = "seed"]
    pub seed: IntParam,

    /// Swing amount (0.0 = straight, 1.0 = full triplet swing).
    #[id = "swing"]
    pub swing: FloatParam,
}

impl Default for DrumgenParams {
    fn default() -> Self {
        Self {
            style: IntParam::new("Style", 0, IntRange::Linear { min: 0, max: 27 }),

            humanize: FloatParam::new(
                "Humanize",
                0.35,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit("%")
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),

            bars: IntParam::new("Bars", 4, IntRange::Linear { min: 1, max: 16 }),

            seed: IntParam::new("Seed", 0, IntRange::Linear { min: 0, max: 9999 }),

            swing: FloatParam::new(
                "Swing",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit("%")
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
        }
    }
}
