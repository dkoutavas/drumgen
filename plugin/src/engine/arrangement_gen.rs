//! Generated song forms: a shape plus a form seed becomes an arrangement
//! string. Port of `arrangement_gen.py`; the templates themselves are data,
//! exported into `builtin.json` (`forms`), so this file holds only the rules.
//!
//! Every choice comes from SplitMix64, never from the engine RNG, so the same
//! (shape, form seed, style) gives the same form string as the Python CLI. The
//! cross-engine fixture (`fixtures/golden_vector.json`, `forms`) pins it.
//! Draw order is the contract: keep it identical to the Python side.

use serde::Deserialize;

use super::cell_library::CellLibrary;

const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;
const INTENSE: [&str; 4] = ["chorus", "blast", "drive", "breakdown"];
const TURNABLE: [&str; 2] = ["verse", "drive"];
pub const MIN_BARS: u32 = 12;
pub const MAX_BARS: u32 = 40;
pub const FORM_SEED_MAX: u64 = 9999;
const STRUCTURE_STREAM: u64 = 0;
const TURN_STREAM: u64 = 1;

/// One slot of a template: a section drawn from `types` with a length drawn
/// from `bars`, or (with `repeat`) its `body` played a rolled number of times.
#[derive(Deserialize, Clone, Debug)]
pub struct Slot {
    #[serde(default)]
    pub types: Vec<String>,
    #[serde(default)]
    pub bars: Vec<u32>,
    /// Per mille.
    #[serde(default = "always")]
    pub chance: u32,
    #[serde(default)]
    pub repeat: Option<[u32; 2]>,
    #[serde(default)]
    pub body: Vec<Slot>,
}

fn always() -> u32 {
    1000
}

/// A shape (`FORMS` in arrangement_gen.py).
#[derive(Deserialize, Clone, Debug)]
pub struct FormTemplate {
    pub key: String,
    /// Stepper text, at most 10 characters.
    pub label: String,
    /// Short name: the status line and a songs.txt name ("ARC 42").
    pub tag: String,
    pub turns: u32,
    pub turn_chance: u32,
    pub max_silences: u32,
    #[serde(default)]
    pub about: String,
    pub slots: Vec<Slot>,
}

/// A placed section. `meter` is None for the home meter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormSection {
    pub bars: u32,
    pub section: String,
    pub meter: Option<(i32, i32)>,
}

/// SplitMix64 (Vigna). Integer-only, so both engines draw the same stream.
pub struct Dice {
    state: u64,
}

impl Dice {
    pub fn new(seed: u64, stream: u64) -> Self {
        Dice { state: seed ^ stream.wrapping_mul(GOLDEN) }
    }

    pub fn draw(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GOLDEN);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Always draws, whatever the chance, so the stream never depends on it.
    fn chance(&mut self, per_mille: u32) -> bool {
        self.draw() % 1000 < per_mille as u64
    }

    fn pick<'a, T>(&mut self, options: &'a [T]) -> &'a T {
        &options[(self.draw() % options.len() as u64) as usize]
    }

    fn between(&mut self, lo: u32, hi: u32) -> u32 {
        lo + (self.draw() % (hi.saturating_sub(lo) as u64 + 1)) as u32
    }
}

fn place(slots: &[Slot], dice: &mut Dice, out: &mut Vec<(u32, String)>) {
    for slot in slots {
        if !dice.chance(slot.chance) {
            continue;
        }
        if let Some([lo, hi]) = slot.repeat {
            for _ in 0..dice.between(lo, hi) {
                place(&slot.body, dice, out);
            }
            continue;
        }
        if slot.types.is_empty() || slot.bars.is_empty() {
            continue;
        }
        let prev = out.last().map(|(_, s)| s.as_str());
        let fresh: Vec<&String> = slot.types.iter().filter(|t| Some(t.as_str()) != prev).collect();
        let section = if fresh.is_empty() {
            dice.pick(&slot.types).clone()
        } else {
            (*dice.pick(&fresh)).clone()
        };
        let bars = *dice.pick(&slot.bars);
        out.push((bars, section));
    }
}

/// The rules every shape obeys. No dice, so they never shift the draw order.
fn tidy(placed: Vec<(u32, String)>, max_silences: u32) -> Vec<(u32, String)> {
    // Fills only directly before an intense section.
    let mut out: Vec<(u32, String)> = Vec::new();
    for (i, (bars, sec)) in placed.iter().enumerate() {
        let next = placed.get(i + 1).map(|(_, s)| s.as_str());
        if sec == "fill" && !next.is_some_and(|n| INTENSE.contains(&n)) {
            continue;
        }
        out.push((*bars, sec.clone()));
    }
    // Silence: never first or last, never twice in a row, capped.
    let mut kept: Vec<(u32, String)> = Vec::new();
    let mut silences = 0;
    let n = out.len();
    for (i, (bars, sec)) in out.into_iter().enumerate() {
        let mut bars = bars;
        if sec == "silence" {
            let last = i == n - 1;
            let after_silence = kept.last().is_some_and(|(_, s)| s == "silence");
            if kept.is_empty() || last || after_silence || silences >= max_silences {
                continue;
            }
            silences += 1;
            bars = 1;
        }
        kept.push((bars, sec));
    }
    // Two of a kind left side by side become one section.
    let mut merged: Vec<(u32, String)> = Vec::new();
    for (bars, sec) in kept {
        match merged.last_mut() {
            Some((b, s)) if *s == sec && sec != "fill" && sec != "silence" => {
                *b = (*b + bars).min(32);
            }
            _ => merged.push((bars, sec)),
        }
    }
    fit_length(merged)
}

/// Guard for edited templates: halve the longest section while too long,
/// double the longest playing one while too short.
fn fit_length(mut sections: Vec<(u32, String)>) -> Vec<(u32, String)> {
    let total = |s: &[(u32, String)]| s.iter().map(|(b, _)| *b).sum::<u32>();
    // Longest first; on a tie, the earliest (Python: max by (bars, -index)).
    let longest = |s: &[(u32, String)], only_playing: bool| {
        let mut best: Option<usize> = None;
        for (k, (b, sec)) in s.iter().enumerate() {
            if only_playing && (sec == "fill" || sec == "silence") {
                continue;
            }
            if best.is_none_or(|j| *b > s[j].0) {
                best = Some(k);
            }
        }
        best
    };
    while total(&sections) > MAX_BARS {
        let Some(i) = longest(&sections, false) else { break };
        if sections[i].0 == 1 {
            break;
        }
        sections[i].0 /= 2;
    }
    while total(&sections) < MIN_BARS {
        let Some(i) = longest(&sections, true) else { break };
        let b = sections[i].0;
        let grown = 32.min(b * 2).min(b + MAX_BARS - total(&sections));
        if grown == b {
            break;
        }
        sections[i].0 = grown;
    }
    sections
}

/// Meters the style's groove cells are written in, sorted by (den, num). The
/// plugin ships built-in cells only, which is what the Python side counts.
pub fn style_meters(lib: &CellLibrary, style: &str) -> Vec<(i32, i32)> {
    let mut meters: Vec<(i32, i32)> = lib
        .get_pool(style)
        .into_iter()
        .filter(|c| c.role != "fill")
        .map(|c| c.time_sig)
        .collect();
    meters.sort_by_key(|&(n, d)| (d, n));
    meters.dedup();
    meters
}

pub fn generate_sections(
    form: &FormTemplate,
    form_seed: u64,
    meters: &[(i32, i32)],
    home: (i32, i32),
    meter_auto: bool,
) -> Vec<FormSection> {
    let mut placed = Vec::new();
    place(&form.slots, &mut Dice::new(form_seed, STRUCTURE_STREAM), &mut placed);
    let mut sections: Vec<FormSection> = tidy(placed, form.max_silences)
        .into_iter()
        .map(|(bars, section)| FormSection { bars, section, meter: None })
        .collect();

    let candidates: Vec<(i32, i32)> = meters.iter().copied().filter(|&m| m != home).collect();
    if !meter_auto || candidates.is_empty() || form.turns == 0 {
        return sections;
    }
    let mut dice = Dice::new(form_seed, TURN_STREAM);
    let mut turns = 0;
    for s in &mut sections {
        if TURNABLE.contains(&s.section.as_str())
            && turns < form.turns
            && dice.chance(form.turn_chance)
        {
            s.meter = Some(*dice.pick(&candidates));
            turns += 1;
        }
    }
    sections
}

pub fn to_arrangement(sections: &[FormSection]) -> String {
    sections
        .iter()
        .map(|s| match s.meter {
            Some((n, d)) => format!("{}:{}@{}/{}", s.bars, s.section, n, d),
            None => format!("{}:{}", s.bars, s.section),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The arrangement string for a shape (by key) and seed, or None for an
/// unknown shape. With the meter on Auto, verses and drives may turn into
/// the style's other meters.
pub fn generate_form(
    lib: &CellLibrary,
    shape: &str,
    form_seed: u64,
    style: &str,
    home: (i32, i32),
    meter_auto: bool,
) -> Option<String> {
    let form = lib.forms().iter().find(|f| f.key == shape)?;
    let meters = style_meters(lib, style);
    Some(to_arrangement(&generate_sections(form, form_seed, &meters, home, meter_auto)))
}

/// "ARC 42". A songs.txt name keeps 10 characters; this always fits.
pub fn form_name(form: &FormTemplate, form_seed: u64) -> String {
    format!("{} {}", form.tag, form_seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> CellLibrary {
        CellLibrary::new()
    }

    #[test]
    fn splitmix64_is_the_reference_stream() {
        let mut d = Dice::new(0, 0);
        assert_eq!(
            [d.draw(), d.draw(), d.draw()],
            [0xE220_A839_7B1D_CDAF, 0x6E78_9E6A_A1B9_65F4, 0x06C4_5D18_8009_454F]
        );
    }

    #[test]
    fn the_six_shapes_ship_in_stepper_order() {
        let lib = lib();
        let keys: Vec<&str> = lib.forms().iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, ["verse_chorus", "arc", "stop_go", "quiet_loud", "maze", "slow_burn"]);
        for f in lib.forms() {
            assert!(f.label.chars().count() <= 10, "{}", f.label);
            assert!(form_name(f, FORM_SEED_MAX).chars().count() <= 10);
        }
    }

    /// The cross-engine contract: Python wrote these forms, Rust must agree.
    #[test]
    fn forms_match_the_python_engine() {
        let raw = include_str!("../../fixtures/golden_vector.json");
        let fixture: serde_json::Value = serde_json::from_str(raw).unwrap();
        let cases = fixture["forms"].as_array().expect("fixture has no forms");
        assert!(cases.len() > 100);
        let lib = lib();
        for c in cases {
            let home: Vec<i32> = c["home"].as_array().unwrap().iter()
                .map(|v| v.as_i64().unwrap() as i32).collect();
            let got = generate_form(
                &lib,
                c["shape"].as_str().unwrap(),
                c["form_seed"].as_u64().unwrap(),
                c["style"].as_str().unwrap(),
                (home[0], home[1]),
                c["meter_auto"].as_bool().unwrap(),
            );
            assert_eq!(got.as_deref(), c["form"].as_str(), "case {c}");
        }
    }

    #[test]
    fn every_form_is_a_valid_song() {
        let lib = lib();
        for f in lib.forms() {
            for style in ["fugazi", "posthardcore", "zona", "kidcrash"] {
                let meters = style_meters(&lib, style);
                for seed in 0..1000 {
                    let secs = generate_sections(f, seed, &meters, (4, 4), true);
                    let total: u32 = secs.iter().map(|s| s.bars).sum();
                    assert!((MIN_BARS..=MAX_BARS).contains(&total), "{} {seed}", f.key);
                    let arr = to_arrangement(&secs);
                    assert!(crate::params::valid_arrangement(&arr), "{} {seed}: {arr}", f.key);
                }
            }
        }
    }

    #[test]
    fn a_forced_meter_and_a_four_four_style_never_turn() {
        let lib = lib();
        assert_eq!(style_meters(&lib, "kidcrash"), vec![(4, 4)]);
        for seed in 0..500 {
            let kc = generate_form(&lib, "maze", seed, "kidcrash", (4, 4), true).unwrap();
            let forced = generate_form(&lib, "maze", seed, "fugazi", (4, 4), false).unwrap();
            assert!(!kc.contains('@') && !forced.contains('@'), "{seed}");
        }
    }

    #[test]
    fn unknown_shape_is_none() {
        assert_eq!(generate_form(&lib(), "sonata", 1, "fugazi", (4, 4), true), None);
    }
}
