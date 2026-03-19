use rand::Rng;
use rand::distributions::{Distribution, Standard};
use rand_chacha::ChaCha8Rng;
use rand::SeedableRng;

use super::cell::{Instrument, VelocityLevel};

/// Section drift profiles — how timing shifts across bars within a section.
fn drift_profile(section_type: &str) -> Option<&'static str> {
    match section_type {
        "verse" | "atmospheric" | "intro" | "quiet" | "outro" => Some("gradual_drag"),
        "chorus" | "blast" | "drive" => Some("constant_push"),
        "build" | "buildup" | "crescendo" => Some("gradual_push"),
        "breakdown" | "half_time" => Some("constant_drag"),
        "fill" => Some("fill_rush"),
        _ => None,
    }
}

/// Tag → ghost clustering amount (0.0 = none, 1.0 = max).
fn cluster_tag_amount(tag: &str) -> Option<f64> {
    match tag {
        "faraquet" => Some(0.7),
        "angular" => Some(0.6),
        "math" => Some(0.6),
        "raein" | "euro_screamo" | "daitro" => Some(0.5),
        "fugazi" | "posthardcore" | "driving" => Some(0.4),
        "screamo" | "emoviolence" => Some(0.3),
        "shellac" | "noise_rock" | "blast" | "post_punk" | "motorik" => Some(0.0),
        _ => None,
    }
}

/// Get ghost clustering amount for a cell based on its tags.
pub fn get_cluster_amount(tags: &[String]) -> f64 {
    let amounts: Vec<f64> = tags.iter()
        .filter_map(|t| cluster_tag_amount(t))
        .collect();
    if amounts.is_empty() {
        0.3
    } else {
        amounts.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
    }
}

/// Infer section type from cell tags for push/pull drift.
pub fn infer_section_type(tags: &[String]) -> &'static str {
    let has = |t: &str| tags.iter().any(|tag| tag == t);
    if has("blast") || has("extreme") { return "blast"; }
    if has("build") || has("crescendo") { return "build"; }
    if has("breakdown") || has("half_time") { return "breakdown"; }
    if has("atmospheric") || has("sparse") || has("quiet") { return "atmospheric"; }
    if has("driving") || has("intense") { return "drive"; }
    if has("fill") { return "fill"; }
    "verse"
}

/// An assembled MIDI event: (absolute_tick, instrument, velocity).
#[derive(Debug, Clone)]
pub struct Event {
    pub tick: i64,
    pub instrument: Instrument,
    pub velocity: i32,
}

/// Physics-based drum humanizer with seeded RNG.
pub struct Humanizer {
    pub rng: ChaCha8Rng,
    pub humanize_amount: f64,
}

impl Humanizer {
    pub fn new(humanize_amount: f64, seed: u64) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(seed),
            humanize_amount,
        }
    }

    /// Per-instrument velocity variance width.
    fn instrument_variance(instrument: Instrument) -> i32 {
        match instrument {
            Instrument::HihatClosed => 25,
            Instrument::HihatOpen => 20,
            Instrument::Ride => 22,
            Instrument::RideBell => 20,
            Instrument::Snare => 18,
            Instrument::SnareGhost => 20,
            Instrument::SnareRim => 15,
            Instrument::Kick => 16,
            Instrument::Crash1 | Instrument::Crash2 => 14,
            Instrument::China => 16,
            Instrument::Splash => 14,
            Instrument::TomHigh | Instrument::TomMid | Instrument::TomLow | Instrument::TomFloor => 18,
            Instrument::TomMidHigh => 18,
            Instrument::HihatPedal => 15,
            Instrument::HihatWideOpen => 20,
            Instrument::Crash1Choke | Instrument::Crash2Choke => 8,
            Instrument::RideCrash => 16,
            Instrument::FxCymbal1 | Instrument::FxCymbal2 => 14,
            Instrument::China2 => 16,
        }
    }

    /// Per-instrument timing tendency in milliseconds.
    fn timing_tendency(instrument: Instrument) -> f64 {
        match instrument {
            Instrument::Snare => 2.0,
            Instrument::SnareGhost => 4.0,
            Instrument::SnareRim => 2.0,
            Instrument::Kick => 0.0,
            Instrument::HihatClosed => -1.0,
            Instrument::Ride => -2.0,
            Instrument::RideBell => -2.0,
            Instrument::Crash1 | Instrument::Crash2 => -3.0,
            Instrument::China => -2.0,
            Instrument::TomFloor | Instrument::TomLow => 1.0,
            Instrument::TomMid | Instrument::TomHigh | Instrument::TomMidHigh => 0.0,
            Instrument::HihatPedal => 0.0,
            Instrument::HihatOpen => -1.0,
            Instrument::Splash => -2.0,
            Instrument::HihatWideOpen => -1.0,
            Instrument::Crash1Choke | Instrument::Crash2Choke => 0.0,
            Instrument::RideCrash => -3.0,
            Instrument::FxCymbal1 | Instrument::FxCymbal2 => -2.0,
            Instrument::China2 => -2.0,
        }
    }

    /// Contour offset for wrist pattern on cymbals.
    fn contour_offset(sub: f64) -> f64 {
        // Find closest sub value
        let subs = [(0.0, 6.0), (0.25, -4.0), (0.5, 3.0), (0.75, -5.0)];
        let mut closest = subs[0];
        let mut min_dist = f64::MAX;
        for &(s, offset) in &subs {
            let dist = (s - sub).abs();
            if dist < min_dist {
                min_dist = dist;
                closest = (s, offset);
            }
        }
        closest.1
    }

    /// Whether an instrument gets velocity contour (wrist pattern).
    fn is_contour_instrument(instrument: Instrument) -> bool {
        matches!(
            instrument,
            Instrument::Ride | Instrument::RideBell
                | Instrument::HihatClosed | Instrument::HihatOpen
                | Instrument::HihatWideOpen
        )
    }

    /// Generate a random integer in [low, high] inclusive.
    fn randint(&mut self, low: i32, high: i32) -> i32 {
        if low >= high { return low; }
        self.rng.gen_range(low..=high)
    }

    /// Generate a gaussian random value with given mean and stddev.
    fn gauss(&mut self, mean: f64, stddev: f64) -> f64 {
        use rand_distr::Normal;
        let normal = Normal::new(mean, stddev).unwrap_or(Normal::new(0.0, 1.0).unwrap());
        normal.sample(&mut self.rng)
    }

    /// Generate a uniform random f64 in [low, high].
    fn uniform(&mut self, low: f64, high: f64) -> f64 {
        self.rng.gen_range(low..=high)
    }

    /// Generate a random f64 in [0.0, 1.0).
    fn random(&mut self) -> f64 {
        self.rng.gen::<f64>()
    }

    /// Humanize velocity for a given level and instrument.
    /// Returns a MIDI velocity value (1-127).
    pub fn humanize_velocity(&mut self, velocity_level: VelocityLevel, instrument: Instrument) -> i32 {
        let (low, high) = velocity_level.range();
        let center = (low + high) / 2;
        let variance = Self::instrument_variance(instrument);
        let scaled_variance = (variance as f64 * self.humanize_amount).round() as i32;
        let scaled_variance = scaled_variance.max(3);
        if scaled_variance <= 0 {
            return center.clamp(1, 127);
        }
        let vel = self.randint(center - scaled_variance, center + scaled_variance);
        vel.clamp(1, 127)
    }

    /// Humanize timing for a hit at the given absolute tick position.
    /// Returns adjusted absolute tick (clamped to >= 0).
    pub fn humanize_timing(&mut self, abs_tick: i64, instrument: Instrument, tempo_bpm: f64, ppq: i64) -> i64 {
        let ms_per_tick = (60000.0 / tempo_bpm) / ppq as f64;
        let tendency_ms = Self::timing_tendency(instrument);
        let tendency_ticks = (tendency_ms * self.humanize_amount / ms_per_tick) as i64;
        let jitter_ms = self.gauss(0.0, 6.0 * self.humanize_amount);
        let jitter_ticks = (jitter_ms / ms_per_tick) as i64;
        (abs_tick + tendency_ticks + jitter_ticks).max(0)
    }

    /// Apply swing to upbeat hits.
    pub fn apply_swing(&self, abs_tick: i64, is_upbeat: bool, swing_amount: f64, ticks_per_beat: i64) -> i64 {
        if !is_upbeat || swing_amount <= 0.0 {
            return abs_tick;
        }
        let triplet_offset = ticks_per_beat / 3;
        let swing_offset = (triplet_offset as f64 * swing_amount) as i64;
        abs_tick + swing_offset
    }

    /// Apply wrist-pattern contour + beat-1 emphasis to cymbal instruments.
    pub fn velocity_contour(&self, velocity: i32, instrument: Instrument, beat: i32, sub: f64) -> i32 {
        if !Self::is_contour_instrument(instrument) {
            return velocity;
        }
        let contour_amount = self.humanize_amount * 0.8;
        if contour_amount == 0.0 {
            return velocity;
        }
        let mut offset = Self::contour_offset(sub) * contour_amount;
        if beat == 1 {
            offset += 4.0 * contour_amount;
        }
        (velocity + offset as i32).clamp(1, 127)
    }

    /// Compute timing drift in ms for a bar based on section type.
    pub fn compute_section_drift_ms(&self, section_type: &str, bar_index: i32, total_bars: i32) -> f64 {
        let drift_amount = self.humanize_amount * 0.7;
        if drift_amount == 0.0 {
            return 0.0;
        }
        let profile = match drift_profile(section_type) {
            Some(p) => p,
            None => return 0.0,
        };
        let progress = if total_bars > 1 {
            bar_index as f64 / (total_bars - 1) as f64
        } else {
            0.0
        };
        match profile {
            "gradual_drag" => (bar_index as f64 * 1.0).min(6.0) * drift_amount,
            "constant_push" => -4.0 * drift_amount,
            "constant_drag" => 5.0 * drift_amount,
            "gradual_push" => -6.0 * progress * drift_amount,
            "fill_rush" => -8.0 * progress * drift_amount,
            _ => 0.0,
        }
    }

    /// Pull kick earlier when simultaneous with high-velocity snare. Returns new event list.
    pub fn apply_flam(&mut self, events: &[Event], tempo: f64, ppq: i64) -> Vec<Event> {
        if self.humanize_amount < 0.2 {
            return events.to_vec();
        }
        let ms_per_tick = (60000.0 / tempo) / ppq as f64;

        // Group events by tick
        let mut tick_map: std::collections::HashMap<i64, Vec<usize>> = std::collections::HashMap::new();
        for (i, event) in events.iter().enumerate() {
            tick_map.entry(event.tick).or_default().push(i);
        }

        let mut result = events.to_vec();

        for (_tick, indices) in &tick_map {
            let mut instruments: std::collections::HashMap<Instrument, usize> = std::collections::HashMap::new();
            let mut velocities: std::collections::HashMap<Instrument, i32> = std::collections::HashMap::new();

            for &i in indices {
                instruments.insert(result[i].instrument, i);
                velocities.insert(result[i].instrument, result[i].velocity);
            }

            let has_kick = instruments.contains_key(&Instrument::Kick)
                && *velocities.get(&Instrument::Kick).unwrap_or(&0) > 75;
            let has_snare = [Instrument::Snare, Instrument::SnareRim].iter().any(|inst| {
                instruments.contains_key(inst) && *velocities.get(inst).unwrap_or(&0) > 75
            });

            if has_kick && has_snare {
                let flam_ms = self.uniform(5.0, 12.0);
                let flam_ticks = (flam_ms / ms_per_tick) as i64;
                let flam_ticks = flam_ticks.max(1);
                if let Some(&ki) = instruments.get(&Instrument::Kick) {
                    result[ki].tick = (result[ki].tick - flam_ticks).max(0);
                }
            }
        }

        result
    }

    /// Pull ghost notes toward nearby snare accents. Returns new event list.
    pub fn apply_ghost_clustering(&mut self, events: &[Event], cluster_amount: f64, _tempo: f64, ppq: i64) -> Vec<Event> {
        let effective = cluster_amount * self.humanize_amount;
        if effective <= 0.0 {
            return events.to_vec();
        }
        let sixteenth_ticks = ppq / 4;

        let mut ghosts: Vec<Event> = Vec::new();
        let mut accents: Vec<Event> = Vec::new();
        let mut others: Vec<Event> = Vec::new();

        for event in events {
            if event.instrument == Instrument::SnareGhost
                || (event.instrument == Instrument::Snare && event.velocity < 50)
            {
                ghosts.push(event.clone());
            } else if (event.instrument == Instrument::Snare || event.instrument == Instrument::SnareRim)
                && event.velocity >= 100
            {
                accents.push(event.clone());
                others.push(event.clone());
            } else {
                others.push(event.clone());
            }
        }

        if accents.is_empty() || ghosts.is_empty() {
            return events.to_vec();
        }

        let mut accent_ticks: Vec<i64> = accents.iter().map(|e| e.tick).collect();
        accent_ticks.sort();
        accent_ticks.dedup();

        let mut new_ghosts: Vec<Event> = Vec::new();
        let mut all_ticks: std::collections::HashSet<i64> = events.iter().map(|e| e.tick).collect();

        for ghost in &mut ghosts {
            let nearest = *accent_ticks.iter()
                .min_by_key(|&&t| (t - ghost.tick).abs())
                .unwrap();
            let distance = nearest - ghost.tick;
            let pull = ((distance as f64 * effective * 0.4) as i64)
                .clamp(-sixteenth_ticks, sixteenth_ticks);
            ghost.tick = (ghost.tick + pull).max(0);
            let jitter = self.randint(-5, 5) as i64;
            ghost.tick = (ghost.tick + jitter).max(0);

            if self.random() < effective * 0.3 {
                let new_tick = if distance > 0 {
                    nearest + self.randint(sixteenth_ticks as i32 / 2, sixteenth_ticks as i32) as i64
                } else {
                    nearest - self.randint(sixteenth_ticks as i32 / 2, sixteenth_ticks as i32) as i64
                };
                let new_tick = new_tick.max(0);
                let too_close = all_ticks.iter().any(|&t| (new_tick - t).abs() < sixteenth_ticks / 4);
                if !too_close {
                    let new_vel = self.randint(20, 45);
                    new_ghosts.push(Event {
                        tick: new_tick,
                        instrument: Instrument::SnareGhost,
                        velocity: new_vel,
                    });
                    all_ticks.insert(new_tick);
                }
            }
        }

        let mut result = others;
        result.extend(ghosts);
        result.extend(new_ghosts);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_velocity_ranges() {
        let mut h = Humanizer::new(0.5, 42);
        // Accent should be in the high range
        for _ in 0..20 {
            let vel = h.humanize_velocity(VelocityLevel::Accent, Instrument::Snare);
            assert!(vel >= 1 && vel <= 127);
        }
        // Ghost should be in the low range
        for _ in 0..20 {
            let vel = h.humanize_velocity(VelocityLevel::Ghost, Instrument::Snare);
            assert!(vel >= 1 && vel <= 127);
        }
    }

    #[test]
    fn test_humanize_at_zero() {
        let mut h = Humanizer::new(0.0, 42);
        // At humanize=0, timing should still work but with minimal jitter
        let tick = h.humanize_timing(1000, Instrument::Kick, 120.0, 480);
        // Should be close to 1000 (kick has 0ms tendency)
        assert!((tick - 1000).abs() < 50);
    }

    #[test]
    fn test_velocity_contour() {
        let h = Humanizer::new(0.5, 42);
        let base_vel = 90;
        // Beat 1, sub 0.0 should get a boost (contour +6 * 0.4 + beat1 +4 * 0.4)
        let boosted = h.velocity_contour(base_vel, Instrument::Ride, 1, 0.0);
        assert!(boosted > base_vel);
        // Non-cymbal should be unchanged
        let unchanged = h.velocity_contour(base_vel, Instrument::Kick, 1, 0.0);
        assert_eq!(unchanged, base_vel);
    }

    #[test]
    fn test_section_drift() {
        let h = Humanizer::new(0.5, 42);
        // Verse should drag (positive ms)
        let drift = h.compute_section_drift_ms("verse", 3, 8);
        assert!(drift > 0.0);
        // Blast should push (negative ms)
        let drift = h.compute_section_drift_ms("blast", 3, 8);
        assert!(drift < 0.0);
    }

    #[test]
    fn test_infer_section_type() {
        assert_eq!(infer_section_type(&["blast".into(), "intense".into()]), "blast");
        assert_eq!(infer_section_type(&["driving".into()]), "drive");
        assert_eq!(infer_section_type(&["groovy".into()]), "verse");
    }

    #[test]
    fn test_cluster_amount() {
        assert!((get_cluster_amount(&["faraquet".into()]) - 0.7).abs() < 0.001);
        assert!((get_cluster_amount(&["shellac".into()]) - 0.0).abs() < 0.001);
        assert!((get_cluster_amount(&["unknown_tag".into()]) - 0.3).abs() < 0.001);
    }
}
