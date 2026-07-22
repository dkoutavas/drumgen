use nih_plug::prelude::*;
use nih_plug_egui::EguiState;
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

    /// Time signature. 0 = Auto (follows the HOST's time signature, falling
    /// back to the style's native meter when the host reports none); otherwise
    /// forces a meter, falling back gracefully if the style has no cell in it.
    #[id = "meter"]
    pub meter: IntParam,

    /// Fill frequency — index into FILLS. A tag-matched fill cell replaces the
    /// groove every N bars (0 = off).
    #[id = "fill"]
    pub fill: IntParam,

    /// Song structure — index into SONGS. 0 = Off (loop mode); otherwise the
    /// pattern is a whole arranged song skeleton (sections, dynamics, stops).
    #[id = "song"]
    pub song: IntParam,

    /// Editor window state (size / open) — persisted with the plugin state.
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,
}

/// Editor window size (logical px) — 2x a 240x160 virtual screen.
pub const EDITOR_WIDTH: u32 = 480;
pub const EDITOR_HEIGHT: u32 = 320;

/// Meter param index → (numerator, denominator). Index 0 is Auto = (0,0).
pub const METERS: [(i32, i32); 7] = [(0, 0), (3, 4), (4, 4), (5, 4), (6, 4), (6, 8), (7, 8)];

pub fn meter_of(index: i32) -> (i32, i32) {
    *METERS.get(index as usize).unwrap_or(&(0, 0))
}

/// Resolve the METER param to an effective meter: Auto (index 0) follows the
/// host time signature; a forced index wins outright. (0,0) still means
/// "style's native meter" downstream.
pub fn effective_meter(index: i32, host_meter: (i32, i32)) -> (i32, i32) {
    if index == 0 { host_meter } else { meter_of(index) }
}

fn meter_label(index: i32) -> String {
    match meter_of(index) {
        (0, 0) => "Auto".to_string(),
        (n, d) => format!("{}/{}", n, d),
    }
}

/// Fill param index → fill-every-N-bars (0 = off), ordered by intensity.
pub const FILLS: [i32; 4] = [0, 8, 4, 2];

pub fn fill_of(index: i32) -> i32 {
    *FILLS.get(index as usize).unwrap_or(&0)
}

fn fill_label(index: i32) -> String {
    match fill_of(index) {
        0 => "Off".to_string(),
        n => format!("Every {}", n),
    }
}

/// Song structure presets: (stepper label, arrangement string). Index 0 = Off
/// (loop mode). Strings use the engine's section vocabulary and were verified
/// against SECTION_PREFERENCES + the style pools (see the Song Mode design).
/// All 4/4 and fill-token-free until the fill-section engine change lands.
pub const SONGS: [(&str, &str); 8] = [
    ("Off", ""),
    // 20 bars — the workhorse Fugazi/ATDI verse-chorus skeleton.
    ("Verse/Chor", "2:intro 4:verse 4:chorus 4:verse 4:chorus 2:outro"),
    // 16 bars — Daitro/City of Caterpillar: 8-bar crescendo (matches the
    // 8-bar build cells) erupting into blast, heavy landing.
    ("Skramz Arc", "2:intro 8:build 4:blast 2:breakdown"),
    // 17 bars — Orchid/pg.99 start-stop stabs; silences are real dead air.
    ("Stop/Go", "2:blast 1:silence 2:blast 1:silence 2:blast 1:silence 4:breakdown 3:chorus"),
    // 20 bars — Saetia quiet-loud-quiet: fragile passage, eruption, a held
    // silence (the gasp), fragile again, full blast, decay.
    ("Quiet/Loud", "4:atmospheric 4:drive 2:silence 4:atmospheric 4:blast 2:outro"),
    // 16 bars — Orchid eruption form: uneasy calm punched apart by silences
    // and blast bursts, a halftime weight in the middle.
    ("Eruption", "2:atmospheric 1:silence 3:blast 1:silence 2:breakdown 3:blast 1:silence 3:blast"),
    // 32 bars — Envy post-rock scale-build: long build, 6/8 lift, blast wall,
    // long comedown. Pair with styles that have 6/8 cells (shellac/fugazi).
    ("Post-Rock", "4:intro 8:build 4:drive@6/8 8:blast 4:atmospheric 4:outro"),
    // 24 bars — black-metal blast-forward with a 7/8 tremolo passage.
    ("Blast Fwd", "2:intro 8:blast 4:drive@7/8 8:blast 2:breakdown"),
];

pub fn song_str(index: i32) -> &'static str {
    SONGS.get(index as usize).map(|(_, s)| *s).unwrap_or("")
}

fn song_label(index: i32) -> String {
    SONGS.get(index as usize).map(|(n, _)| n.to_string()).unwrap_or_else(|| "Off".to_string())
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

            // Default "Every 4": a tasteful fill closing each 4-bar phrase.
            fill: IntParam::new("Fill", 2, IntRange::Linear { min: 0, max: (FILLS.len() - 1) as i32 })
                .with_value_to_string(Arc::new(fill_label)),

            song: IntParam::new("Song", 0, IntRange::Linear { min: 0, max: (SONGS.len() - 1) as i32 })
                .with_value_to_string(Arc::new(song_label)),

            editor_state: EguiState::from_size(EDITOR_WIDTH, EDITOR_HEIGHT),
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
