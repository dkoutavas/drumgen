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
}

/// Seed-keyed rotation into a candidate cell list — the "dice" for fixed-cell
/// styles. Deterministic per seed, so every dice press changes the groove.
fn rotate_pick<'a>(list: &[&'a Cell], seed: u64) -> &'a Cell {
    list[(seed as usize) % list.len()]
}

/// Velocity drift direction per section type.
fn drift_direction(section_type: &str) -> &'static str {
    match section_type {
        "verse" | "chorus" | "drive" | "build" | "intro" => "up",
        "outro" => "down",
        _ => "none", // blast, breakdown, atmospheric, silence, fill
    }
}

/// Calculate velocity offset for a bar within a section.
fn drift_offset(bar_index: i32, total_bars: i32, direction: &str) -> i32 {
    if direction == "none" || total_bars <= 1 {
        return 0;
    }
    let center = total_bars as f64 / 2.0;
    match direction {
        "up" => ((bar_index as f64 - center) * 1.5) as i32,
        "down" => ((center - bar_index as f64) * 1.5) as i32,
        _ => 0,
    }
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

/// Realize a probability grid into concrete hits.
fn realize_probability_grid(cell: &Cell, bars: i32, rng: &mut ChaCha8Rng) -> Vec<Hit> {
    let cell_num_bars = cell.num_bars;
    let mut all_hits = Vec::new();

    for bar_idx in 0..bars {
        let output_bar = bar_idx + 1;
        let cell_bar = (bar_idx % cell_num_bars) + 1;

        let mut bar_hits = Vec::new();
        for entry in &cell.grid {
            if entry.bar != cell_bar {
                continue;
            }
            if rng.gen::<f64>() < entry.probability {
                bar_hits.push(Hit {
                    bar: output_bar,
                    beat: entry.beat,
                    sub: entry.sub,
                    instrument: entry.instrument,
                    velocity_level: entry.velocity_level,
                });
            }
        }

        bar_hits = validate_physical_constraints(&bar_hits);
        all_hits.extend(bar_hits);
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

    for hit in active_hits {
        if hit.bar != cell_bar {
            continue;
        }

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

    // Fill selection mirrors Python assemble(): tag-overlap score against the
    // chosen cell, RNG tie-break among the top scorers. This consumes the RNG
    // before grid realization, same as Python's ordering.
    // Fills must match the RESOLVED meter — a 4/4 fill dropped into a 3/4 bar
    // pushes its beat-4 hits past the bar end (hung notes / next-bar doubling).
    // No matching fill = no fill for this pattern, gracefully.
    let fill_cell: Option<&Cell> = if fill_every > 0 {
        let fills: Vec<&Cell> = library
            .get_fill_cells()
            .into_iter()
            .filter(|f| f.time_sig == (num, den))
            .collect();
        if fills.is_empty() {
            None
        } else {
            let scored: Vec<(usize, &Cell)> = fills
                .iter()
                .map(|f| (f.tags.iter().filter(|t| cell.tags.contains(t)).count(), *f))
                .collect();
            let best = scored.iter().map(|(s, _)| *s).max().unwrap_or(0);
            let top: Vec<&Cell> =
                scored.into_iter().filter(|(s, _)| *s == best).map(|(_, c)| c).collect();
            Some(top[rng.gen_range(0..top.len())])
        }
    } else {
        None
    };

    let cell_hits = if is_prob {
        realize_probability_grid(cell, bars, &mut rng)
    } else {
        cell.hits.clone()
    };
    let fill_hits: Vec<Hit> = fill_cell.map(|f| f.hits.clone()).unwrap_or_default();

    let mut events = Vec::new();
    let mut seen_cell_bars = std::collections::HashSet::new();
    let section_type = infer_section_type(&cell.tags);

    for bar_idx in 0..bars {
        let bar_number = bar_idx + 1;

        let is_fill = fill_cell.is_some() && bar_number % fill_every == 0;

        let (mut active_hits, active_cell, cell_bar) = if is_fill {
            let f = fill_cell.unwrap();
            (fill_hits.clone(), f, (bar_idx % f.num_bars) + 1)
        } else if is_prob {
            // Probability hits already carry correct output bar numbers.
            (cell_hits.clone(), cell, bar_number)
        } else {
            (cell_hits.clone(), cell, (bar_idx % cell.num_bars) + 1)
        };

        // Vary mutations on repeated cell bars (skip for probability cells).
        // time_sig for vary is the RESOLVED meter, not the requested one
        // ((0,0) = Auto must never reach vary_hits).
        if !is_prob && vary > 0.0 && seen_cell_bars.contains(&cell_bar) {
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

    for section in &sections {
        let (sec_num, sec_den) = section.time_sig;
        let beat_ticks = ppq * 4 / sec_den as i64;

        // In generative mode, prefer probability cells
        let section_pool: Vec<&Cell> = if generative {
            let prob_match: Vec<&Cell> = pool.iter()
                .filter(|c| c.is_probability() && c.time_sig == (sec_num, sec_den))
                .copied()
                .collect();
            if !prob_match.is_empty() { prob_match } else { pool.clone() }
        } else {
            pool.clone()
        };

        let cell = match library.get_cell_for_section(
            &section_pool, &section.section_type,
            Some((sec_num, sec_den)), &mut rng,
        ) {
            Some(c) => c,
            None => {
                // Silence section
                bar_cursor += section.bars;
                continue;
            }
        };

        let is_prob = cell.is_probability();
        let cell_hits = if is_prob {
            realize_probability_grid(cell, section.bars, &mut rng)
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

        let drift_dir = drift_direction(&section.section_type);
        let mut seen_cell_bars = std::collections::HashSet::new();

        for i in 0..section.bars {
            let bar_number = bar_cursor + i + 1;

            let cell_bar = if is_prob {
                i + 1
            } else {
                (i % cell.num_bars) + 1
            };

            let vel_offset = drift_offset(i, section.bars, drift_dir);

            let mut current_hits = cell_hits.clone();
            if !is_prob && vary > 0.0 && seen_cell_bars.contains(&cell_bar) {
                current_hits = vary_hits(&cell_hits, cell_bar, vary, &mut rng, (sec_num, sec_den));
            }
            seen_cell_bars.insert(cell_bar);

            let drift_ms = humanizer.compute_section_drift_ms(&section.section_type, i, section.bars);

            if is_prob {
                // Remap probability hits to correct global bar number
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
    let cluster_amt = get_cluster_amount(
        &pool.first().map(|c| c.tags.clone()).unwrap_or_default()
    );
    events = humanizer.apply_ghost_clustering(&events, cluster_amt, tempo, ppq);
    events.sort_by(|a, b| a.tick.cmp(&b.tick).then_with(|| a.instrument.as_str().cmp(b.instrument.as_str())));

    AssembleResult {
        events,
        tempo,
        time_signatures,
        seed,
        total_bars,
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

    let humanize_amount = humanize.unwrap_or_else(|| {
        humanize_values.iter().cloned().fold(f64::INFINITY, f64::min).min(0.3)
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
        humanize_per_bar: None,
    };

    let mut events = Vec::new();

    for bar_idx in 0..bars {
        let bar_number = bar_idx + 1;
        let mut merged_hits = Vec::new();

        for (layer_name, cell) in &layer_cells {
            let cell_bar = (bar_idx % cell.num_bars) + 1;

            let layer_hits: Vec<Hit> = if cell.is_probability() {
                let realized = realize_probability_grid(cell, cell.num_bars, &mut rng);
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
    fn test_fill_skipped_on_meter_mismatch() {
        // All shipped fill cells are 4/4. A 3/4 groove must NOT receive one —
        // a 4/4 fill's beat-4 hits would land at/past the 3/4 bar end (tick >=
        // total_ticks on the last bar), producing hung notes and next-bar
        // doubling. With no meter-matched fill the output must be identical to
        // fill-off (the skipped selection also consumes no RNG).
        let lib = CellLibrary::new();
        let run = |fill_every: i32| {
            assemble(&lib, None, Some("driving_3_4"), 4, 120.0, (3, 4), Some(0.0), 0.0, fill_every, 42, 0.0, false)
        };
        let a = run(0);
        let b = run(4);
        let key = |r: &AssembleResult| -> Vec<(i64, Instrument, i32)> {
            r.events.iter().map(|e| (e.tick, e.instrument, e.velocity)).collect()
        };
        assert_eq!(key(&a), key(&b), "3/4 groove must skip the 4/4-only fills entirely");
        // And nothing may sit at/past the loop end regardless.
        let total = 4 * 3 * PPQ;
        assert!(b.events.iter().all(|e| e.tick < total), "no event at/past total_ticks");
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
}
