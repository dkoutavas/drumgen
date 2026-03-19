use serde::Deserialize;
use std::collections::HashMap;

/// Instrument names matching the Python engine.
/// Each maps to a MIDI note number via the kit mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Instrument {
    Kick,
    Snare,
    SnareRim,
    SnareGhost,
    TomHigh,
    TomMidHigh,
    TomMid,
    TomLow,
    TomFloor,
    HihatClosed,
    HihatOpen,
    HihatWideOpen,
    HihatPedal,
    Crash1,
    Crash1Choke,
    Crash2,
    Crash2Choke,
    Ride,
    RideBell,
    RideCrash,
    China,
    China2,
    Splash,
    FxCymbal1,
    FxCymbal2,
}

impl Instrument {
    /// Parse from a snake_case string (matching Python instrument names).
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "kick" => Some(Self::Kick),
            "snare" => Some(Self::Snare),
            "snare_rim" => Some(Self::SnareRim),
            "snare_ghost" => Some(Self::SnareGhost),
            "tom_high" => Some(Self::TomHigh),
            "tom_mid_high" => Some(Self::TomMidHigh),
            "tom_mid" => Some(Self::TomMid),
            "tom_low" => Some(Self::TomLow),
            "tom_floor" => Some(Self::TomFloor),
            "hihat_closed" => Some(Self::HihatClosed),
            "hihat_open" => Some(Self::HihatOpen),
            "hihat_wide_open" => Some(Self::HihatWideOpen),
            "hihat_pedal" => Some(Self::HihatPedal),
            "crash_1" => Some(Self::Crash1),
            "crash_1_choke" => Some(Self::Crash1Choke),
            "crash_2" => Some(Self::Crash2),
            "crash_2_choke" => Some(Self::Crash2Choke),
            "ride" => Some(Self::Ride),
            "ride_bell" => Some(Self::RideBell),
            "ride_crash" => Some(Self::RideCrash),
            "china" => Some(Self::China),
            "china_2" => Some(Self::China2),
            "splash" => Some(Self::Splash),
            "fx_cymbal_1" => Some(Self::FxCymbal1),
            "fx_cymbal_2" => Some(Self::FxCymbal2),
            _ => None,
        }
    }

    /// Convert to snake_case string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Kick => "kick",
            Self::Snare => "snare",
            Self::SnareRim => "snare_rim",
            Self::SnareGhost => "snare_ghost",
            Self::TomHigh => "tom_high",
            Self::TomMidHigh => "tom_mid_high",
            Self::TomMid => "tom_mid",
            Self::TomLow => "tom_low",
            Self::TomFloor => "tom_floor",
            Self::HihatClosed => "hihat_closed",
            Self::HihatOpen => "hihat_open",
            Self::HihatWideOpen => "hihat_wide_open",
            Self::HihatPedal => "hihat_pedal",
            Self::Crash1 => "crash_1",
            Self::Crash1Choke => "crash_1_choke",
            Self::Crash2 => "crash_2",
            Self::Crash2Choke => "crash_2_choke",
            Self::Ride => "ride",
            Self::RideBell => "ride_bell",
            Self::RideCrash => "ride_crash",
            Self::China => "china",
            Self::China2 => "china_2",
            Self::Splash => "splash",
            Self::FxCymbal1 => "fx_cymbal_1",
            Self::FxCymbal2 => "fx_cymbal_2",
        }
    }

    /// Get the MIDI note number for this instrument using the Ugritone mapping.
    pub fn midi_note(&self) -> u8 {
        match self {
            Self::Kick => 36,
            Self::Snare => 38,
            Self::SnareRim => 37,
            Self::SnareGhost => 38,
            Self::TomHigh => 48,
            Self::TomMidHigh => 47,
            Self::TomMid => 45,
            Self::TomLow => 43,
            Self::TomFloor => 41,
            Self::HihatClosed => 42,
            Self::HihatOpen => 46,
            Self::HihatWideOpen => 32,
            Self::HihatPedal => 44,
            Self::Crash1 => 49,
            Self::Crash1Choke => 50,
            Self::Crash2 => 57,
            Self::Crash2Choke => 58,
            Self::Ride => 51,
            Self::RideBell => 53,
            Self::RideCrash => 54,
            Self::China => 52,
            Self::China2 => 61,
            Self::Splash => 55,
            Self::FxCymbal1 => 56,
            Self::FxCymbal2 => 59,
        }
    }

    /// Check if this instrument is in the cymbal group (for physical constraints).
    pub fn is_cymbal(&self) -> bool {
        matches!(
            self,
            Self::HihatClosed
                | Self::HihatOpen
                | Self::HihatWideOpen
                | Self::HihatPedal
                | Self::Ride
                | Self::RideBell
                | Self::RideCrash
                | Self::Crash1
                | Self::Crash2
                | Self::Crash1Choke
                | Self::Crash2Choke
                | Self::China
                | Self::China2
                | Self::Splash
                | Self::FxCymbal1
                | Self::FxCymbal2
        )
    }

    /// Check if this instrument is a stick hit (snare/tom group).
    pub fn is_stick(&self) -> bool {
        matches!(
            self,
            Self::Snare
                | Self::SnareRim
                | Self::SnareGhost
                | Self::TomHigh
                | Self::TomMidHigh
                | Self::TomMid
                | Self::TomLow
                | Self::TomFloor
        )
    }

    /// Cymbal priority for physical constraint resolution (higher wins).
    pub fn cymbal_priority(&self) -> i32 {
        match self {
            Self::Crash1 | Self::Crash2 | Self::Crash1Choke | Self::Crash2Choke
            | Self::China | Self::China2 => 3,
            Self::Splash | Self::RideCrash | Self::FxCymbal1 | Self::FxCymbal2 => 2,
            Self::Ride | Self::RideBell => 1,
            Self::HihatClosed | Self::HihatOpen | Self::HihatWideOpen | Self::HihatPedal => 0,
            _ => -1,
        }
    }

    /// Stick priority for physical constraint resolution (higher wins).
    pub fn stick_priority(&self) -> i32 {
        match self {
            Self::Snare | Self::SnareRim => 2,
            Self::SnareGhost => 1,
            Self::TomHigh | Self::TomMidHigh | Self::TomMid | Self::TomLow | Self::TomFloor => 0,
            _ => -1,
        }
    }

    /// Check if this is a foot instrument (can coexist with hand instruments).
    pub fn is_foot(&self) -> bool {
        matches!(self, Self::Kick | Self::HihatPedal)
    }
}

/// Velocity level names matching the Python engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VelocityLevel {
    Ghost,
    Soft,
    Normal,
    Accent,
}

impl VelocityLevel {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "ghost" => Self::Ghost,
            "soft" => Self::Soft,
            "accent" => Self::Accent,
            _ => Self::Normal,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ghost => "ghost",
            Self::Soft => "soft",
            Self::Normal => "normal",
            Self::Accent => "accent",
        }
    }

    /// Velocity range (low, high) for this level.
    pub fn range(&self) -> (i32, i32) {
        match self {
            Self::Ghost => (20, 50),
            Self::Soft => (50, 75),
            Self::Normal => (75, 105),
            Self::Accent => (105, 127),
        }
    }

    /// Rank for conflict resolution (higher wins).
    pub fn rank(&self) -> i32 {
        match self {
            Self::Ghost => 0,
            Self::Soft => 1,
            Self::Normal => 2,
            Self::Accent => 3,
        }
    }
}

/// A single hit in a cell pattern. Always stored as a 5-tuple equivalent.
#[derive(Debug, Clone)]
pub struct Hit {
    /// Bar number within the cell (1-indexed).
    pub bar: i32,
    /// Beat number within the bar (1-indexed).
    pub beat: i32,
    /// Subdivision offset (0.0, 0.25, 0.5, 0.75).
    pub sub: f64,
    /// Instrument to play.
    pub instrument: Instrument,
    /// Velocity level.
    pub velocity_level: VelocityLevel,
}

/// A probability grid entry for generative cells.
#[derive(Debug, Clone)]
pub struct GridEntry {
    /// Bar number within the cell (1-indexed).
    pub bar: i32,
    /// Beat number within the bar (1-indexed).
    pub beat: i32,
    /// Subdivision offset.
    pub sub: f64,
    /// Instrument.
    pub instrument: Instrument,
    /// Probability of firing (0.0-1.0).
    pub probability: f64,
    /// Velocity level if fired.
    pub velocity_level: VelocityLevel,
}

/// Cell type — fixed pattern or probability grid.
#[derive(Debug, Clone, PartialEq)]
pub enum CellType {
    Fixed,
    Probability,
}

/// A rhythmic cell definition.
#[derive(Debug, Clone)]
pub struct Cell {
    pub name: String,
    pub tags: Vec<String>,
    pub time_sig: (i32, i32),
    pub num_bars: i32,
    pub humanize: f64,
    pub role: String,
    pub cell_type: CellType,
    /// Hits for fixed cells.
    pub hits: Vec<Hit>,
    /// Grid entries for probability cells.
    pub grid: Vec<GridEntry>,
    /// Optional per-bar humanize overrides: (start_bar, end_bar) -> amount.
    pub humanize_per_bar: Option<HashMap<(i32, i32), f64>>,
}

impl Cell {
    /// Check if this cell has a specific tag.
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }

    /// Check if this cell is a probability grid cell.
    pub fn is_probability(&self) -> bool {
        self.cell_type == CellType::Probability
    }
}

/// Layer groups for layer mode.
pub fn layer_instruments(layer: &str) -> Option<&'static [Instrument]> {
    match layer {
        "kick" => Some(&[Instrument::Kick]),
        "snare" => Some(&[Instrument::Snare, Instrument::SnareRim, Instrument::SnareGhost]),
        "cymbal" => Some(&[
            Instrument::HihatClosed, Instrument::HihatOpen, Instrument::HihatWideOpen,
            Instrument::HihatPedal, Instrument::Ride, Instrument::RideBell,
            Instrument::RideCrash, Instrument::Crash1, Instrument::Crash2,
            Instrument::Crash1Choke, Instrument::Crash2Choke, Instrument::China,
            Instrument::China2, Instrument::Splash, Instrument::FxCymbal1, Instrument::FxCymbal2,
        ]),
        "toms" => Some(&[
            Instrument::TomHigh, Instrument::TomMidHigh, Instrument::TomMid,
            Instrument::TomLow, Instrument::TomFloor,
        ]),
        _ => None,
    }
}
