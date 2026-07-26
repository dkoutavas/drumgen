use rand::Rng;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use super::cell::{Cell, CellType, Hit, Instrument, VelocityLevel, layer_instruments};
use super::cell_library::CellLibrary;
use super::humanizer::{Event, Humanizer, get_cluster_amount, infer_section_type};
use super::midi_math::{self, TimeSigEntry, PPQ};

/// Result of pattern assembly.
#[derive(Debug, Clone)]
pub struct AssembleResult {
    pub events: Vec<Event>,
    pub tempo: f64,
    pub time_signatures: Vec<TimeSigEntry>,
    pub seed: u64,
    pub total_bars: i32,
    /// Name of the cell each arrangement section actually resolved to, in
    /// section order ("" for silence). Empty in loop mode. The GUI needs this
    /// to report what is PLAYING — the section type is only what the song form
    /// asked for, and a style with no blast cell does not start blasting just
    /// because the form said "blast".
    pub section_cells: Vec<String>,
}

/// Seed-keyed rotation into a candidate cell list — the "dice" for fixed-cell
/// styles. Deterministic per seed, so every dice press changes the groove.
fn rotate_pick<'a>(list: &[&'a Cell], seed: u64) -> &'a Cell {
    list[(seed as usize) % list.len()]
}

/// Per-section dynamics: (vel_base, vel_slope_per_bar, tension_mult).
/// Mirrors Python SECTION_DYNAMICS — vel_base shifts every hit's velocity,
/// vel_slope ramps it per bar, tension_mult scales the probability-grid
/// tension envelope so loud sections also run DENSER, not just harder.
fn section_dynamics(section_type: &str) -> (f64, f64, f64) {
    match section_type {
        "intro" => (-6.0, 0.5, 0.9),
        "atmospheric" => (-14.0, 0.0, 0.85),
        "verse" => (-3.0, 0.6, 1.0),
        "build" => (-12.0, 2.2, 1.05),
        "chorus" => (5.0, 0.4, 1.08),
        "drive" => (3.0, 0.5, 1.05),
        "blast" => (7.0, 0.0, 1.1),
        "breakdown" => (7.0, -0.8, 0.92),
        "outro" => (-2.0, -2.0, 0.9),
        "fill" => (4.0, 0.0, 1.0),
        _ => (0.0, 0.0, 1.0),
    }
}

/// Velocity offset for a bar within a section: base + slope * bar.
fn section_vel_offset(section_type: &str, bar_index: i32) -> i32 {
    let (base, slope, _) = section_dynamics(section_type);
    ((base + slope * bar_index as f64) as i32).clamp(-20, 20)
}

fn section_tension(section_type: &str) -> f64 {
    section_dynamics(section_type).2
}

/// Validate physical constraints at each position in a bar.
/// Keeps highest-priority cymbal, highest-priority stick, and all feet.
fn validate_physical_constraints(bar_hits: &[Hit]) -> Vec<Hit> {
    use std::collections::BTreeMap;

    // BTreeMap (not HashMap): group iteration order must be deterministic because
    // the surviving hits feed the per-hit humanize RNG stream downstream. With a
    // random HashMap order the same seed produced different MIDI across runs.
    let mut positions: BTreeMap<(i32, i64), Vec<&Hit>> = BTreeMap::new();
    for hit in bar_hits {
        // Use integer sub for grouping (avoid float comparison issues)
        let sub_key = (hit.sub * 1000.0) as i64;
        positions.entry((hit.beat, sub_key)).or_default().push(hit);
    }

    let mut filtered = Vec::new();

    for (_, hits) in &positions {
        let mut best_cymbal: Option<&Hit> = None;
        let mut best_cymbal_prio: i32 = -1;
        let mut best_stick: Option<&Hit> = None;
        let mut best_stick_prio: i32 = -1;

        for &hit in hits {
            if hit.instrument.is_cymbal() {
                let prio = hit.instrument.cymbal_priority();
                if prio > best_cymbal_prio {
                    best_cymbal_prio = prio;
                    best_cymbal = Some(hit);
                }
            }
            if hit.instrument.is_stick() {
                let prio = hit.instrument.stick_priority();
                if prio > best_stick_prio {
                    best_stick_prio = prio;
                    best_stick = Some(hit);
                }
            }
            if hit.instrument.is_foot() {
                filtered.push(hit.clone());
            }
        }

        if let Some(h) = best_cymbal {
            filtered.push(h.clone());
        }
        if let Some(h) = best_stick {
            filtered.push(h.clone());
        }
    }

    filtered
}

// ── Stage 1: shaped randomness (mirrors assembler.py) ──────────────────────
// Syncopation guard rails (Witek et al. 2014: groove pleasure peaks at MEDIUM
// syncopation). If a bar's kick/snare offbeat ratio falls outside the band the
// bar is re-rolled once; the second roll is kept regardless.
const SYNC_LO: f64 = 0.10;
const SYNC_HI: f64 = 0.60;
// Tension envelope: mid-band probabilities ramp across the pattern so later
// bars run slightly hotter — a shaped arc instead of a flat texture.
const TENSION_START: f64 = 0.88;
const TENSION_END: f64 = 1.10;

fn syncopation_ok(bar_hits: &[Hit]) -> bool {
    let core: Vec<&Hit> = bar_hits
        .iter()
        .filter(|h| matches!(h.instrument, Instrument::Kick | Instrument::Snare))
        .collect();
    if core.len() < 3 {
        return true;
    }
    let off = core.iter().filter(|h| h.sub != 0.0).count() as f64 / core.len() as f64;
    (SYNC_LO..=SYNC_HI).contains(&off)
}

/// One realization attempt for one bar of a probability grid.
#[allow(clippy::too_many_arguments)]
fn roll_bar(
    cell: &Cell,
    cell_bar: i32,
    output_bar: i32,
    pass_num: i32,
    total_passes: i32,
    tension: f64,
    rng: &mut ChaCha8Rng,
) -> Vec<Hit> {
    let mut bar_hits = Vec::new();
    let mut prev_fired = false;
    for entry in &cell.grid {
        if entry.bar != cell_bar {
            continue;
        }
        if !entry.condition.allows(pass_num, total_passes, prev_fired) {
            prev_fired = false;
            continue;
        }
        let p = if entry.probability > 0.15 && entry.probability < 0.85 {
            (entry.probability * tension).min(1.0)
        } else {
            entry.probability
        };
        let fired = rng.gen::<f64>() < p;
        prev_fired = fired;
        if fired {
            bar_hits.push(Hit {
                bar: output_bar,
                beat: entry.beat,
                sub: entry.sub,
                instrument: entry.instrument,
                velocity_level: entry.velocity_level,
            });
        }
    }
    bar_hits
}

/// Realize a probability grid into concrete hits. Trig conditions gate
/// entries by cell pass, a tension envelope ramps mid-band probabilities, and
/// a syncopation guard re-rolls a bar once when kick/snare leave the sweet
/// spot. Mirrors Python realize_probability_grid.
fn realize_probability_grid(
    cell: &Cell,
    bars: i32,
    rng: &mut ChaCha8Rng,
    tension_mult: f64,
) -> Vec<Hit> {
    let cell_num_bars = cell.num_bars;
    let total_passes = (bars + cell_num_bars - 1) / cell_num_bars;
    let mut all_hits = Vec::new();

    for bar_idx in 0..bars {
        let output_bar = bar_idx + 1;
        let cell_bar = (bar_idx % cell_num_bars) + 1;
        let pass_num = bar_idx / cell_num_bars + 1;
        let tension = tension_mult
            * (TENSION_START
                + (TENSION_END - TENSION_START) * (bar_idx as f64 / (bars - 1).max(1) as f64));

        let mut bar_hits =
            roll_bar(cell, cell_bar, output_bar, pass_num, total_passes, tension, rng);
        if !syncopation_ok(&bar_hits) {
            bar_hits = roll_bar(cell, cell_bar, output_bar, pass_num, total_passes, tension, rng);
        }

        bar_hits = validate_physical_constraints(&bar_hits);
        all_hits.extend(bar_hits);
    }

    all_hits
}

/// Euclidean rhythm, downbeat-anchored: slot i is an onset iff
/// (i * pulses) mod steps < pulses. Equivalent to Bjorklund up to rotation,
/// with slot 0 always an onset (E(3,8) -> x..x..x.). Mirrors Python.
fn euclid_pattern(pulses: i32, steps: i32) -> Vec<bool> {
    (0..steps).map(|i| (i * pulses) % steps < pulses).collect()
}

/// Realize a Euclidean cell's per-limb patterns over the whole output.
/// Each limb tiles its own `steps`-slot cycle across the pattern with NO bar
/// reset (polymeter). `dice_rotate` limbs add a seed-derived rotation; anchor
/// limbs stay put. Slots are sixteenths: 4 per beat in /4 meters, 2 per
/// (eighth-)beat in /8. Deterministic — consumes no RNG. Mirrors Python.
/// FNV-1a over (seed LE bytes, limb index). Mirrors _mix_seed in assembler.py
/// byte for byte — no RNG stream, both engines agree exactly.
fn mix_seed(seed: u64, limb_index: usize) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in seed.to_le_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h ^= (limb_index & 0xFF) as u64;
    h.wrapping_mul(0x100000001b3)
}

fn realize_euclidean(cell: &Cell, bars: i32, seed: u64) -> Vec<Hit> {
    let (num, den) = cell.time_sig;
    let slots_per_beat: i32 = if den == 8 { 2 } else { 4 };
    let spb = num * slots_per_beat;
    let mut hits = Vec::new();

    for (li, limb) in cell.limbs.iter().enumerate() {
        let steps = limb.steps;
        let pattern = euclid_pattern(limb.pulses, steps);
        let mut rot = limb.rotation;
        if limb.dice_rotate {
            // Hashed, not `(seed >> li) % steps`: with the raw seed, two seeds
            // whose difference is a multiple of `steps` rotated identically,
            // so a dice jump could land on a byte-identical realization — a
            // press that changed nothing. Mirrors _mix_seed in assembler.py.
            rot += (mix_seed(seed, li) % steps as u64) as i32;
        }
        for g in 0..(bars * spb) {
            if pattern[((g + rot).rem_euclid(steps)) as usize] {
                let bar = g / spb + 1;
                let within = g % spb;
                let beat = within / slots_per_beat + 1;
                let sub = (within % slots_per_beat) as f64
                    * if den == 8 { 0.5 } else { 0.25 };
                hits.push(Hit {
                    bar,
                    beat,
                    sub,
                    instrument: limb.instrument,
                    velocity_level: limb.velocity_level,
                });
            }
        }
    }

    let mut all_hits = Vec::new();
    for bar_number in 1..=bars {
        let bar_hits: Vec<Hit> =
            hits.iter().filter(|h| h.bar == bar_number).cloned().collect();
        all_hits.extend(validate_physical_constraints(&bar_hits));
    }
    all_hits
}

/// Resolve conflicts in merged layer hits.
fn resolve_layer_conflicts(merged_hits: &[Hit]) -> Vec<Hit> {
    use std::collections::BTreeMap;

    // BTreeMap for deterministic group and instrument iteration (see the note in
    // validate_physical_constraints — same seed must give the same MIDI).
    let mut positions: BTreeMap<(i32, i32, i64), Vec<&Hit>> = BTreeMap::new();
    for hit in merged_hits {
        let sub_key = (hit.sub * 1000.0) as i64;
        positions.entry((hit.bar, hit.beat, sub_key)).or_default().push(hit);
    }

    let mut filtered = Vec::new();

    for (_, hits) in &positions {
        // Deduplicate by instrument — keep highest velocity rank
        let mut by_inst: BTreeMap<Instrument, &Hit> = BTreeMap::new();
        for &hit in hits {
            let existing = by_inst.get(&hit.instrument);
            if existing.is_none()
                || hit.velocity_level.rank() > existing.unwrap().velocity_level.rank()
            {
                by_inst.insert(hit.instrument, hit);
            }
        }

        let deduped: Vec<&Hit> = by_inst.values().copied().collect();

        // Apply physical constraints
        let mut best_cymbal: Option<&Hit> = None;
        let mut best_cymbal_prio: i32 = -1;
        let mut best_stick: Option<&Hit> = None;
        let mut best_stick_prio: i32 = -1;

        for &hit in &deduped {
            if hit.instrument.is_cymbal() {
                let prio = hit.instrument.cymbal_priority();
                if prio > best_cymbal_prio {
                    best_cymbal_prio = prio;
                    best_cymbal = Some(hit);
                }
            }
            if hit.instrument.is_stick() {
                let prio = hit.instrument.stick_priority();
                if prio > best_stick_prio {
                    best_stick_prio = prio;
                    best_stick = Some(hit);
                }
            }
            if hit.instrument.is_foot() {
                filtered.push(hit.clone());
            }
        }

        if let Some(h) = best_cymbal {
            filtered.push(h.clone());
        }
        if let Some(h) = best_stick {
            filtered.push(h.clone());
        }
    }

    filtered
}

/// Vary hits for repeated bars.
fn vary_hits(hits: &[Hit], cell_bar: i32, vary_amount: f64, rng: &mut ChaCha8Rng, time_sig: (i32, i32)) -> Vec<Hit> {
    let mut mutated = hits.to_vec();
    let bar_hits: Vec<(usize, Hit)> = mutated.iter().enumerate()
        .filter(|(_, h)| h.bar == cell_bar)
        .map(|(i, h)| (i, h.clone()))
        .collect();

    let mut occupied: std::collections::HashSet<(i32, i64, Instrument)> = bar_hits.iter()
        .map(|(_, h)| (h.beat, (h.sub * 1000.0) as i64, h.instrument))
        .collect();

    let max_beats = time_sig.0;

    // Ghost note add (p = vary * 0.5)
    let mut new_hits = Vec::new();
    for (_, h) in &bar_hits {
        if h.instrument == Instrument::Snare && h.velocity_level == VelocityLevel::Accent
            && rng.gen::<f64>() < vary_amount * 0.5
        {
            let new_sub = h.sub + 0.25;
            let new_sub_key = (new_sub * 1000.0) as i64;
            if new_sub < 1.0 && !occupied.contains(&(h.beat, new_sub_key, Instrument::SnareGhost)) {
                new_hits.push(Hit {
                    bar: h.bar,
                    beat: h.beat,
                    sub: new_sub,
                    instrument: Instrument::SnareGhost,
                    velocity_level: VelocityLevel::Ghost,
                });
                occupied.insert((h.beat, new_sub_key, Instrument::SnareGhost));
            }
        }
    }
    mutated.extend(new_hits);

    // Kick displacement (p = vary * 0.3)
    for (idx, h) in &bar_hits {
        if h.instrument == Instrument::Kick
            && !(h.beat == 1 && h.sub == 0.0)
            && rng.gen::<f64>() < vary_amount * 0.3
        {
            let shift: f64 = if rng.gen::<bool>() { -0.25 } else { 0.25 };
            let mut new_sub = h.sub + shift;
            let mut new_beat = h.beat;
            if new_sub >= 1.0 {
                new_sub -= 1.0;
                new_beat += 1;
            } else if new_sub < 0.0 {
                new_sub += 1.0;
                new_beat -= 1;
            }
            let new_sub_key = (new_sub * 1000.0) as i64;
            if new_beat >= 1 && new_beat <= max_beats
                && !occupied.contains(&(new_beat, new_sub_key, Instrument::Kick))
            {
                let old_sub_key = (h.sub * 1000.0) as i64;
                occupied.remove(&(h.beat, old_sub_key, Instrument::Kick));
                mutated[*idx] = Hit {
                    bar: h.bar,
                    beat: new_beat,
                    sub: new_sub,
                    instrument: Instrument::Kick,
                    velocity_level: h.velocity_level,
                };
                occupied.insert((new_beat, new_sub_key, Instrument::Kick));
            }
        }
    }

    // Hi-hat open/close swap (p = vary * 0.2)
    let hh_indices: Vec<(usize, Hit)> = bar_hits.iter()
        .filter(|(_, h)| h.instrument == Instrument::HihatClosed)
        .cloned()
        .collect();
    if !hh_indices.is_empty() && rng.gen::<f64>() < vary_amount * 0.2 {
        let choice = rng.gen_range(0..hh_indices.len());
        let (swap_idx, swap_h) = &hh_indices[choice];
        mutated[*swap_idx] = Hit {
            bar: swap_h.bar,
            beat: swap_h.beat,
            sub: swap_h.sub,
            instrument: Instrument::HihatOpen,
            velocity_level: swap_h.velocity_level,
        };
    }

    // Ride accent shift (p = vary * 0.15)
    let ride_indices: Vec<(usize, Hit)> = bar_hits.iter()
        .filter(|(_, h)| h.instrument == Instrument::Ride)
        .cloned()
        .collect();
    if !ride_indices.is_empty() && rng.gen::<f64>() < vary_amount * 0.15 {
        let choice = rng.gen_range(0..ride_indices.len());
        let (swap_idx, swap_h) = &ride_indices[choice];
        let new_vel = if swap_h.velocity_level == VelocityLevel::Normal {
            VelocityLevel::Accent
        } else {
            VelocityLevel::Normal
        };
        mutated[*swap_idx] = Hit {
            bar: swap_h.bar,
            beat: swap_h.beat,
            sub: swap_h.sub,
            instrument: swap_h.instrument,
            velocity_level: new_vel,
        };
    }

    mutated
}

/// Process one bar of hits into events.
fn process_bar(
    bar_number: i32,
    cell_bar: i32,
    active_hits: &[Hit],
    cell: &Cell,
    humanizer: &mut Humanizer,
    tempo: f64,
    time_sig_list: &[TimeSigEntry],
    ppq: i64,
    beat_ticks: i64,
    swing: f64,
    humanize_override: Option<f64>,
    velocity_offset: i32,
    section_drift_ms: f64,
) -> Vec<Event> {
    let mut events = Vec::new();

    // Determine humanize amount for this bar
    let saved = humanizer.humanize_amount;
    let h_amount = if let Some(override_val) = humanize_override {
        override_val
    } else if let Some(ref per_bar) = cell.humanize_per_bar {
        per_bar.iter()
            .find(|&(&(start, end), _)| start <= cell_bar && cell_bar <= end)
            .map(|(_, &amount)| amount)
            .unwrap_or(cell.humanize)
    } else {
        cell.humanize
    };
    humanizer.humanize_amount = h_amount;

    let bar_hits: Vec<&Hit> = active_hits.iter().filter(|h| h.bar == cell_bar).collect();

    // Meter adapter. The bar's meter can differ from the cell's: a song-mode
    // section keeps its declared meter while the fallback cell keeps its own
    // beats. A 4/4 cell in a 6/4 bar covered beats 1-4 and left 5-6
    // structurally silent in EVERY bar — audible as the arc "entering silence"
    // under any forced non-4/4 meter. Vamp the head of the figure to fill the
    // bar (what a drummer does when the riff is short of the bar), and clip
    // hits past the barline (a wider cell used to bleed into the next bar).
    // Deterministic, order-stable, consumes no RNG. Mirrors _process_bar.
    let bar_beats = midi_math::get_time_sig_for_bar(bar_number, time_sig_list).0;
    let cell_beats = cell.time_sig.0;
    let adapted: Vec<Hit>;
    let bar_hits: Vec<&Hit> = if cell_beats != bar_beats {
        let mut hits: Vec<Hit> = bar_hits
            .iter()
            .filter(|h| h.beat <= bar_beats)
            .map(|&h| h.clone())
            .collect();
        let mut shift = cell_beats;
        while shift < bar_beats {
            for h in bar_hits.iter().filter(|h| h.beat + shift <= bar_beats) {
                hits.push(Hit { beat: h.beat + shift, ..(*h).clone() });
            }
            shift += cell_beats;
        }
        adapted = hits;
        adapted.iter().collect()
    } else {
        bar_hits
    };

    for hit in bar_hits {
        let mut abs_tick = midi_math::position_to_ticks(bar_number, hit.beat, hit.sub, time_sig_list, ppq);

        if swing > 0.0 {
            let is_upbeat = (hit.sub - 0.5).abs() < 0.01;
            abs_tick = humanizer.apply_swing(abs_tick, is_upbeat, swing, beat_ticks);
        }

        abs_tick = humanizer.humanize_timing(abs_tick, hit.instrument, tempo, ppq);

        // Section drift
        if section_drift_ms != 0.0 {
            let ms_per_tick = (60000.0 / tempo) / ppq as f64;
            abs_tick = (abs_tick + (section_drift_ms / ms_per_tick) as i64).max(0);
        }

        let mut velocity = humanizer.humanize_velocity(hit.velocity_level, hit.instrument);
        velocity = humanizer.velocity_contour(velocity, hit.instrument, hit.beat, hit.sub);
        velocity = (velocity + velocity_offset).clamp(1, 127);

        events.push(Event {
            tick: abs_tick,
            instrument: hit.instrument,
            velocity,
        });
    }

    humanizer.humanize_amount = saved;
    events
}

/// Assemble a single-cell pattern.
pub fn assemble(
    library: &CellLibrary,
    style: Option<&str>,
    cell_name: Option<&str>,
    bars: i32,
    tempo: f64,
    time_sig: (i32, i32),
    humanize: Option<f64>,
    swing: f64,
    fill_every: i32,
    seed: u64,
    vary: f64,
    // Kept for Python-signature parity; selection no longer prefers prob cells
    // (see the rotation comment below) and realization keys off the cell type.
    _generative: bool,
) -> AssembleResult {
    // Meter (0,0) means "Auto" — no meter filter, take the style's native meter.
    let meter_filter: Option<(i32, i32)> = if time_sig == (0, 0) { None } else { Some(time_sig) };
    let meter_ok = |c: &Cell| meter_filter.map_or(true, |m| c.time_sig == m);

    // Resolve cell
    let cell = if let Some(name) = cell_name {
        library.get_cell(name)
            .unwrap_or_else(|| library.get_pool("screamo").first().copied()
                .expect("No cells available"))
    } else if let Some(style_name) = style {
        let pool = library.get_pool(style_name);
        if pool.is_empty() {
            library.get_pool("screamo").first().copied()
                .expect("No cells available")
        } else {
            // Rotate over the FULL meter-matched pool — deliberately ignoring
            // `generative` here (Python's assemble prefers prob_match[0]; the
            // rotation is plugin dice semantics, divergent since Phase 4b).
            // The old prob-only filter collapsed styles onto shared single
            // probability cells: rotate_pick over a 1-element list is
            // seed-invariant, which made 13 of 31 styles bit-identical.
            // Probability cells still re-realize per seed when rotation lands
            // on one (is_probability path below).
            let ts_match: Vec<&Cell> = pool.iter().filter(|c| meter_ok(c)).copied().collect();
            if !ts_match.is_empty() { rotate_pick(&ts_match, seed) } else { rotate_pick(&pool, seed) }
        }
    } else {
        library.get_pool("screamo").first().copied()
            .expect("No cells available")
    };

    // Meter truth: the loop's time signature is the CHOSEN cell's actual meter,
    // not the requested one — otherwise the recorded MIDI's bar grid lies when a
    // fallback picked a different-meter cell.
    let (num, den) = cell.time_sig;
    let time_signatures = vec![TimeSigEntry {
        bar_start: 1,
        bar_end: bars,
        numerator: num,
        denominator: den,
    }];

    let humanize_amount = humanize.unwrap_or(cell.humanize);
    let mut humanizer = Humanizer::new(humanize_amount, seed);
    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    let ppq = PPQ;
    let beat_ticks = ppq * 4 / den as i64;

    let is_prob = cell.is_probability();
    let is_euclid = cell.is_euclidean();

    // Fill candidates mirror Python assemble(): meter-matched (a 4/4 fill in a
    // 3/4 bar overflows the bar), tag-scored against the groove. The actual
    // fill is drawn PER FILL BAR inside the loop so consecutive fill bars
    // alternate; a single-cell top set falls back to all meter-matched fills.
    // Scoring consumes no RNG, so toggling FILL no longer shifts realization.
    let fill_candidates: Vec<&Cell> = if fill_every > 0 {
        let fills: Vec<&Cell> = library
            .get_fill_cells()
            .into_iter()
            .filter(|f| f.time_sig == (num, den))
            .collect();
        if fills.is_empty() {
            Vec::new()
        } else {
            let scored: Vec<(usize, &Cell)> = fills
                .iter()
                .map(|f| (f.tags.iter().filter(|t| cell.tags.contains(t)).count(), *f))
                .collect();
            let best = scored.iter().map(|(s, _)| *s).max().unwrap_or(0);
            let top: Vec<&Cell> =
                scored.iter().filter(|(s, _)| *s == best).map(|(_, c)| *c).collect();
            if top.len() > 1 { top } else { fills }
        }
    } else {
        Vec::new()
    };

    let cell_hits = if is_prob {
        realize_probability_grid(cell, bars, &mut rng, 1.0)
    } else if is_euclid {
        realize_euclidean(cell, bars, seed)
    } else {
        cell.hits.clone()
    };

    let mut events = Vec::new();
    let mut seen_cell_bars = std::collections::HashSet::new();
    let section_type = infer_section_type(&cell.tags);
    let mut last_fill: Option<usize> = None;

    for bar_idx in 0..bars {
        let bar_number = bar_idx + 1;

        let is_fill = !fill_candidates.is_empty() && bar_number % fill_every.max(1) == 0;

        let (mut active_hits, active_cell, cell_bar) = if is_fill {
            let mut pick = rng.gen_range(0..fill_candidates.len());
            // Avoid the same fill twice in a row when there is a choice.
            if fill_candidates.len() > 1 && Some(pick) == last_fill {
                pick = rng.gen_range(0..fill_candidates.len());
            }
            last_fill = Some(pick);
            let f = fill_candidates[pick];
            (f.hits.clone(), f, (bar_idx % f.num_bars) + 1)
        } else if is_prob || is_euclid {
            // Realized hits already carry correct output bar numbers.
            (cell_hits.clone(), cell, bar_number)
        } else {
            (cell_hits.clone(), cell, (bar_idx % cell.num_bars) + 1)
        };

        // Vary mutations on repeated cell bars (skip for generative cells —
        // prob re-realizes per seed and euclidean phasing never repeats).
        // time_sig for vary is the RESOLVED meter, not the requested one
        // ((0,0) = Auto must never reach vary_hits).
        if !(is_prob || is_euclid) && vary > 0.0 && seen_cell_bars.contains(&cell_bar) {
            active_hits = vary_hits(&active_hits, cell_bar, vary, &mut rng, (num, den));
        }
        seen_cell_bars.insert(cell_bar);

        let drift_ms = humanizer.compute_section_drift_ms(section_type, bar_idx, bars);
        let bar_events = process_bar(
            bar_number, cell_bar, &active_hits, active_cell, &mut humanizer,
            tempo, &time_signatures, ppq, beat_ticks, swing, humanize,
            0, drift_ms,
        );
        events.extend(bar_events);
    }

    // Add crash on bar 1 beat 1 if not present
    // Matches Python's inst.startswith("crash") — includes the choke variants so a
    // cell that already opens on a crash choke doesn't get a redundant crash added.
    let has_crash_bar1 = events.iter().any(|e| {
        matches!(
            e.instrument,
            Instrument::Crash1 | Instrument::Crash2 | Instrument::Crash1Choke | Instrument::Crash2Choke
        ) && e.tick < beat_ticks
    });
    if !has_crash_bar1 && bars > 0 {
        let crash_tick = midi_math::position_to_ticks(1, 1, 0.0, &time_signatures, ppq);
        let crash_tick = humanizer.humanize_timing(crash_tick, Instrument::Crash1, tempo, ppq);
        let crash_vel = humanizer.humanize_velocity(VelocityLevel::Accent, Instrument::Crash1);
        events.push(Event {
            tick: crash_tick,
            instrument: Instrument::Crash1,
            velocity: crash_vel,
        });
    }

    // Post-processing: flam and ghost clustering
    events = humanizer.apply_flam(&events, tempo, ppq);
    let cluster_amt = get_cluster_amount(&cell.tags);
    events = humanizer.apply_ghost_clustering(&events, cluster_amt, tempo, ppq);

    events.sort_by(|a, b| a.tick.cmp(&b.tick).then_with(|| a.instrument.as_str().cmp(b.instrument.as_str())));

    AssembleResult {
        events,
        tempo,
        time_signatures,
        seed,
        total_bars: bars,
        section_cells: Vec::new(),
    }
}

/// Parsed arrangement section.
pub struct ArrangementSection {
    pub bars: i32,
    pub section_type: String,
    pub time_sig: (i32, i32),
}

/// Parse an arrangement string like "4:build 8:drive@7/8 2:blast".
pub fn parse_arrangement(arrangement_str: &str, default_time_sig: (i32, i32)) -> Vec<ArrangementSection> {
    let mut sections = Vec::new();
    for token in arrangement_str.split_whitespace() {
        if !token.contains(':') {
            continue;
        }
        let parts: Vec<&str> = token.splitn(2, ':').collect();
        let count: i32 = parts[0].parse().unwrap_or(4);
        let rest = parts[1];

        let (section_type, ts) = if rest.contains('@') {
            let ts_parts: Vec<&str> = rest.splitn(2, '@').collect();
            let ts_str = ts_parts[1];
            let ts_nums: Vec<i32> = ts_str.split('/').filter_map(|x| x.parse().ok()).collect();
            let ts = if ts_nums.len() >= 2 { (ts_nums[0], ts_nums[1]) } else { default_time_sig };
            (ts_parts[0].to_lowercase(), ts)
        } else {
            (rest.to_lowercase(), default_time_sig)
        };

        sections.push(ArrangementSection {
            bars: count.max(1),
            section_type,
            time_sig: ts,
        });
    }
    sections
}

/// Consolidate adjacent time signature entries with the same values.
fn consolidate_time_signatures(time_sigs: &[TimeSigEntry]) -> Vec<TimeSigEntry> {
    if time_sigs.is_empty() {
        return Vec::new();
    }
    let mut result = vec![time_sigs[0].clone()];
    for ts in &time_sigs[1..] {
        let prev = result.last_mut().unwrap();
        if ts.numerator == prev.numerator && ts.denominator == prev.denominator {
            prev.bar_end = ts.bar_end;
        } else {
            result.push(ts.clone());
        }
    }
    result
}

/// Section types that get a crash+kick on beat 1.
fn is_intense_section(section_type: &str) -> bool {
    matches!(section_type, "chorus" | "blast" | "breakdown" | "drive")
}

/// Assemble an arrangement (multiple sections).
pub fn assemble_arrangement(
    library: &CellLibrary,
    style: &str,
    arrangement_str: &str,
    tempo: f64,
    default_time_sig: (i32, i32),
    humanize: Option<f64>,
    swing: f64,
    seed: u64,
    vary: f64,
    generative: bool,
) -> AssembleResult {
    let sections = parse_arrangement(arrangement_str, default_time_sig);
    let pool = library.get_pool(style);
    let total_bars: i32 = sections.iter().map(|s| s.bars).sum();

    // Build time signatures
    let mut raw_time_sigs = Vec::new();
    let mut bar_cursor = 0;
    for section in &sections {
        raw_time_sigs.push(TimeSigEntry {
            bar_start: bar_cursor + 1,
            bar_end: bar_cursor + section.bars,
            numerator: section.time_sig.0,
            denominator: section.time_sig.1,
        });
        bar_cursor += section.bars;
    }
    let time_signatures = consolidate_time_signatures(&raw_time_sigs);

    let ppq = PPQ;
    let mut humanizer = Humanizer::new(humanize.unwrap_or(0.3), seed);
    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    let mut events = Vec::new();
    let mut bar_cursor = 0;
    // Cells actually chosen, for the ghost-clustering amount below.
    let mut used_cells: Vec<&Cell> = Vec::with_capacity(sections.len());
    // Same, but positional (one entry per section, "" for silence) so the GUI
    // can report what is playing instead of what the form asked for.
    let mut section_cells: Vec<String> = Vec::with_capacity(sections.len());

    for (sec_idx, section) in sections.iter().enumerate() {
        let (sec_num, sec_den) = section.time_sig;
        let beat_ticks = ppq * 4 / sec_den as i64;
        // The upcoming section steers fill choice (into_* tags).
        let next_section = sections.get(sec_idx + 1).map(|sec| sec.section_type.as_str());

        // Score the WHOLE pool against the section, then prefer a per-seed
        // varying cell only among equal scorers.
        //
        // This used to narrow the pool to probability/euclidean cells BEFORE
        // scoring, which subordinated section intent to generativity: most
        // styles own one or two grids, so build, blast and breakdown all
        // collapsed onto the same cell and the fixed blast cell was
        // unreachable. The telegraph announced "BLAST NOW" over the same
        // groove as the verse, eight bars louder. blast_traditional scores 7
        // for a blast section against prob_screamo_4_4's 3 — let the score
        // say so.
        let cell = match library.get_cell_for_section(
            &pool, &section.section_type,
            Some((sec_num, sec_den)), &mut rng, next_section, generative,
        ) {
            Some(c) => c,
            None => {
                // Silence section
                section_cells.push(String::new());
                bar_cursor += section.bars;
                continue;
            }
        };
        used_cells.push(cell);
        section_cells.push(cell.name.clone());

        // The section's bar grid stays at the REQUESTED meter (it is what the
        // arrangement asked for and what the host follows); when no cell in the
        // pool matches, the fallback cell's own meter differs and the groove
        // reads oddly against that grid. Python prints the same warning.
        if cell.time_sig != (sec_num, sec_den) {
            nih_plug::nih_log!(
                "drumgen: section '{}' has no {}/{} cell — using {}/{} cell '{}'",
                section.section_type, sec_num, sec_den,
                cell.time_sig.0, cell.time_sig.1, cell.name
            );
        }

        let is_prob = cell.is_probability();
        let is_euclid = cell.is_euclidean();
        let cell_hits = if is_prob {
            realize_probability_grid(cell, section.bars, &mut rng, section_tension(&section.section_type))
        } else if is_euclid {
            realize_euclidean(cell, section.bars, seed)
        } else {
            cell.hits.clone()
        };

        let cell_humanize = humanize.unwrap_or(cell.humanize);
        humanizer.humanize_amount = cell_humanize;

        // Crash+kick on intense sections
        let section_start_bar = bar_cursor + 1;
        if is_intense_section(&section.section_type) {
            let crash_tick = midi_math::position_to_ticks(section_start_bar, 1, 0.0, &time_signatures, ppq);
            let crash_tick_h = humanizer.humanize_timing(crash_tick, Instrument::Crash1, tempo, ppq);
            let crash_vel = humanizer.humanize_velocity(VelocityLevel::Accent, Instrument::Crash1);
            events.push(Event { tick: crash_tick_h, instrument: Instrument::Crash1, velocity: crash_vel });

            let kick_tick = midi_math::position_to_ticks(section_start_bar, 1, 0.0, &time_signatures, ppq);
            let kick_tick_h = humanizer.humanize_timing(kick_tick, Instrument::Kick, tempo, ppq);
            let kick_vel = humanizer.humanize_velocity(VelocityLevel::Accent, Instrument::Kick);
            events.push(Event { tick: kick_tick_h, instrument: Instrument::Kick, velocity: kick_vel });
        }

        let mut seen_cell_bars = std::collections::HashSet::new();

        for i in 0..section.bars {
            let bar_number = bar_cursor + i + 1;

            let cell_bar = if is_prob || is_euclid {
                // Realized hits (prob AND euclidean) are keyed by output bar
                // within the section; the fixed-cell modulo would replay
                // local bar 1 forever and discard euclidean phasing.
                i + 1
            } else {
                (i % cell.num_bars) + 1
            };

            let vel_offset = section_vel_offset(&section.section_type, i);

            let mut current_hits = cell_hits.clone();
            if !(is_prob || is_euclid) && vary > 0.0 && seen_cell_bars.contains(&cell_bar) {
                current_hits = vary_hits(&cell_hits, cell_bar, vary, &mut rng, (sec_num, sec_den));
            }
            seen_cell_bars.insert(cell_bar);

            let drift_ms = humanizer.compute_section_drift_ms(&section.section_type, i, section.bars);

            if is_prob || is_euclid {
                // Remap realized hits to the correct global bar number
                let remapped: Vec<Hit> = current_hits.iter()
                    .filter(|h| h.bar == cell_bar)
                    .map(|h| Hit {
                        bar: bar_number,
                        beat: h.beat,
                        sub: h.sub,
                        instrument: h.instrument,
                        velocity_level: h.velocity_level,
                    })
                    .collect();
                let bar_events = process_bar(
                    bar_number, bar_number, &remapped, cell, &mut humanizer,
                    tempo, &time_signatures, ppq, beat_ticks, swing, humanize,
                    vel_offset, drift_ms,
                );
                events.extend(bar_events);
            } else {
                let bar_events = process_bar(
                    bar_number, cell_bar, &current_hits, cell, &mut humanizer,
                    tempo, &time_signatures, ppq, beat_ticks, swing, humanize,
                    vel_offset, drift_ms,
                );
                events.extend(bar_events);
            }
        }

        bar_cursor += section.bars;
    }

    // Post-processing
    events = humanizer.apply_flam(&events, tempo, ppq);
    // Ghost clustering follows the cells actually PLAYED, not pool[0] — a
    // shellac section (0.0) next to a faraquet one (0.7) should take the
    // busier amount, exactly as Python's `max(... for c in used_cells)` does.
    let cluster_amt = used_cells
        .iter()
        .map(|c| get_cluster_amount(&c.tags))
        .fold(f64::NEG_INFINITY, f64::max);
    let cluster_amt = if cluster_amt.is_finite() { cluster_amt } else { 0.3 };
    events = humanizer.apply_ghost_clustering(&events, cluster_amt, tempo, ppq);
    events.sort_by(|a, b| a.tick.cmp(&b.tick).then_with(|| a.instrument.as_str().cmp(b.instrument.as_str())));

    AssembleResult {
        events,
        tempo,
        time_signatures,
        seed,
        total_bars,
        section_cells,
    }
}

/// Assemble a layered pattern (mixing instrument groups from different cells).
pub fn assemble_layered(
    library: &CellLibrary,
    layers: &std::collections::HashMap<String, String>,
    bars: i32,
    tempo: f64,
    time_sig: (i32, i32),
    humanize: Option<f64>,
    swing: f64,
    seed: u64,
    _vary: f64,
) -> AssembleResult {
    let (num, den) = time_sig;
    let time_signatures = vec![TimeSigEntry {
        bar_start: 1,
        bar_end: bars,
        numerator: num,
        denominator: den,
    }];

    let ppq = PPQ;
    let beat_ticks = ppq * 4 / den as i64;
    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    // Load layer cells
    let mut layer_cells: Vec<(&str, &Cell)> = Vec::new();
    let mut humanize_values = Vec::new();
    for (layer_name, cell_name) in layers {
        if let Some(cell) = library.get_cell(cell_name) {
            layer_cells.push((layer_name.as_str(), cell));
            humanize_values.push(cell.humanize);
        }
    }

    // Python: `min(humanize_values) if humanize_values else 0.3`. The old
    // `.min(0.3)` capped every layered pattern at 0.3 humanize, which is not
    // what the reference does — 0.3 is the EMPTY default, not a ceiling.
    let humanize_amount = humanize.unwrap_or_else(|| {
        if humanize_values.is_empty() {
            0.3
        } else {
            humanize_values.iter().cloned().fold(f64::INFINITY, f64::min)
        }
    });
    let mut humanizer = Humanizer::new(humanize_amount, seed);

    let dummy_cell = Cell {
        name: "__layered__".to_string(),
        tags: Vec::new(),
        time_sig,
        num_bars: 1,
        humanize: humanize_amount,
        role: "groove".to_string(),
        cell_type: CellType::Fixed,
        hits: Vec::new(),
        grid: Vec::new(),
        limbs: Vec::new(),
        humanize_per_bar: None,
    };

    let mut events = Vec::new();

    for bar_idx in 0..bars {
        let bar_number = bar_idx + 1;
        let mut merged_hits = Vec::new();

        for (layer_name, cell) in &layer_cells {
            let cell_bar = (bar_idx % cell.num_bars) + 1;

            let layer_hits: Vec<Hit> = if cell.is_probability() {
                let realized = realize_probability_grid(cell, cell.num_bars, &mut rng, 1.0);
                realized.into_iter().filter(|h| h.bar == cell_bar).collect()
            } else if cell.is_euclidean() {
                let realized = realize_euclidean(cell, cell.num_bars, seed);
                realized.into_iter().filter(|h| h.bar == cell_bar).collect()
            } else {
                cell.hits.iter().filter(|h| h.bar == cell_bar).cloned().collect()
            };

            // Filter to layer instruments
            if let Some(allowed) = layer_instruments(layer_name) {
                for h in layer_hits {
                    if allowed.contains(&h.instrument) {
                        merged_hits.push(Hit {
                            bar: bar_number,
                            beat: h.beat,
                            sub: h.sub,
                            instrument: h.instrument,
                            velocity_level: h.velocity_level,
                        });
                    }
                }
            }
        }

        merged_hits = resolve_layer_conflicts(&merged_hits);

        let drift_ms = humanizer.compute_section_drift_ms("verse", bar_idx, bars);
        let bar_events = process_bar(
            bar_number, bar_number, &merged_hits, &dummy_cell, &mut humanizer,
            tempo, &time_signatures, ppq, beat_ticks, swing, humanize,
            0, drift_ms,
        );
        events.extend(bar_events);
    }

    // Post-processing
    events = humanizer.apply_flam(&events, tempo, ppq);
    let cluster_amt = layer_cells.iter()
        .map(|(_, c)| get_cluster_amount(&c.tags))
        .fold(0.0_f64, f64::max);
    events = humanizer.apply_ghost_clustering(&events, cluster_amt, tempo, ppq);
    events.sort_by(|a, b| a.tick.cmp(&b.tick).then_with(|| a.instrument.as_str().cmp(b.instrument.as_str())));

    AssembleResult {
        events,
        tempo,
        time_signatures,
        seed,
        total_bars: bars,
        section_cells: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assemble_basic() {
        let lib = CellLibrary::new();
        let result = assemble(&lib, Some("screamo"), None, 4, 120.0, (4, 4), None, 0.0, 0, 42, 0.0, false);
        assert!(!result.events.is_empty(), "Should produce events");
        assert_eq!(result.total_bars, 4);
        assert_eq!(result.seed, 42);
    }

    #[test]
    fn test_meter_truth_stamps_chosen_cell_meter() {
        let lib = CellLibrary::new();
        // A 7/8 cell requested under 4/4 must still be stamped 7/8 (bar = 7*240),
        // not the requested meter — otherwise the recorded MIDI's grid lies.
        let r = assemble(&lib, None, Some("driving_7_8"), 2, 120.0, (4, 4), Some(0.0), 0.0, 0, 0, 0.0, false);
        assert_eq!(r.time_signatures[0].numerator, 7);
        assert_eq!(r.time_signatures[0].denominator, 8);
    }

    #[test]
    fn test_meter_forced_and_auto() {
        let lib = CellLibrary::new();
        // Forcing 7/8 on posthardcore selects a 7/8 cell.
        let forced = assemble(&lib, Some("posthardcore"), None, 4, 120.0, (7, 8), Some(0.0), 0.0, 0, 0, 0.0, true);
        assert_eq!((forced.time_signatures[0].numerator, forced.time_signatures[0].denominator), (7, 8));
        // Auto (0,0) uses the style's native meter and must not overshoot.
        let auto = assemble(&lib, Some("posthardcore"), None, 4, 120.0, (0, 0), Some(0.0), 0.0, 0, 0, 0.0, true);
        assert!(auto.time_signatures[0].denominator == 4 || auto.time_signatures[0].denominator == 8);
    }

    #[test]
    fn test_dice_rotation_changes_groove() {
        // Non-generative fixed-cell style: different seeds must be able to select
        // different cells (the dice for fixed-cell styles). Collect the event
        // signature across seeds and assert not all identical.
        let lib = CellLibrary::new();
        let sig = |seed: u64| -> Vec<(i64, Instrument)> {
            assemble(&lib, Some("screamo"), None, 4, 120.0, (0, 0), Some(0.0), 0.0, 0, seed, 0.0, false)
                .events.iter().map(|e| (e.tick, e.instrument)).collect()
        };
        let base = sig(0);
        let differs = (1..8).any(|s| sig(s) != base);
        assert!(differs, "dice (seed rotation) should change the groove across seeds");
    }

    #[test]
    fn test_generative_seed_reproducible_across_runs() {
        // Regression: HashMap iteration order in constraint/flam paths used to
        // make the same seed produce different MIDI across runs. Generate the
        // same generative pattern 20x with humanize on and assert every run is
        // byte-identical (tick, instrument, velocity).
        let lib = CellLibrary::new();
        let key = |r: &AssembleResult| -> Vec<(i64, Instrument, i32)> {
            r.events.iter().map(|e| (e.tick, e.instrument, e.velocity)).collect()
        };
        let first = key(&assemble(
            &lib, Some("posthardcore"), None, 4, 160.0, (4, 4), Some(0.7), 0.0, 0, 7, 0.0, true,
        ));
        assert!(!first.is_empty());
        for _ in 0..20 {
            let again = key(&assemble(
                &lib, Some("posthardcore"), None, 4, 160.0, (4, 4), Some(0.7), 0.0, 0, 7, 0.0, true,
            ));
            assert_eq!(first, again, "same seed must yield identical MIDI every run");
        }
    }

    #[test]
    fn test_assemble_by_cell_name() {
        let lib = CellLibrary::new();
        let result = assemble(&lib, None, Some("blast_traditional"), 2, 200.0, (4, 4), None, 0.0, 0, 42, 0.0, false);
        assert!(!result.events.is_empty());
    }

    #[test]
    fn test_fill_every_swaps_only_fill_bars() {
        // Fixed cell + humanize 0 => fully deterministic except the fill swap.
        // With fill_every=4 over 4 bars, bars 1-3 must be identical to the
        // no-fill run and bar 4 must differ (the tag-matched fill takes over).
        let lib = CellLibrary::new();
        assert!(!lib.get_fill_cells().is_empty(), "builtin.json must ship fill-role cells");
        let run = |fill_every: i32| {
            assemble(&lib, None, Some("blast_traditional"), 4, 120.0, (4, 4), Some(0.0), 0.0, fill_every, 42, 0.0, false)
        };
        let no_fill = run(0);
        let with_fill = run(4);
        let bar4_start = 3 * 4 * PPQ;
        let split = |r: &AssembleResult| {
            // The bar-1 auto-crash is excluded: its velocity is drawn from the
            // humanizer RNG *after* the bar loop, so its stream position (and
            // thus the value) legitimately shifts when bar 4's content changes.
            // Same ordering as Python.
            let head: Vec<_> = r.events.iter()
                .filter(|e| e.tick < bar4_start && e.instrument != Instrument::Crash1)
                .map(|e| (e.tick, e.instrument, e.velocity)).collect();
            let tail: Vec<_> = r.events.iter().filter(|e| e.tick >= bar4_start)
                .map(|e| (e.tick, e.instrument, e.velocity)).collect();
            (head, tail)
        };
        let (head_a, tail_a) = split(&no_fill);
        let (head_b, tail_b) = split(&with_fill);
        assert_eq!(head_a, head_b, "non-fill bars must be untouched by fill_every");
        assert_ne!(tail_a, tail_b, "the fill bar must actually change");
    }

    #[test]
    fn test_euclid_pattern_matches_python() {
        // Same downbeat-anchored modulo form as assembler.py _euclid_pattern.
        assert_eq!(
            euclid_pattern(3, 8),
            vec![true, false, false, true, false, false, true, false]
        );
        let p = euclid_pattern(5, 8);
        assert_eq!(p.iter().filter(|&&v| v).count(), 5);
        assert_eq!(euclid_pattern(4, 4), vec![true; 4]);
    }

    #[test]
    fn test_trig_condition_semantics() {
        use super::super::cell::TrigCond;
        let r22 = TrigCond::from_str("2:2");
        assert!(r22.allows(2, 8, false) && r22.allows(4, 8, false) && !r22.allows(1, 8, false));
        let r44 = TrigCond::from_str("4:4");
        assert!(r44.allows(4, 4, false) && !r44.allows(3, 4, false));
        assert!(TrigCond::from_str("1st").allows(1, 4, false));
        assert!(!TrigCond::from_str("1st").allows(2, 4, false));
        assert!(TrigCond::from_str("last").allows(4, 4, false));
        assert!(TrigCond::from_str("pre").allows(1, 4, true));
        assert!(!TrigCond::from_str("pre").allows(1, 4, false));
        assert!(TrigCond::from_str("!pre").allows(1, 4, false));
        // Empty = always; unknown fails open — same as Python.
        assert!(TrigCond::from_str("").allows(3, 4, false));
        assert!(TrigCond::from_str("wat").allows(3, 4, false));
    }

    #[test]
    fn test_conditioned_entry_gates_by_pass() {
        // Mirror of the Python test: a "2:2" china fires only on passes 2 and 4.
        let json = r#"{
            "cells": {
                "t": {"name": "t", "tags": ["generative"], "time_sig": [4, 4],
                      "num_bars": 1, "humanize": 0.0, "role": "groove", "type": "probability",
                      "grid": [[1, 0.0, "kick", 1.0, "accent"],
                               [3, 0.0, "china", 1.0, "accent", "2:2"]]}
            },
            "style_pools": {"t": ["t"]},
            "section_preferences": {}
        }"#;
        let lib = CellLibrary::from_json(json);
        let r = assemble(&lib, None, Some("t"), 4, 120.0, (4, 4), Some(0.0), 0.0, 0, 0, 0.0, true);
        let bar_ticks = 4 * PPQ;
        let mut china_bars: Vec<i64> = r
            .events
            .iter()
            .filter(|e| e.instrument == Instrument::China)
            .map(|e| e.tick / bar_ticks + 1)
            .collect();
        china_bars.dedup();
        assert_eq!(china_bars, vec![2, 4], "2:2 must fire on passes 2 and 4");
    }

    #[test]
    fn test_euclidean_cell_through_assemble() {
        let lib = CellLibrary::new();
        let run = |seed: u64| {
            assemble(&lib, None, Some("euclid_skramz_surge_4_4"), 4, 160.0, (4, 4), Some(0.0), 0.0, 0, seed, 0.0, true)
        };
        let a = run(0);
        let b = run(9);
        assert!(!a.events.is_empty());
        let key = |r: &AssembleResult| -> Vec<(i64, Instrument)> {
            r.events.iter().map(|e| (e.tick, e.instrument)).collect()
        };
        assert_ne!(key(&a), key(&b), "dice_rotate limbs must move across seeds");
        // Anchor limb (kick, dice_rotate false) must not move.
        let kicks = |r: &AssembleResult| -> Vec<i64> {
            r.events.iter().filter(|e| e.instrument == Instrument::Kick).map(|e| e.tick).collect()
        };
        assert_eq!(kicks(&a), kicks(&b), "anchor limbs hold across seeds");
    }

    #[test]
    fn test_arrangement_euclidean_phases() {
        // Regression: the arrangement path used the fixed-cell bar modulo for
        // euclidean cells, replaying local bar 1 forever and discarding the
        // polymeter phasing. Bars of a euclidean verse must not all be equal.
        let lib = CellLibrary::new();
        let r = assemble_arrangement(
            &lib, "faraquet", "4:verse@7/8", 140.0, (4, 4), Some(0.0), 0.0, 5, 0.0, true,
        );
        let bar_ticks = 7 * PPQ / 2;
        let mut bars: std::collections::BTreeMap<i64, Vec<(i64, Instrument)>> =
            std::collections::BTreeMap::new();
        for e in &r.events {
            bars.entry(e.tick / bar_ticks).or_default().push((e.tick % bar_ticks, e.instrument));
        }
        for v in bars.values_mut() {
            v.sort();
        }
        let distinct: std::collections::HashSet<_> = bars.values().collect();
        assert!(distinct.len() > 1, "euclidean phasing must survive arrangement mode");
    }

    #[test]
    fn test_fill_respects_meter() {
        // A 3/4 groove picks only 3/4 fills (they ship now), and no fill hit
        // may land at/past the loop end — the original overflow bug.
        let lib = CellLibrary::new();
        let run = |fill_every: i32| {
            assemble(&lib, None, Some("driving_3_4"), 4, 120.0, (3, 4), Some(0.0), 0.0, fill_every, 42, 0.0, false)
        };
        let a = run(0);
        let b = run(4);
        let key = |r: &AssembleResult| -> Vec<(i64, Instrument, i32)> {
            r.events.iter().map(|e| (e.tick, e.instrument, e.velocity)).collect()
        };
        assert_ne!(key(&a), key(&b), "a meter-matched 3/4 fill must engage");
        let total = 4 * 3 * PPQ;
        assert!(b.events.iter().all(|e| e.tick < total), "no event at/past total_ticks");
    }

    #[test]
    fn test_fill_skipped_on_meter_mismatch() {
        // Fixture: one 3/4 groove, one 4/4-only fill. The mismatched fill
        // must be skipped entirely — output identical to fill-off (the skipped
        // selection also consumes no RNG).
        let json = r#"{
            "cells": {
                "g34": {"name": "g34", "tags": ["waltz"], "time_sig": [3, 4],
                        "num_bars": 1, "humanize": 0.0, "role": "groove", "type": "fixed",
                        "hits": [[1, 0.0, "kick", "accent"], [2, 0.0, "snare", "accent"],
                                 [3, 0.0, "kick", "normal"]]},
                "f44": {"name": "f44", "tags": ["fill", "waltz"], "time_sig": [4, 4],
                        "num_bars": 1, "humanize": 0.0, "role": "fill", "type": "fixed",
                        "hits": [[4, 0.0, "tom_floor", "accent"], [4, 0.5, "tom_floor", "accent"]]}
            },
            "style_pools": {"waltz": ["g34"]},
            "section_preferences": {}
        }"#;
        let lib = CellLibrary::from_json(json);
        let run = |fill_every: i32| {
            assemble(&lib, None, Some("g34"), 4, 120.0, (3, 4), Some(0.0), 0.0, fill_every, 42, 0.0, false)
        };
        let key = |r: &AssembleResult| -> Vec<(i64, Instrument, i32)> {
            r.events.iter().map(|e| (e.tick, e.instrument, e.velocity)).collect()
        };
        assert_eq!(key(&run(0)), key(&run(4)), "4/4-only fill must be skipped for a 3/4 groove");
    }

    #[test]
    fn test_assemble_deterministic() {
        let lib = CellLibrary::new();
        let r1 = assemble(&lib, Some("screamo"), None, 4, 120.0, (4, 4), None, 0.0, 0, 42, 0.0, false);
        let r2 = assemble(&lib, Some("screamo"), None, 4, 120.0, (4, 4), None, 0.0, 0, 42, 0.0, false);
        assert_eq!(r1.events.len(), r2.events.len(), "Same seed should produce same event count");
        for (a, b) in r1.events.iter().zip(r2.events.iter()) {
            assert_eq!(a.tick, b.tick);
            assert_eq!(a.velocity, b.velocity);
        }
    }

    #[test]
    fn test_parse_arrangement() {
        let sections = parse_arrangement("4:verse 8:drive@7/8 2:blast", (4, 4));
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].bars, 4);
        assert_eq!(sections[0].section_type, "verse");
        assert_eq!(sections[1].time_sig, (7, 8));
        assert_eq!(sections[2].section_type, "blast");
    }

    #[test]
    fn test_assemble_arrangement() {
        let lib = CellLibrary::new();
        let result = assemble_arrangement(
            &lib, "screamo", "4:verse 4:blast", 120.0, (4, 4),
            None, 0.0, 42, 0.0, false,
        );
        assert!(!result.events.is_empty());
        assert_eq!(result.total_bars, 8);
    }

    /// A forced home meter wider than any cell the style owns must not leave
    /// the tail of every bar silent. unwound has no 6/4 cell, so before the
    /// meter adapter every 6/4 bar died after beat 4 — "the arc enters
    /// silence" — the fallback 4/4 cell simply had no beats 5-6. The adapter
    /// vamps the figure's head to fill the bar.
    #[test]
    fn forced_wide_meter_fills_the_whole_bar() {
        let lib = CellLibrary::new();
        let arr = "2:intro 4:verse 3:chorus 2:outro";
        let res = assemble_arrangement(&lib, "unwound", arr, 140.0, (6, 4), Some(0.0), 0.0, 5, 0.25, true);
        let bar_ticks = 6 * PPQ; // 6/4
        let mut starved = Vec::new();
        for bar in 0..11i64 {
            let (lo, hi) = (bar * bar_ticks, (bar + 1) * bar_ticks);
            let max_beat = res
                .events
                .iter()
                .filter(|e| e.tick >= lo && e.tick < hi)
                .map(|e| (e.tick - lo) / PPQ)
                .max();
            match max_beat {
                Some(m) if m <= 3 => starved.push(bar + 1),
                None => starved.push(bar + 1),
                _ => {}
            }
        }
        assert!(
            starved.is_empty(),
            "6/4 bars with nothing past beat 4: {starved:?}"
        );
    }

    /// The GUI labels the viewed bar from this, so it must line up with the
    /// sections one-for-one — including silence, which contributes a bar span
    /// but no cell. An off-by-one here relabels every section after a silence.
    #[test]
    fn section_cells_line_up_with_the_sections() {
        let lib = CellLibrary::new();
        // Two silences, one of them not last, plus a fill.
        let arr = "2:intro 1:silence 3:blast 1:fill 2:silence 2:outro";
        let res = assemble_arrangement(&lib, "unwound", arr, 140.0, (4, 4), Some(0.4), 0.0, 9, 0.25, true);
        let sections = parse_arrangement(arr, (4, 4));

        assert_eq!(res.section_cells.len(), sections.len(), "one entry per section");
        for (sec, name) in sections.iter().zip(res.section_cells.iter()) {
            if sec.section_type == "silence" {
                assert!(name.is_empty(), "silence names no cell, got '{name}'");
            } else {
                assert!(!name.is_empty(), "{} resolved to nothing", sec.section_type);
            }
        }
        // unwound owns no blast cell; the label must therefore NOT claim one.
        let blast_idx = sections.iter().position(|s| s.section_type == "blast").unwrap();
        let played = &res.section_cells[blast_idx];
        assert!(
            !lib.get_cell(played).map_or(false, |c| c.has_tag("blast")),
            "unwound has no blast cell, so '{played}' should not be tagged blast"
        );
    }

    /// The cross-engine golden vector: Python is the reference engine, and on
    /// the least-random configuration available (fixed-hit cell, humanize 0,
    /// swing 0, vary 0, no fill) the Rust port must reproduce its output.
    ///
    /// Tick, instrument and MIDI note are fully deterministic and pinned
    /// exactly. Velocity is NOT: `humanize_velocity` floors scaled_variance at
    /// 3, so both engines draw `randint(center-3, center+3)` even at
    /// humanize=0, and Mersenne Twister vs ChaCha8 cannot agree. Two draws in
    /// the same 7-wide band differ by at most 6 — anything beyond that is a
    /// real divergence (wrong velocity level, wrong contour, wrong section
    /// offset), which is what this bound catches.
    ///
    /// Fixture generated by `python export_golden.py`.
    #[test]
    fn golden_vector_matches_python_engine() {
        #[derive(serde::Deserialize)]
        struct Golden {
            params: GoldenParams,
            events: Vec<(i64, u8, i32, String)>,
        }
        #[derive(serde::Deserialize)]
        struct GoldenParams {
            cell_name: String,
            bars: i32,
            tempo: f64,
            seed: u64,
        }

        let golden: Golden =
            serde_json::from_str(include_str!("../../fixtures/golden_vector.json"))
                .expect("golden_vector.json parses");

        let lib = CellLibrary::new();
        let res = assemble(
            &lib, None, Some(&golden.params.cell_name), golden.params.bars,
            golden.params.tempo, (4, 4), Some(0.0), 0.0, 0, golden.params.seed, 0.0, false,
        );

        let ours: Vec<(i64, u8, i32, String)> = res
            .events
            .iter()
            .map(|e| (e.tick, e.instrument.midi_note(), e.velocity, e.instrument.as_str().to_string()))
            .collect();

        assert_eq!(
            ours.len(),
            golden.events.len(),
            "event count diverged from the Python engine"
        );
        // Rerun export_golden.py ONLY if the reference engine changed on purpose.
        for (i, (got, want)) in ours.iter().zip(golden.events.iter()).enumerate() {
            assert_eq!(
                (got.0, got.1, &got.3),
                (want.0, want.1, &want.3),
                "event {i}: tick/note/instrument diverged from the Python engine"
            );
            assert!(
                (got.2 - want.2).abs() <= 6,
                "event {i} ({}): velocity {} is outside the ±6 same-band window around Python's {}",
                got.3, got.2, want.2
            );
        }
    }

    /// A real drummer has two hands and two feet. At humanize=0 (no timing
    /// jitter to blur coincidences) no assembled pattern may put two different
    /// cymbals, or a snare and a tom, on the same tick. Mirrors
    /// validate_midi.check_physical_constraints across every shipped style:
    /// conflict == two instruments of DIFFERENT priority in the same group,
    /// hihat_pedal exempt (it is a foot), auto-crash tick excluded.
    #[test]
    fn physical_constraints_hold_across_styles() {
        use std::collections::{BTreeMap, BTreeSet};
        let lib = CellLibrary::new();
        let styles: Vec<String> = lib.style_names().to_vec();
        let mut violations = Vec::new();

        for style in &styles {
            for bars in [4, 8] {
                let res = assemble(
                    &lib, Some(style), None, bars, 140.0, (0, 0), Some(0.0), 0.0, 0, 7, 0.0, true,
                );
                // The bar-1 auto-crash is added after constraint validation, so
                // it may legitimately share beat 1 with the cell's own cymbal.
                let crash_tick = res.events.iter().map(|e| e.tick).min().unwrap_or(0);

                let mut by_tick: BTreeMap<i64, BTreeSet<Instrument>> = BTreeMap::new();
                for e in &res.events {
                    by_tick.entry(e.tick).or_default().insert(e.instrument);
                }
                for (tick, insts) in by_tick {
                    if tick == crash_tick {
                        continue;
                    }
                    let cymbal_prios: BTreeSet<i32> = insts
                        .iter()
                        .filter(|i| i.is_cymbal() && **i != Instrument::HihatPedal)
                        .map(|i| i.cymbal_priority())
                        .collect();
                    if cymbal_prios.len() > 1 {
                        violations.push(format!(
                            "{style} bars={bars} tick {tick}: cymbal conflict {insts:?}"
                        ));
                    }
                    let stick_prios: BTreeSet<i32> = insts
                        .iter()
                        .filter(|i| i.is_stick())
                        .map(|i| i.stick_priority())
                        .collect();
                    if stick_prios.len() > 1 {
                        violations.push(format!(
                            "{style} bars={bars} tick {tick}: stick conflict {insts:?}"
                        ));
                    }
                }
            }
        }

        assert!(
            violations.is_empty(),
            "{} physical-constraint violations:\n{}",
            violations.len(),
            violations.join("\n")
        );
    }

    /// The Rust engine hardcodes its instrument→MIDI-note map; the Python side
    /// reads kit_mappings/ugritone.json. They must not drift, or the plugin and
    /// the CLI would drive different pads on the same kit.
    #[test]
    fn midi_notes_match_the_ugritone_kit() {
        #[derive(serde::Deserialize)]
        struct Kit {
            mapping: std::collections::BTreeMap<String, u8>,
        }
        let kit: Kit =
            serde_json::from_str(include_str!("../../../kit_mappings/ugritone.json"))
                .expect("ugritone.json parses");

        let mut mismatches = Vec::new();
        for inst in Instrument::ALL {
            match kit.mapping.get(inst.as_str()) {
                Some(&note) if note == inst.midi_note() => {}
                Some(&note) => mismatches.push(format!(
                    "{}: kit {} != engine {}",
                    inst.as_str(),
                    note,
                    inst.midi_note()
                )),
                None => mismatches.push(format!("{}: absent from the kit", inst.as_str())),
            }
        }
        assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
        assert_eq!(
            kit.mapping.len(),
            Instrument::ALL.len(),
            "kit has instruments the engine cannot emit"
        );
    }
}
