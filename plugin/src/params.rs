use nih_plug::params::persist::PersistentField;
use nih_plug::prelude::*;
use nih_plug_egui::EguiState;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use crate::worker::GenRequest;

/// Pattern bank size. 16 slots = the MIDI notes 36..51 (a pad grid's bottom
/// two rows) and the GUI's one-row-of-cells budget.
pub const BANK_SLOTS: usize = 16;

/// Lowest MIDI note that triggers a slot (C1); slot n = FIRST_TRIGGER_NOTE + n.
pub const FIRST_TRIGGER_NOTE: u8 = 36;

/// One stored bank slot: everything needed to regenerate its pattern.
///
/// Patterns themselves are never persisted — same seed + same params = the
/// same notes, so a snapshot IS the pattern. Tempo is deliberately absent:
/// patterns bake tempo (humanization is ms-based), so a slot re-generates at
/// whatever the transport currently says.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SlotSnapshot {
    /// Style NAME, so a slot survives cells being added to the library (which
    /// re-sorts the style list and shifts every index).
    pub style_name: String,
    /// Style index at store time — the fallback when the name is gone, and
    /// the value the audio thread actually uses (it never touches the String).
    pub style_index: i32,
    pub humanize: f32,
    pub bars: i32,
    pub seed: i32,
    pub swing: f32,
    /// METER param INDEX (0 = Auto), not an effective meter: an Auto slot must
    /// keep following the host after a reload.
    pub meter: i32,
    pub fill: i32,
    /// SONG param INDEX at store time. An index into a list that includes the
    /// user's songs.txt, so it moves when they edit that file.
    pub song: i32,
    /// The song's NAME ("" = Off): what the slot actually means. `sanitize_bank`
    /// resolves it back to an index, so reordering or inserting a line in
    /// songs.txt cannot silently turn a pad into a different form.
    /// `serde(default)`: banks saved before this field existed still load (an
    /// empty name keeps the stored index, the old behaviour).
    #[serde(default)]
    pub song_name: String,
}

impl SlotSnapshot {
    /// The Copy projection the audio thread reads (no String, no allocation).
    pub fn gen_part(&self) -> SlotGen {
        SlotGen {
            style: self.style_index,
            humanize: self.humanize,
            bars: self.bars,
            seed: self.seed,
            swing: self.swing,
            meter: self.meter,
            fill: self.fill,
            song: self.song,
        }
    }
}

/// Copy-only slice of a slot, safe to memcpy off the bank mutex on the audio
/// thread while the lock is held for a few nanoseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotGen {
    pub style: i32,
    pub humanize: f32,
    pub bars: i32,
    pub seed: i32,
    pub swing: f32,
    pub meter: i32,
    pub fill: i32,
    pub song: i32,
}

impl SlotGen {
    /// Build a generation request for this slot. Mirrors
    /// `ParamSnapshot::to_request` in lib.rs.
    ///
    /// An Auto-meter slot bakes the host meter in force when it generated; a
    /// later host meter flip re-dirties Auto slots only (lib.rs `dirty_force`).
    pub fn to_request(&self, tempo: f32, host_meter: (i32, i32), generation: u64) -> GenRequest {
        GenRequest {
            style: self.style,
            humanize: self.humanize as f64,
            bars: self.bars,
            seed: self.seed as u64,
            swing: self.swing as f64,
            generative: true,
            tempo: tempo as f64,
            meter: effective_meter(self.meter, host_meter),
            fill_every: fill_of(self.fill),
            song: self.song,
            generation,
        }
    }
}

/// The persisted bank payload: 16 optional slots.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct BankState {
    pub slots: Vec<Option<SlotSnapshot>>,
}

impl BankState {
    pub fn empty() -> Self {
        Self { slots: vec![None; BANK_SLOTS] }
    }

    /// Slot accessor that tolerates a short/long restored vector.
    pub fn slot(&self, i: usize) -> Option<&SlotSnapshot> {
        self.slots.get(i).and_then(|s| s.as_ref())
    }

    /// Bit i set = slot i holds a snapshot.
    pub fn filled_mask(&self) -> u16 {
        let mut m = 0u16;
        for i in 0..BANK_SLOTS {
            if self.slot(i).is_some() {
                m |= 1 << i;
            }
        }
        m
    }
}

/// Repair a restored bank against the CURRENT style list and song table.
///
/// A project can outlive the library it was saved against: cells get added
/// (re-sorting styles), a style gets deleted, songs.txt gets edited. Rules,
/// in order of trust:
///   1. style_name found in `styles` → use that index (names are the truth).
///   2. name gone but style_index still in range → keep the index, refresh the
///      name so the GUI stops showing a style that no longer exists.
///   3. neither resolves → the slot is dropped (an empty pad beats a wrong one).
/// Everything else is clamped into range: an out-of-range song (songs.txt
/// shrank) falls back to Off rather than silently playing a different form.
pub fn sanitize_bank(state: BankState, styles: &[String]) -> BankState {
    let n_songs = n_songs() as i32;
    let labels: Vec<String> = (0..n_songs).map(song_label).collect();
    let mut slots = vec![None; BANK_SLOTS];

    for (i, dst) in slots.iter_mut().enumerate() {
        let Some(s) = state.slot(i) else { continue };

        let resolved = match styles.iter().position(|n| n == &s.style_name) {
            Some(idx) => Some((idx as i32, s.style_name.clone())),
            None => styles
                .get(s.style_index as usize)
                .map(|n| (s.style_index, n.clone())),
        };
        let Some((style_index, style_name)) = resolved else { continue };
        let song = resolve_song(&s.song_name, s.song, &labels);

        *dst = Some(SlotSnapshot {
            style_name,
            style_index,
            humanize: s.humanize.clamp(0.0, 1.0),
            bars: s.bars.clamp(1, 16),
            seed: s.seed.rem_euclid(10000),
            swing: s.swing.clamp(0.0, 1.0),
            meter: if (0..METERS.len() as i32).contains(&s.meter) { s.meter } else { 0 },
            fill: if (0..FILLS.len() as i32).contains(&s.fill) { s.fill } else { 0 },
            song,
            song_name: song_name_for(song),
        });
    }

    BankState { slots }
}

/// Everything the pattern bank shares between the GUI, the audio thread and
/// plugin state.
///
/// The bank is a PLAYBACK OVERLAY: it never writes params (the audio thread
/// cannot — `ProcessContext` has no `set_parameter`), and any param tweak
/// exits bank mode. Params stay the single editing surface.
///
/// Threading contract:
///   - `state` is locked outright by the GUI thread and `try_lock`ed ONLY by
///     the audio thread, which copies `SlotGen`s out and drops the guard
///     immediately. A failed try_lock just delays a background pre-generation
///     by one buffer.
///   - the atomics are the realtime channel: `trigger` GUI→audio,
///     `active`/`queued` audio→GUI, `epoch` bumped whenever `state` changes.
pub struct BankShared {
    pub state: Mutex<BankState>,
    /// Bumped on every store/clear/state-restore; the audio thread watches it
    /// to know a slot needs regenerating.
    pub epoch: AtomicU32,
    /// GUI → audio slot trigger, as slot+1 (0 = nothing pending). The audio
    /// thread takes it with `swap(0)`.
    pub trigger: AtomicI32,
    /// Audio → GUI: currently playing slot, -1 = bank inactive.
    pub active: AtomicI32,
    /// Audio → GUI: slot queued for the next bar boundary, -1 = none.
    pub queued: AtomicI32,
    /// Audio → GUI: bit i = the audio thread holds a generated pattern for slot
    /// i. A stored pad is not playable until its pattern lands, and the GUI must
    /// show that difference rather than claiming every stored pad is ready.
    pub ready: AtomicU32,
    /// GUI → audio: bit i = slot i holds a snapshot. The audio thread cannot
    /// lock the bank on every buffer to find out, and gating triggers on the
    /// PATTERN instead would silently swallow a press during the generation
    /// window — a dead pad with no explanation.
    pub stored: AtomicU32,
    /// The style list this bank resolves names against. Set once at
    /// construction, never mutated.
    pub styles: Vec<String>,
}

impl BankShared {
    pub fn new(styles: Vec<String>) -> Self {
        Self {
            state: Mutex::new(BankState::empty()),
            epoch: AtomicU32::new(0),
            trigger: AtomicI32::new(0),
            active: AtomicI32::new(-1),
            queued: AtomicI32::new(-1),
            ready: AtomicU32::new(0),
            stored: AtomicU32::new(0),
            styles,
        }
    }

    /// Lock the bank, ignoring poisoning (a panicked GUI frame must not brick
    /// the bank for the rest of the session).
    pub fn lock(&self) -> std::sync::MutexGuard<'_, BankState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Mark the bank changed so the audio thread re-generates, and republish
    /// which pads hold a snapshot. Call after every store, clear or restore.
    pub fn bump(&self) {
        let mask = self.lock().filled_mask() as u32;
        self.stored.store(mask, Ordering::Relaxed);
        self.epoch.fetch_add(1, Ordering::Relaxed);
    }
}

/// Persist the bank with the plugin state. Restoring runs `sanitize_bank` (the
/// library may have moved under the project) and bumps the epoch so the audio
/// thread regenerates every restored slot.
impl PersistentField<'_, BankState> for Arc<BankShared> {
    fn set(&self, new_value: BankState) {
        let sane = sanitize_bank(new_value, &self.styles);
        *self.lock() = sane;
        self.bump();
    }

    fn map<F, R>(&self, f: F) -> R
    where
        F: Fn(&BankState) -> R,
    {
        f(&self.lock())
    }
}

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
    /// forces a meter. The meter is a promise: a style with no cell in it gets
    /// its closest cell adapted to the bar (see `assemble`), never silence.
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

    /// The 16-slot pattern bank. Snapshots only — patterns regenerate from
    /// them, so a saved project restores the bank without storing any MIDI.
    #[persist = "bank-v1"]
    pub bank: Arc<BankShared>,
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

/// Clamp the HOST's reported time signature to what the engine can play.
///
/// Under METER Auto the host's signature was stamped verbatim, so anything the
/// DAW could say reached the engine: a denominator above 1920 gives a zero-tick
/// beat, 1/1 or 3/16 or 31/4 are meters the cells and the grid were never
/// authored for. What survives is the vocabulary the plugin has: /4 and /8 with
/// 1..=15 beats (the same bounds `songs.txt` enforces). A /2 bar is the same
/// length as a /4 bar of twice the beats, so 2/2 plays as 4/4 and 3/2 as 6/4.
/// Anything else is "the host said something we cannot play": (0,0), which the
/// engine reads as the style's native meter, same as a host that says nothing.
pub fn normalize_host_meter(n: i32, d: i32) -> (i32, i32) {
    let (n, d) = if d == 2 { (n.saturating_mul(2), 4) } else { (n, d) };
    if (1..=15).contains(&n) && (d == 4 || d == 8) {
        (n, d)
    } else {
        (0, 0)
    }
}

/// What a STORE click captures: the pad that is PLAYING if one is, otherwise
/// the knobs. In bank mode the bank is an overlay and the knobs are not what
/// the user hears, so storing them would capture a sound that is not playing
/// (and the pad's own header says "PAD n"). A knob tweak exits bank mode, which
/// is how the user says "the knobs are the sound now".
pub fn store_source(
    active: i32,
    slots: &[Option<SlotSnapshot>],
    knobs: impl FnOnce() -> SlotSnapshot,
) -> SlotSnapshot {
    usize::try_from(active)
        .ok()
        .and_then(|i| slots.get(i))
        .and_then(|s| s.clone())
        .unwrap_or_else(knobs)
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
/// Forms use `fill` sections and per-section meters (`@N/M`) where the record
/// they come from does.
pub const SONGS: [(&str, &str); 10] = [
    ("Off", ""),
    // 20 bars — the workhorse Fugazi/ATDI verse-chorus skeleton.
    ("Verse/Chor", "2:intro 4:verse 3:chorus 1:fill 4:verse 4:chorus 2:outro"),
    // 16 bars — Daitro/City of Caterpillar: 8-bar crescendo (matches the
    // 8-bar build cells) erupting into blast, heavy landing.
    ("Skramz Arc", "2:intro 8:build 1:fill 4:blast 1:breakdown"),
    // 16 bars — Orchid/pg.99 start-stop stabs; silences are real dead air.
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
        let section = match rest.split_once('@') {
            Some((sec, ts)) => {
                let Some((n, d)) = ts.split_once('/') else { return false };
                let (Ok(n), Ok(d)) = (n.parse::<i32>(), d.parse::<i32>()) else { return false };
                if !(1..=15).contains(&n) || !(d == 4 || d == 8) {
                    return false;
                }
                sec
            }
            None => rest,
        };
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

/// One DICE roll: a prime stride through the 0..10000 seed space.
/// It is a dice, so it must FEEL like one — the old seed+1 read as a counter.
/// Chosen over real randomness on purpose: no RNG state anywhere in the GUI,
/// the same press-path from the same start forever (determinism is a feature),
/// one automatable/undoable param touch, and the SEED display still names the
/// exact take.
///
/// Why a prime stride and not an LCG scramble: the engine turns the seed into
/// a cell-rotation index (`salted % pool_len`), and an LCG's consecutive
/// outputs can differ by a multiple of the pool length — measured on
/// noise_rock, two presses in eight landed on the same sparse fixed cell and
/// changed nothing audible. 7919 is prime, so consecutive rolls can never
/// agree modulo any real pool size; the rotation is guaranteed to advance,
/// exactly the property seed+1 had. Coprime to 10000 → all 10000 seeds are
/// visited once before the path repeats, and no seed maps to itself.
pub fn dice_roll(seed: i32) -> i32 {
    (seed + 7919) % 10000
}

/// The name a snapshot stores for a SONG index ("" = Off).
pub fn song_name_for(index: i32) -> String {
    if index <= 0 {
        String::new()
    } else {
        song_label(index)
    }
}

/// Resolve a stored song back to an index. `labels[i]` is the label of song
/// index `i` (`labels[0]` = Off).
///   - a stored NAME wins: found → its index now; gone → Off (an Off pad beats
///     a pad that silently plays a different form);
///   - no name (Off, or a bank saved before names existed) → keep the stored
///     index if it is still in range, else Off.
fn resolve_song(name: &str, index: i32, labels: &[String]) -> i32 {
    if !name.is_empty() {
        return labels
            .iter()
            .skip(1)
            .position(|l| l == name)
            .map(|p| p as i32 + 1)
            .unwrap_or(0);
    }
    if (0..labels.len() as i32).contains(&index) {
        index
    } else {
        0
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
        let bank = Arc::new(BankShared::new(style_names.clone()));
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

            bank,
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

    fn styles() -> Vec<String> {
        ["blast", "kidcrash", "posthardcore", "zona"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn snap(name: &str, index: i32) -> SlotSnapshot {
        SlotSnapshot {
            style_name: name.to_string(),
            style_index: index,
            humanize: 0.4,
            bars: 4,
            seed: 42,
            swing: 0.0,
            meter: 0,
            fill: 2,
            song: 0,
            song_name: String::new(),
        }
    }

    fn bank_with(slots: Vec<(usize, SlotSnapshot)>) -> BankState {
        let mut b = BankState::empty();
        for (i, s) in slots {
            b.slots[i] = Some(s);
        }
        b
    }

    #[test]
    fn bank_state_survives_a_serde_round_trip() {
        // The bank is persisted as plugin state; a slot must come back byte-
        // identical or a reopened project plays something else than it saved.
        let bank = bank_with(vec![
            (0, snap("kidcrash", 1)),
            (7, SlotSnapshot { meter: 6, fill: 0, song: 3, ..snap("zona", 3) }),
        ]);
        let json = serde_json::to_string(&bank).expect("serializes");
        let back: BankState = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(bank, back);
        assert_eq!(back.filled_mask(), (1 << 0) | (1 << 7));
    }

    #[test]
    fn sanitize_resolves_a_style_that_moved_in_the_list() {
        // Adding cells re-sorts the style list, so a saved index points at a
        // different genre. The NAME is the truth; the index gets rewritten.
        let saved = bank_with(vec![(0, snap("zona", 0))]); // index 0 was zona once
        let out = sanitize_bank(saved, &styles());
        let s = out.slot(0).expect("slot survives");
        assert_eq!(s.style_index, 3, "index re-resolved from the name");
        assert_eq!(s.style_name, "zona");
    }

    #[test]
    fn sanitize_falls_back_to_the_index_when_the_style_is_gone() {
        // A deleted style (shellac) still has a usable index — keep playing
        // something rather than dropping the slot, but refresh the label so
        // the GUI never shows a genre that no longer exists.
        let saved = bank_with(vec![(2, snap("shellac", 1))]);
        let out = sanitize_bank(saved, &styles());
        let s = out.slot(2).expect("slot survives via index");
        assert_eq!(s.style_index, 1);
        assert_eq!(s.style_name, "kidcrash");
    }

    #[test]
    fn sanitize_drops_a_slot_that_resolves_to_nothing() {
        // Name gone AND index out of range: an empty pad beats a wrong one.
        let saved = bank_with(vec![(3, snap("shellac", 99))]);
        assert!(sanitize_bank(saved, &styles()).slot(3).is_none());
    }

    #[test]
    fn sanitize_clamps_every_out_of_range_field() {
        // songs.txt shrinking must not leave a slot pointing at a form that
        // no longer exists — it falls back to Off, not to a random song.
        let wild = SlotSnapshot {
            humanize: 4.0,
            bars: 99,
            seed: -5,
            swing: -1.0,
            meter: 42,
            fill: 42,
            song: 9999,
            ..snap("blast", 0)
        };
        let out = sanitize_bank(bank_with(vec![(1, wild)]), &styles());
        let s = out.slot(1).expect("clamped, not dropped");
        assert_eq!((s.humanize, s.swing), (1.0, 0.0));
        assert_eq!((s.bars, s.meter, s.fill, s.song), (16, 0, 0, 0));
        assert!((0..10000).contains(&s.seed));
    }

    #[test]
    fn storing_publishes_the_pad_mask_the_trigger_gate_reads() {
        // The audio thread gates a press on `stored`, not on "the pattern
        // arrived" — a press during the generation window must take, or the pad
        // is dead with no explanation. That only works if every store/clear
        // republishes the mask.
        let bank = BankShared::new(styles());
        assert_eq!(bank.stored.load(Ordering::Relaxed), 0);

        bank.lock().slots[0] = Some(snap("kidcrash", 1));
        bank.bump();
        assert_eq!(bank.stored.load(Ordering::Relaxed), 1, "pad 1 is triggerable");

        bank.lock().slots[3] = Some(snap("zona", 3));
        bank.bump();
        assert_eq!(bank.stored.load(Ordering::Relaxed), 0b1001);

        bank.lock().slots[0] = None;
        bank.bump();
        assert_eq!(bank.stored.load(Ordering::Relaxed), 0b1000, "a cleared pad stops firing");
    }

    #[test]
    fn restoring_the_bank_sanitizes_and_bumps_the_epoch() {
        // The epoch bump is what makes the audio thread regenerate restored
        // slots — without it a reopened project has snapshots but no patterns.
        let shared = Arc::new(BankShared::new(styles()));
        let before = shared.epoch.load(Ordering::Relaxed);
        PersistentField::set(&shared, bank_with(vec![(0, snap("zona", 0))]));
        assert_eq!(shared.lock().slot(0).expect("restored").style_index, 3);
        assert_ne!(shared.epoch.load(Ordering::Relaxed), before);
    }

    #[test]
    fn slot_gen_resolves_auto_meter_from_the_host() {
        // An Auto slot must keep following the host after a reload; a forced
        // slot must ignore the host entirely.
        let auto = snap("blast", 0).gen_part();
        assert_eq!(auto.to_request(120.0, (7, 8), 1).meter, (7, 8));
        let forced = SlotSnapshot { meter: 1, ..snap("blast", 0) }.gen_part();
        assert_eq!(forced.to_request(120.0, (7, 8), 1).meter, (3, 4));
        // FILL index 2 = every 4 bars, not the literal index.
        assert_eq!(auto.to_request(120.0, (4, 4), 1).fill_every, 4);
    }

    #[test]
    fn dice_roll_is_a_full_permutation_with_no_fixed_points() {
        // Every seed must be reachable (10000 rolls visit all 10000 seeds
        // exactly once) and no press may leave the seed unchanged — a dice
        // that can roll its own number again is a dead press.
        let mut seen = vec![false; 10000];
        let mut s = 0i32;
        for _ in 0..10000 {
            s = dice_roll(s);
            assert!((0..10000).contains(&s));
            assert!(!seen[s as usize], "cycle shorter than 10000 at seed {s}");
            seen[s as usize] = true;
        }
        assert!(seen.iter().all(|&v| v), "not a full permutation");
        for seed in 0..10000 {
            assert_ne!(dice_roll(seed), seed, "fixed point at {seed}");
        }
    }

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

    fn labels(names: &[&str]) -> Vec<String> {
        std::iter::once("Off").chain(names.iter().copied()).map(String::from).collect()
    }

    /// The reason the name exists: songs.txt is edited by hand, and an index
    /// into it silently re-meant a pad when a line was inserted above it.
    #[test]
    fn a_reordered_songs_file_cannot_change_which_form_a_pad_plays() {
        let before = labels(&["Verse/Chor", "Mine", "Other"]);
        let after = labels(&["Verse/Chor", "Other", "Mine"]); // user swapped two lines
        assert_eq!(resolve_song("Mine", 2, &before), 2);
        assert_eq!(resolve_song("Mine", 2, &after), 3, "must follow the NAME, not the index");
    }

    #[test]
    fn a_deleted_song_falls_back_to_off_not_to_another_form() {
        let after = labels(&["Verse/Chor", "Other"]);
        assert_eq!(resolve_song("Mine", 2, &after), 0);
    }

    #[test]
    fn a_nameless_song_keeps_its_index_if_in_range() {
        // Off (empty name, index 0) and banks saved before names existed.
        let l = labels(&["Verse/Chor", "Mine"]);
        assert_eq!(resolve_song("", 0, &l), 0);
        assert_eq!(resolve_song("", 2, &l), 2);
        assert_eq!(resolve_song("", 9, &l), 0, "out of range falls back to Off");
        assert_eq!(resolve_song("", -1, &l), 0);
    }

    #[test]
    fn a_bank_saved_before_song_names_still_loads() {
        // Adding a required field would have made every existing `bank-v1`
        // payload fail to deserialize, and nih-plug then keeps the default:
        // all pads silently lost.
        let old = r#"{"style_name":"a","style_index":0,"humanize":0.4,"bars":4,
                      "seed":1,"swing":0.0,"meter":0,"fill":0,"song":1}"#;
        let s: SlotSnapshot = serde_json::from_str(old).expect("old payload must load");
        assert_eq!(s.song, 1);
        assert_eq!(s.song_name, "");
    }

    #[test]
    fn song_names_round_trip_and_off_is_empty() {
        assert_eq!(song_name_for(0), "");
        assert_eq!(song_name_for(1), song_label(1));
        let mut s = snap("a", 0);
        s.song = 1;
        s.song_name = song_label(1);
        let back: SlotSnapshot = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn host_meters_the_engine_can_play_pass_through() {
        for (n, d) in [(3, 4), (4, 4), (5, 4), (6, 4), (6, 8), (7, 8), (2, 4), (9, 8), (12, 8), (15, 4)] {
            assert_eq!(normalize_host_meter(n, d), (n, d));
        }
    }

    #[test]
    fn a_half_note_denominator_folds_to_quarters() {
        // Same bar length: 2/2 = 4/4, 3/2 = 6/4.
        assert_eq!(normalize_host_meter(2, 2), (4, 4));
        assert_eq!(normalize_host_meter(3, 2), (6, 4));
    }

    #[test]
    fn host_meters_the_engine_cannot_play_become_native() {
        // 1/1, 3/16, 7/32, 16/4, 0/4 and a denominator that would give a
        // zero-tick beat all mean "no usable host meter".
        for (n, d) in [(1, 1), (3, 16), (7, 32), (16, 4), (0, 4), (4, 0), (4, 4096), (-3, 4), (8, 2)] {
            assert_eq!(normalize_host_meter(n, d), (0, 0), "{n}/{d}");
        }
    }

    fn pad(name: &str) -> SlotSnapshot {
        SlotSnapshot { seed: 77, ..snap(name, 0) }
    }

    #[test]
    fn store_copies_the_playing_pad_not_the_knobs() {
        let mut slots = vec![None; BANK_SLOTS];
        slots[3] = Some(pad("playing"));
        let knobs = || snap("knobs", 1);
        let got = store_source(3, &slots, knobs);
        assert_eq!(got.style_name, "playing");
        assert_eq!(got.seed, 77);
    }

    #[test]
    fn store_takes_the_knobs_when_no_pad_is_playing() {
        let slots = vec![None; BANK_SLOTS];
        assert_eq!(store_source(-1, &slots, || snap("knobs", 1)).style_name, "knobs");
        // A stale active index pointing at an empty or out-of-range slot must
        // not lose the store either.
        assert_eq!(store_source(5, &slots, || snap("knobs", 1)).style_name, "knobs");
        assert_eq!(store_source(99, &slots, || snap("knobs", 1)).style_name, "knobs");
    }
}
