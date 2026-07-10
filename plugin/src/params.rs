use nih_plug::prelude::*;
use std::sync::Arc;

/// Plugin parameters exposed to the DAW for automation.
#[derive(Params)]
pub struct DrumgenParams {
    /// Style index — maps to the sorted style list. Displayed as the genre name.
    #[id = "style"]
    pub style: IntParam,

    /// Master humanize amount (0.0 = robotic, 1.0 = loose).
    /// Controls velocity variance, timing drift, flam, ghost clustering.
    #[id = "humanize"]
    pub humanize: FloatParam,

    /// Pattern length in bars (1-16).
    #[id = "bars"]
    pub bars: IntParam,

    /// RNG seed — the "dice". Each value re-rolls the groove.
    #[id = "seed"]
    pub seed: IntParam,

    /// Swing amount (0.0 = straight, 1.0 = full triplet swing).
    #[id = "swing"]
    pub swing: FloatParam,

    /// Time signature. 0 = Auto (the style's native meter); otherwise forces a
    /// meter, falling back gracefully if the style has no cell in it.
    #[id = "meter"]
    pub meter: IntParam,
}

/// Meter param index → (numerator, denominator). Index 0 is Auto = (0,0).
pub const METERS: [(i32, i32); 7] = [(0, 0), (3, 4), (4, 4), (5, 4), (6, 4), (6, 8), (7, 8)];

pub fn meter_of(index: i32) -> (i32, i32) {
    *METERS.get(index as usize).unwrap_or(&(0, 0))
}

fn meter_label(index: i32) -> String {
    match meter_of(index) {
        (0, 0) => "Auto".to_string(),
        (n, d) => format!("{}/{}", n, d),
    }
}

impl DrumgenParams {
    /// Build the params bound to the given sorted style names, so the Style
    /// param covers every style and shows genre names instead of bare indices.
    pub fn new(style_names: Vec<String>) -> Self {
        let count = style_names.len().max(1);
        let max_style = (count - 1) as i32;
        // Default to the persona's home genre; fall back to index 0.
        let default_style = style_names
            .iter()
            .position(|s| s == "posthardcore")
            .unwrap_or(0) as i32;

        // value_to_string maps the index to the genre name (also seen in the
        // host-generic UI, so even without the custom editor it never shows a bare int).
        let names = Arc::new(style_names);
        let names_fmt = names.clone();
        let style_fmt = Arc::new(move |v: i32| {
            names_fmt
                .get(v as usize)
                .cloned()
                .unwrap_or_else(|| v.to_string())
        });

        Self {
            style: IntParam::new("Style", default_style, IntRange::Linear { min: 0, max: max_style })
                .with_value_to_string(style_fmt),

            humanize: FloatParam::new("Humanize", 0.40, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),

            bars: IntParam::new("Bars", 4, IntRange::Linear { min: 1, max: 16 }),

            seed: IntParam::new("Seed", 0, IntRange::Linear { min: 0, max: 9999 }),

            swing: FloatParam::new("Swing", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),

            meter: IntParam::new("Meter", 0, IntRange::Linear { min: 0, max: (METERS.len() - 1) as i32 })
                .with_value_to_string(Arc::new(meter_label)),
        }
    }
}

impl Default for DrumgenParams {
    /// Fallback with no style names (host never uses this path — the plugin
    /// builds params via `new()` with the real list).
    fn default() -> Self {
        Self::new(Vec::new())
    }
}
