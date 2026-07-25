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

    /// Swing amount: 0.0 = straight, 0.50 = full triplet swing (values above
    /// push past triplet toward dotted feel). For zona comping, 0.35-0.50.
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
    /// Key is versioned: old projects saved 480x320 under "editor-state" and
    /// would silently stomp the new default; unknown keys are ignored, so v2
    /// makes every project pick up the cockpit size (a deliberately resized
    /// old window is lost once — accepted).
    #[persist = "editor-state-v2"]
    pub editor_state: Arc<EguiState>,
}

/// Editor window size (logical px). 720x440 fits the horizon strip (4 bars
/// x ~10px cells) plus the full control stack without clipping.
pub const EDITOR_WIDTH: u32 = 720;
pub const EDITOR_HEIGHT: u32 = 440;

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
pub const SONGS: [(&str, &str); 10] = [
    ("Off", ""),
    // 20 bars — the workhorse Fugazi/ATDI verse-chorus skeleton.
    ("Verse/Chor", "2:intro 4:verse 3:chorus 1:fill 4:verse 4:chorus 2:outro"),
    // 16 bars — Daitro/City of Caterpillar: 8-bar crescendo (matches the
    // 8-bar build cells) erupting into blast, heavy landing.
    ("Skramz Arc", "2:intro 8:build 1:fill 4:blast 1:breakdown"),
    // 17 bars — Orchid/pg.99 start-stop stabs; silences are real dead air.
    ("Stop/Go", "2:blast 1:silence 2:blast 1:silence 2:blast 1:silence 4:breakdown 3:chorus"),
    // 20 bars — Saetia quiet-loud-quiet: fragile passage, eruption, a held
    // silence (the gasp), fragile again, full blast, decay.
    ("Quiet/Loud", "4:atmospheric 4:drive 1:silence 4:atmospheric 4:blast 3:outro"),
    // 16 bars — Orchid eruption form: uneasy calm punched apart by silences
    // and blast bursts, a halftime weight in the middle.
    ("Eruption", "2:atmospheric 1:silence 3:blast 2:breakdown 4:blast 1:silence 3:blast"),
    // 32 bars — Envy post-rock scale-build: long build, 6/8 lift, blast wall,
    // long comedown. Pair with styles that have 6/8 cells (shellac/fugazi).
    ("Post-Rock", "4:intro 8:build 4:drive@6/8 1:fill 7:blast 4:atmospheric 4:outro"),
    // 24 bars — black-metal blast-forward with a 7/8 tremolo passage.
    ("Blast Fwd", "2:intro 7:blast 1:fill 4:drive@7/8 8:blast 2:breakdown"),
    // 24 bars — Lord Snow logic: twinkle/burst alternation through a maze of
    // short sections, meters flipping under your feet, stops as punctuation.
    ("Labyrinth", "2:intro 3:verse@7/8 1:fill 2:blast 3:verse@7/8 4:build 1:fill 4:blast 2:breakdown 2:outro"),
    // 14 bars — Ampere: the whole song is the climax; in and out in a minute.
    ("Ampere", "1:intro 3:blast 1:silence 2:blast 2:breakdown 1:fill 3:blast 1:outro"),
];

// ── User song forms: ~/.config/drumgen/songs.txt ────────────────────────────
// One song per line: `Name | 3:verse@7/8 2:blast 1:silence ...`. Loaded ONCE
// at plugin instantiation (restart the DAW to reload). A bad line is rejected
// WHOLE with a log — a typo must never silently become a mediocre song.

use std::sync::OnceLock;

static USER_SONGS: OnceLock<Vec<(String, String)>> = OnceLock::new();

/// Known section vocabulary — must match SECTION_PREFERENCES keys.
const SECTIONS: [&str; 11] = [
    "intro", "build", "verse", "chorus", "drive", "blast", "breakdown",
    "atmospheric", "silence", "fill", "outro",
];

/// Strict arrangement validator. The engine's parse_arrangement is lenient
/// (defaults on bad tokens); user input gets no such mercy.
pub fn valid_arrangement(arr: &str) -> bool {
    let mut total = 0i64;
    let mut any = false;
    for token in arr.split_whitespace() {
        let Some((bars, rest)) = token.split_once(':') else { return false };
        let Ok(bars) = bars.parse::<i64>() else { return false };
        if !(1..=32).contains(&bars) {
            return false;
        }
        let (section, meter) = match rest.split_once('@') {
            Some((sec, ts)) => {
                let Some((n, d)) = ts.split_once('/') else { return false };
                let (Ok(n), Ok(d)) = (n.parse::<i32>(), d.parse::<i32>()) else { return false };
                if !(1..=15).contains(&n) || !(d == 4 || d == 8) {
                    return false;
                }
                (sec, Some((n, d)))
            }
            None => (rest, None),
        };
        let _ = meter;
        if !SECTIONS.contains(&section.to_lowercase().as_str()) {
            return false;
        }
        total += bars;
        any = true;
    }
    any && total <= 64
}

/// Written once when songs.txt doesn't exist yet — teaches the format with
/// zero active lines. Never overwrites an existing file.
const STARTER_SONGS_TXT: &str = "\
# drumgen custom song forms — one per line, restart the DAW to reload.\n\
#\n\
#   Name | bars:section bars:section@meter ...\n\
#\n\
# Sections: intro build verse chorus drive blast breakdown atmospheric\n\
#           silence fill outro\n\
# Meters:   @3/4 @5/4 @6/4 @6/8 @7/8 (omit for the song's home meter, 4/4)\n\
# Rules:    1-32 bars per section, 64 bars total max. A bad line is\n\
#           skipped whole (check the DAW's plugin log).\n\
#\n\
# Example labyrinth (remove the leading # to activate):\n\
# My Maze | 2:atmospheric 3:verse@7/8 1:fill 2:blast 3:verse@7/8 4:build 1:fill 4:blast 2:outro\n\
";

fn load_user_songs() -> Vec<(String, String)> {
    let Some(home) = std::env::var_os("HOME") else { return Vec::new() };
    let dir = std::path::PathBuf::from(home).join(".config/drumgen");
    let path = dir.join("songs.txt");
    if !path.exists() {
        // First run: plant the starter file so the feature is discoverable.
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(&path, STARTER_SONGS_TXT);
    }
    let Ok(text) = std::fs::read_to_string(&path) else { return Vec::new() };
    let mut songs = Vec::new();
    for (ln, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, arr)) = line.split_once('|') else {
            nih_plug::nih_log!("drumgen songs.txt line {}: missing '|' — skipped", ln + 1);
            continue;
        };
        let name: String = name.trim().chars().take(10).collect();
        let arr = arr.trim().to_string();
        if name.is_empty() || !valid_arrangement(&arr) {
            nih_plug::nih_log!(
                "drumgen songs.txt line {}: invalid arrangement — skipped",
                ln + 1
            );
            continue;
        }
        songs.push((name, arr));
    }
    if !songs.is_empty() {
        nih_plug::nih_log!("drumgen: loaded {} user song(s) from songs.txt", songs.len());
    }
    songs
}

/// User songs, loaded once (never on the audio thread — first call happens at
/// plugin instantiation in Default).
pub fn user_songs() -> &'static [(String, String)] {
    USER_SONGS.get_or_init(load_user_songs)
}

/// Built-in presets + user songs.
pub fn n_songs() -> usize {
    SONGS.len() + user_songs().len()
}

pub fn song_str(index: i32) -> &'static str {
    let i = index as usize;
    if i < SONGS.len() {
        SONGS[i].1
    } else {
        user_songs()
            .get(i - SONGS.len())
            .map(|(_, s)| s.as_str())
            .unwrap_or("")
    }
}

/// Display name for a song index — built-in preset or user songs.txt entry.
/// The worker stamps this onto the Pattern for the GUI/telegraph, so it must
/// cover the user table too (indexing SONGS alone left user songs unnamed).
pub fn song_label(index: i32) -> String {
    let i = index as usize;
    if i < SONGS.len() {
        SONGS[i].0.to_string()
    } else {
        user_songs()
            .get(i - SONGS.len())
            .map(|(n, _)| n.clone())
            .unwrap_or_else(|| "Off".to_string())
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

            // Default "Every 4": a tasteful fill closing each 4-bar phrase.
            fill: IntParam::new("Fill", 2, IntRange::Linear { min: 0, max: (FILLS.len() - 1) as i32 })
                .with_value_to_string(Arc::new(fill_label)),

            song: IntParam::new("Song", 0, IntRange::Linear { min: 0, max: (n_songs() - 1) as i32 })
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_song_passes_the_strict_validator() {
        for (name, arr) in SONGS.iter().skip(1) {
            assert!(valid_arrangement(arr), "builtin preset {} failed validation", name);
        }
    }

    #[test]
    fn validator_rejects_the_footguns() {
        assert!(valid_arrangement("3:verse@7/8 2:blast 1:silence"));
        assert!(!valid_arrangement("")); // empty
        assert!(!valid_arrangement("4:vers")); // typo'd section
        assert!(!valid_arrangement("4:verse@7/0")); // div-by-zero meter
        assert!(!valid_arrangement("4:verse@7/3")); // non 4/8 denominator
        assert!(!valid_arrangement("99999:blast")); // hang-scale bars
        assert!(!valid_arrangement("0:blast")); // zero bars
        assert!(!valid_arrangement("blast")); // missing count
        assert!(!valid_arrangement("33:verse 32:blast")); // > 64 total
        assert!(!valid_arrangement("4:verse@x/y")); // garbage meter
    }
}