import random

import sys

from cell_library import get_cell, get_fill_cells, get_pool, get_cell_for_section, STYLE_MAP, STYLE_POOLS, CELLS
from humanizer import Humanizer
from midi_engine import position_to_ticks, DEFAULT_PPQ


# ── Layer mode constants ──────────────────────────────────────────────────────

LAYER_GROUPS = {
    "kick":   {"kick"},
    "snare":  {"snare", "snare_rim", "snare_ghost"},
    "cymbal": {"hihat_closed", "hihat_open", "hihat_pedal", "ride", "ride_bell",
               "crash_1", "crash_2", "china", "splash"},
    "toms":   {"tom_high", "tom_mid", "tom_low", "tom_floor"},
}

# ── Cymbal/stick priority for constraint resolution ──────────────────────────

_CYMBAL_PRIORITY = {"crash_1": 3, "crash_2": 3, "china": 3, "splash": 2,
                    "ride": 1, "ride_bell": 1, "hihat_closed": 0, "hihat_open": 0, "hihat_pedal": 0}
_STICK_PRIORITY = {"snare": 2, "snare_rim": 2, "snare_ghost": 1,
                   "tom_high": 0, "tom_mid": 0, "tom_low": 0, "tom_floor": 0}
_VEL_RANK = {"accent": 3, "normal": 2, "soft": 1, "ghost": 0}


_DRIFT_DIRECTIONS = {
    "verse": "up", "chorus": "up", "drive": "up", "build": "up", "intro": "up",
    "blast": "none", "breakdown": "none", "atmospheric": "none",
    "silence": "none", "fill": "none",
    "outro": "down",
}


def _drift_offset(bar_index, total_bars, direction):
    if direction == "none" or total_bars <= 1:
        return 0
    center = total_bars / 2
    if direction == "up":
        return int((bar_index - center) * 1.5)
    elif direction == "down":
        return int((center - bar_index) * 1.5)
    return 0


def vary_hits(hits, cell_bar, vary_amount, rng, time_sig=(4, 4)):
    """Mutate a copy of normalized 5-tuple hits for a given cell_bar."""
    mutated = list(hits)
    bar_hits = [(i, h) for i, h in enumerate(mutated) if h[0] == cell_bar]
    occupied = {(h[1], h[2], h[3]) for _, h in bar_hits}  # (beat, sub, instrument)

    max_beats = time_sig[0]

    # Ghost note add (p = vary * 0.5)
    for idx, h in bar_hits:
        if h[3] == "snare" and h[4] == "accent" and rng.random() < vary_amount * 0.5:
            new_sub = h[2] + 0.25
            if new_sub < 1.0 and (h[1], new_sub, "snare_ghost") not in occupied:
                mutated.append((h[0], h[1], new_sub, "snare_ghost", "ghost"))
                occupied.add((h[1], new_sub, "snare_ghost"))

    # Kick displacement (p = vary * 0.3)
    for idx, h in list(bar_hits):
        if h[3] == "kick" and not (h[1] == 1 and h[2] == 0.0) and rng.random() < vary_amount * 0.3:
            shift = rng.choice([-0.25, 0.25])
            new_sub = h[2] + shift
            new_beat = h[1]
            if new_sub >= 1.0:
                new_sub -= 1.0
                new_beat += 1
            elif new_sub < 0.0:
                new_sub += 1.0
                new_beat -= 1
            if 1 <= new_beat <= max_beats and (new_beat, new_sub, "kick") not in occupied:
                occupied.discard((h[1], h[2], "kick"))
                mutated[idx] = (h[0], new_beat, new_sub, "kick", h[4])
                occupied.add((new_beat, new_sub, "kick"))

    # Hi-hat open/close swap (p = vary * 0.2)
    hh_indices = [(idx, h) for idx, h in bar_hits if h[3] == "hihat_closed"]
    if hh_indices and rng.random() < vary_amount * 0.2:
        swap_idx, swap_h = rng.choice(hh_indices)
        mutated[swap_idx] = (swap_h[0], swap_h[1], swap_h[2], "hihat_open", swap_h[4])

    # Ride accent shift (p = vary * 0.15)
    ride_indices = [(idx, h) for idx, h in bar_hits if h[3] == "ride"]
    if ride_indices and rng.random() < vary_amount * 0.15:
        swap_idx, swap_h = rng.choice(ride_indices)
        new_vel = "accent" if swap_h[4] == "normal" else "normal"
        mutated[swap_idx] = (swap_h[0], swap_h[1], swap_h[2], swap_h[3], new_vel)

    return mutated


def _normalize_hits(cell):
    """Ensure all hits are 5-tuples (bar, beat, sub, inst, vel_level)."""
    normalized = []
    for hit in cell["hits"]:
        if len(hit) == 4:
            normalized.append((1, hit[0], hit[1], hit[2], hit[3]))
        else:
            normalized.append(hit)
    return normalized


def _get_humanize_for_bar(cell, cell_bar, default_amount):
    """Look up per-bar humanize override from cell, or return default."""
    per_bar = cell.get("humanize_per_bar")
    if not per_bar:
        return default_amount
    for (start, end), amount in per_bar.items():
        if start <= cell_bar <= end:
            return amount
    return default_amount


def _normalize_grid(prob_cell):
    """Convert 5-tuple grid entries to 6-tuples for single-bar cells."""
    normalized = []
    for entry in prob_cell["grid"]:
        if len(entry) == 5:
            # (beat, sub, inst, prob, vel) -> (1, beat, sub, inst, prob, vel)
            normalized.append((1, entry[0], entry[1], entry[2], entry[3], entry[4]))
        else:
            normalized.append(entry)
    return normalized


def _validate_physical_constraints(bar_hits):
    """Filter hits at each position for limb conflicts.

    At each (beat, sub) position:
    - Right hand (cymbals): keep highest priority (crash > ride > hihat)
    - Left hand: keep highest priority (snare > tom)
    - No ride+hihat at same position
    """
    from collections import defaultdict

    positions = defaultdict(list)
    for hit in bar_hits:
        bar, beat, sub, inst, vel = hit
        positions[(beat, sub)].append(hit)

    filtered = []
    for pos, hits in positions.items():
        cymbals = [(h, _CYMBAL_PRIORITY.get(h[3], -1)) for h in hits if h[3] in _CYMBAL_PRIORITY]
        sticks = [(h, _STICK_PRIORITY.get(h[3], -1)) for h in hits if h[3] in _STICK_PRIORITY]
        feet = [h for h in hits if h[3] == "kick" or h[3] == "hihat_pedal"]

        # Keep highest-priority cymbal only
        if cymbals:
            best_prio = max(p for _, p in cymbals)
            best_cymbals = [h for h, p in cymbals if p == best_prio]
            filtered.append(best_cymbals[0])

        # Keep highest-priority stick hit only
        if sticks:
            best_prio = max(p for _, p in sticks)
            best_sticks = [h for h, p in sticks if p == best_prio]
            filtered.append(best_sticks[0])

        # Keep all foot hits (kick, hihat_pedal can coexist)
        filtered.extend(feet)

    return filtered


def realize_probability_grid(prob_cell, bars, rng):
    """Realize a probability grid cell into concrete 5-tuple hits.

    For each output bar, rolls RNG for each grid entry. Returns hits in the
    same format as _normalize_hits() output: list of (bar, beat, sub, inst, vel).
    """
    grid = _normalize_grid(prob_cell)
    cell_num_bars = prob_cell["num_bars"]
    all_hits = []

    for bar_idx in range(bars):
        output_bar = bar_idx + 1
        cell_bar = (bar_idx % cell_num_bars) + 1

        bar_hits = []
        for g_bar, beat, sub, inst, prob, vel in grid:
            if g_bar != cell_bar:
                continue
            if rng.random() < prob:
                bar_hits.append((output_bar, beat, sub, inst, vel))

        # Validate physical constraints per bar
        bar_hits = _validate_physical_constraints(bar_hits)
        all_hits.extend(bar_hits)

    return all_hits


def extract_layer(cell_hits, layer_name):
    """Filter normalized 5-tuple hits to only those in LAYER_GROUPS[layer_name]."""
    instruments = LAYER_GROUPS[layer_name]
    return [h for h in cell_hits if h[3] in instruments]


def _resolve_layer_conflicts(merged_hits):
    """Resolve conflicts in merged layer hits.

    At each (bar, beat, sub): cymbal priority, stick priority, keep higher velocity
    for same instrument.
    """
    from collections import defaultdict

    positions = defaultdict(list)
    for hit in merged_hits:
        bar, beat, sub, inst, vel = hit
        positions[(bar, beat, sub)].append(hit)

    filtered = []
    for pos, hits in positions.items():
        # Group by instrument — keep highest velocity per instrument
        by_inst = defaultdict(list)
        for h in hits:
            by_inst[h[3]].append(h)

        deduped = []
        for inst, inst_hits in by_inst.items():
            if len(inst_hits) > 1:
                best = max(inst_hits, key=lambda h: _VEL_RANK.get(h[4], 0))
                deduped.append(best)
            else:
                deduped.append(inst_hits[0])

        # Now apply physical constraints
        cymbals = [(h, _CYMBAL_PRIORITY.get(h[3], -1)) for h in deduped if h[3] in _CYMBAL_PRIORITY]
        sticks = [(h, _STICK_PRIORITY.get(h[3], -1)) for h in deduped if h[3] in _STICK_PRIORITY]
        feet = [h for h in deduped if h[3] == "kick" or h[3] == "hihat_pedal"]

        if cymbals:
            best_prio = max(p for _, p in cymbals)
            best = [h for h, p in cymbals if p == best_prio]
            filtered.append(best[0])
        if sticks:
            best_prio = max(p for _, p in sticks)
            best = [h for h, p in sticks if p == best_prio]
            filtered.append(best[0])
        filtered.extend(feet)

    return filtered


def _process_bar(bar_number, cell_bar, active_hits, active_cell, humanizer,
                 tempo, time_sig_list, ppq, beat_ticks, swing, humanize_override,
                 velocity_offset=0):
    """Process one bar of hits, returning list of (abs_tick, instrument, velocity)."""
    events = []
    # Determine humanize amount for this bar
    if humanize_override is not None:
        h_amount = humanize_override
    else:
        h_amount = _get_humanize_for_bar(active_cell, cell_bar, active_cell["humanize"])

    saved = humanizer.humanize_amount
    humanizer.humanize_amount = h_amount

    for hit_bar, beat, sub, instrument, vel_level in active_hits:
        if hit_bar != cell_bar:
            continue

        abs_tick = position_to_ticks(bar_number, beat, sub, time_sig_list, ppq)

        if swing > 0:
            is_upbeat = abs(sub - 0.5) < 0.01
            abs_tick = humanizer.apply_swing(abs_tick, is_upbeat, swing, beat_ticks)

        abs_tick = humanizer.humanize_timing(abs_tick, instrument, tempo, ppq)
        velocity = humanizer.humanize_velocity(vel_level, instrument)
        velocity = max(1, min(127, velocity + velocity_offset))
        events.append((abs_tick, instrument, velocity))

    humanizer.humanize_amount = saved
    return events


def assemble(style=None, cell_name=None, bars=4, tempo=120, time_sig="4/4",
             humanize=None, swing=0.0, fill_every=0, seed=None, vary=0.0,
             generative=False):
    if seed is None:
        seed = random.randint(0, 2**31 - 1)

    num, den = [int(x) for x in time_sig.split("/")]
    requested_ts = (num, den)

    # Resolve cell
    if cell_name:
        cell = get_cell(cell_name)
        cell_ts = tuple(cell.get("time_sig", (4, 4)))
        if cell_ts != requested_ts:
            print(f"Warning: cell '{cell['name']}' is {cell_ts[0]}/{cell_ts[1]} "
                  f"but requested {num}/{den} — no matching cell available",
                  file=sys.stderr)
    elif style:
        style_lower = style.lower()
        if style_lower in STYLE_POOLS:
            pool = get_pool(style_lower)
            # In generative mode, prefer probability cells
            if generative:
                prob_match = [c for c in pool
                              if c.get("type") == "probability" and tuple(c["time_sig"]) == requested_ts]
                if prob_match:
                    pool = prob_match
                else:
                    print(f"Warning: no probability cell for style '{style}' in {num}/{den} — using fixed cell",
                          file=sys.stderr)
            ts_match = [c for c in pool if tuple(c["time_sig"]) == requested_ts]
            if ts_match:
                cell = ts_match[0]
            else:
                cell = pool[0]
                cell_ts = tuple(cell.get("time_sig", (4, 4)))
                if cell_ts != requested_ts:
                    print(f"Warning: no {num}/{den} cell for style '{style}' — "
                          f"using {cell_ts[0]}/{cell_ts[1]} cell '{cell['name']}'",
                          file=sys.stderr)
        elif style_lower in STYLE_MAP:
            cell = get_cell(STYLE_MAP[style_lower])
        else:
            raise ValueError(
                f"Unknown style: '{style}'. Available: {', '.join(sorted(STYLE_POOLS.keys()))}"
            )
    else:
        raise ValueError("Must provide --style or --cell")
    time_signatures = [{"bar_start": 1, "bar_end": bars + 10, "numerator": num, "denominator": den}]

    humanize_amount = humanize if humanize is not None else cell["humanize"]
    humanizer = Humanizer(humanize_amount, seed=seed)
    rng = random.Random(seed)

    ppq = DEFAULT_PPQ
    beat_ticks = ppq * 4 // den

    # Handle probability cell
    is_prob = cell.get("type") == "probability"

    fill_cell = None
    if fill_every > 0:
        fill_cells = get_fill_cells()
        if fill_cells:
            fill_cell = fill_cells[0]

    if is_prob:
        cell_hits = realize_probability_grid(cell, bars, rng)
    else:
        cell_hits = _normalize_hits(cell)
    fill_hits = _normalize_hits(fill_cell) if fill_cell else []

    events = []
    seen_cell_bars = set()

    for bar_idx in range(bars):
        bar_number = bar_idx + 1

        is_fill = fill_every > 0 and fill_cell and (bar_number % fill_every == 0)

        if is_fill:
            active_hits = fill_hits
            active_cell = fill_cell
            cell_bar = (bar_idx % active_cell["num_bars"]) + 1
        elif is_prob:
            # Probability hits already have correct output bar numbers
            active_hits = cell_hits
            active_cell = cell
            cell_bar = bar_number  # hits are keyed by output bar
        else:
            active_hits = cell_hits
            active_cell = cell
            cell_bar = (bar_idx % active_cell["num_bars"]) + 1

        # Apply vary mutations on repeated cell_bars (skip for probability cells)
        if not is_prob and vary > 0 and cell_bar in seen_cell_bars:
            active_hits = vary_hits(active_hits, cell_bar, vary, rng, time_sig=(num, den))
        seen_cell_bars.add(cell_bar)

        bar_events = _process_bar(
            bar_number, cell_bar, active_hits, active_cell, humanizer,
            tempo, time_signatures, ppq, beat_ticks, swing, humanize,
        )
        events.extend(bar_events)

    # Add crash on bar 1 beat 1 if not already present
    has_crash_bar1 = any(
        inst.startswith("crash") and tick < beat_ticks
        for tick, inst, _ in events
    )
    if not has_crash_bar1 and bars > 0:
        crash_tick = position_to_ticks(1, 1, 0.0, time_signatures, ppq)
        crash_tick = humanizer.humanize_timing(crash_tick, "crash_1", tempo, ppq)
        crash_vel = humanizer.humanize_velocity("accent", "crash_1")
        events.append((crash_tick, "crash_1", crash_vel))

    events.sort(key=lambda e: (e[0], e[1]))

    return {
        "events": events,
        "tempo": tempo,
        "time_signatures": time_signatures,
        "seed": seed,
    }


def parse_arrangement(arrangement_str, default_time_sig="4/4"):
    """Parse '4:build 8:drive@7/8 2:blast' → [(4, 'build', (4,4)), (8, 'drive', (7,8)), ...]

    Supports @N/M suffix for per-section time signatures.
    """
    default_num, default_den = [int(x) for x in default_time_sig.split("/")]
    sections = []
    for token in arrangement_str.strip().split():
        if ":" not in token:
            raise ValueError(f"Invalid arrangement token '{token}' — expected N:section_type")
        count_str, rest = token.split(":", 1)
        try:
            count = int(count_str)
        except ValueError:
            raise ValueError(f"Invalid bar count '{count_str}' in token '{token}'")
        if count < 1:
            raise ValueError(f"Bar count must be >= 1, got {count} in '{token}'")

        # Parse optional @N/M time sig suffix
        if "@" in rest:
            section_type, ts_str = rest.split("@", 1)
            try:
                ts_num, ts_den = [int(x) for x in ts_str.split("/")]
            except ValueError:
                raise ValueError(f"Invalid time signature '@{ts_str}' in token '{token}'")
            sections.append((count, section_type.lower(), (ts_num, ts_den)))
        else:
            sections.append((count, rest.lower(), (default_num, default_den)))
    if not sections:
        raise ValueError("Arrangement string is empty")
    return sections


def _consolidate_time_signatures(time_signatures):
    """Merge adjacent time signature entries with the same numerator/denominator."""
    if not time_signatures:
        return time_signatures
    consolidated = [time_signatures[0].copy()]
    for ts in time_signatures[1:]:
        prev = consolidated[-1]
        if ts["numerator"] == prev["numerator"] and ts["denominator"] == prev["denominator"]:
            prev["bar_end"] = ts["bar_end"]
        else:
            consolidated.append(ts.copy())
    return consolidated


# Section types that get a crash+kick on beat 1
_INTENSE_SECTIONS = {"chorus", "blast", "breakdown", "drive"}


def assemble_arrangement(style, arrangement_str, tempo=120, time_sig="4/4",
                         humanize=None, swing=0.0, seed=None, vary=0.0,
                         generative=False):
    if seed is None:
        seed = random.randint(0, 2**31 - 1)

    sections = parse_arrangement(arrangement_str, default_time_sig=time_sig)
    pool = get_pool(style)

    default_num, default_den = [int(x) for x in time_sig.split("/")]
    total_bars = sum(count for count, _, _ in sections)

    # Build per-section time signatures
    time_signatures = []
    bar_cursor_ts = 0
    for section_bars, section_type, (sec_num, sec_den) in sections:
        time_signatures.append({
            "bar_start": bar_cursor_ts + 1,
            "bar_end": bar_cursor_ts + section_bars,
            "numerator": sec_num,
            "denominator": sec_den,
        })
        bar_cursor_ts += section_bars
    time_signatures = _consolidate_time_signatures(time_signatures)

    ppq = DEFAULT_PPQ

    humanizer = Humanizer(humanize if humanize is not None else 0.5, seed=seed)
    rng = random.Random(seed)

    events = []
    bar_cursor = 0  # 0-indexed global bar counter

    for section_bars, section_type, (sec_num, sec_den) in sections:
        beat_ticks = ppq * 4 // sec_den

        # In generative mode, prefer probability cells
        section_pool = pool
        if generative:
            prob_match = [c for c in pool
                          if c.get("type") == "probability" and tuple(c["time_sig"]) == (sec_num, sec_den)]
            if prob_match:
                section_pool = prob_match

        cell = get_cell_for_section(section_pool, section_type, requested_time_sig=(sec_num, sec_den))

        if cell is None:
            # Silence section — advance bar counter, emit nothing
            bar_cursor += section_bars
            continue

        if tuple(cell.get("time_sig", (4, 4))) != (sec_num, sec_den):
            print(f"Warning: section '{section_type}' using {cell['time_sig'][0]}/{cell['time_sig'][1]} "
                  f"cell '{cell['name']}' for {sec_num}/{sec_den} — no matching cell", file=sys.stderr)

        is_prob = cell.get("type") == "probability"

        if is_prob:
            cell_hits = realize_probability_grid(cell, section_bars, rng)
        else:
            cell_hits = _normalize_hits(cell)
        cell_humanize = humanize if humanize is not None else cell["humanize"]
        humanizer.humanize_amount = cell_humanize

        # Crash+kick on beat 1 of intense sections
        section_start_bar = bar_cursor + 1
        if section_type in _INTENSE_SECTIONS:
            crash_tick = position_to_ticks(section_start_bar, 1, 0.0, time_signatures, ppq)
            crash_tick_h = humanizer.humanize_timing(crash_tick, "crash_1", tempo, ppq)
            crash_vel = humanizer.humanize_velocity("accent", "crash_1")
            events.append((crash_tick_h, "crash_1", crash_vel))
            kick_tick = humanizer.humanize_timing(
                position_to_ticks(section_start_bar, 1, 0.0, time_signatures, ppq),
                "kick", tempo, ppq
            )
            kick_vel = humanizer.humanize_velocity("accent", "kick")
            events.append((kick_tick, "kick", kick_vel))

        drift_dir = _DRIFT_DIRECTIONS.get(section_type, "none")
        seen_cell_bars = set()

        for i in range(section_bars):
            bar_number = bar_cursor + i + 1  # 1-indexed global

            if is_prob:
                cell_bar = bar_number  # prob hits already use output bar numbers (1..section_bars)
                # But realize_probability_grid used bar_idx 0..section_bars-1, output_bar = bar_idx+1
                # We need to remap: the realized hits have output_bar = i+1 (1-indexed within section)
                cell_bar = i + 1
            else:
                cell_bar = (i % cell["num_bars"]) + 1

            vel_offset = _drift_offset(i, section_bars, drift_dir)

            current_hits = cell_hits
            if not is_prob and vary > 0 and cell_bar in seen_cell_bars:
                current_hits = vary_hits(cell_hits, cell_bar, vary, rng, time_sig=(sec_num, sec_den))
            seen_cell_bars.add(cell_bar)

            # For prob cells, remap cell_bar hits to the correct global bar_number
            if is_prob:
                remapped_hits = []
                for h in current_hits:
                    if h[0] == cell_bar:
                        remapped_hits.append((bar_number, h[1], h[2], h[3], h[4]))
                bar_events = _process_bar(
                    bar_number, bar_number, remapped_hits, cell, humanizer,
                    tempo, time_signatures, ppq, beat_ticks, swing, humanize,
                    velocity_offset=vel_offset,
                )
            else:
                bar_events = _process_bar(
                    bar_number, cell_bar, current_hits, cell, humanizer,
                    tempo, time_signatures, ppq, beat_ticks, swing, humanize,
                    velocity_offset=vel_offset,
                )
            events.extend(bar_events)

        bar_cursor += section_bars

    events.sort(key=lambda e: (e[0], e[1]))

    section_summary = " → ".join(
        f"{count}×{stype}" + (f"@{sn}/{sd}" if (sn, sd) != (default_num, default_den) else "")
        + (f"({get_cell_for_section(pool, stype, requested_time_sig=(sn, sd))['name']})" if get_cell_for_section(pool, stype, requested_time_sig=(sn, sd)) else "(silence)")
        for count, stype, (sn, sd) in sections
    )

    return {
        "events": events,
        "tempo": tempo,
        "time_signatures": time_signatures,
        "seed": seed,
        "total_bars": total_bars,
        "section_summary": section_summary,
    }


def assemble_layered(layers, bars=4, tempo=120, time_sig="4/4",
                     humanize=None, swing=0.0, vary=0.0, seed=None):
    """Assemble a pattern by mixing instrument layers from different cells.

    Args:
        layers: dict like {"kick": "cell_name", "snare": "cell_name", ...}
        bars: number of output bars
        tempo: BPM
        time_sig: time signature string
        humanize: humanization amount (None = min of source cells)
        swing: swing amount
        vary: variation amount
        seed: random seed
    """
    if seed is None:
        seed = random.randint(0, 2**31 - 1)

    num, den = [int(x) for x in time_sig.split("/")]
    time_signatures = [{"bar_start": 1, "bar_end": bars + 10, "numerator": num, "denominator": den}]

    ppq = DEFAULT_PPQ
    beat_ticks = ppq * 4 // den
    rng = random.Random(seed)

    # Load and normalize all layer cells
    layer_cells = {}
    humanize_values = []
    for layer_name, cell_name in layers.items():
        if layer_name not in LAYER_GROUPS:
            raise ValueError(f"Unknown layer '{layer_name}'. Available: {', '.join(LAYER_GROUPS.keys())}")
        cell = get_cell(cell_name)
        layer_cells[layer_name] = cell
        humanize_values.append(cell["humanize"])

    if humanize is None:
        humanize_amount = min(humanize_values) if humanize_values else 0.5
    else:
        humanize_amount = humanize

    humanizer = Humanizer(humanize_amount, seed=seed)

    # Build a dummy cell for _process_bar (it only reads humanize and humanize_per_bar)
    dummy_cell = {"humanize": humanize_amount, "humanize_per_bar": None, "num_bars": 1}

    events = []

    for bar_idx in range(bars):
        bar_number = bar_idx + 1
        merged_hits = []

        for layer_name, cell in layer_cells.items():
            is_prob = cell.get("type") == "probability"
            cell_bar = (bar_idx % cell["num_bars"]) + 1

            if is_prob:
                # Realize just this one bar
                realized = realize_probability_grid(cell, cell["num_bars"], rng)
                layer_hits = [h for h in realized if h[0] == cell_bar]
            else:
                layer_hits = [h for h in _normalize_hits(cell) if h[0] == cell_bar]

            # Extract only this layer's instruments
            layer_hits = extract_layer(layer_hits, layer_name)

            # Remap cell_bar to output bar_number
            for h in layer_hits:
                merged_hits.append((bar_number, h[1], h[2], h[3], h[4]))

        # Resolve conflicts
        merged_hits = _resolve_layer_conflicts(merged_hits)

        # Process bar
        bar_events = _process_bar(
            bar_number, bar_number, merged_hits, dummy_cell, humanizer,
            tempo, time_signatures, ppq, beat_ticks, swing, humanize,
        )
        events.extend(bar_events)

    events.sort(key=lambda e: (e[0], e[1]))

    return {
        "events": events,
        "tempo": tempo,
        "time_signatures": time_signatures,
        "seed": seed,
    }
