import random

from cell_library import get_cell, get_fill_cells, get_pool, get_cell_for_section, STYLE_MAP
from humanizer import Humanizer
from midi_engine import position_to_ticks, DEFAULT_PPQ


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


def vary_hits(hits, cell_bar, vary_amount, rng):
    """Mutate a copy of normalized 5-tuple hits for a given cell_bar."""
    mutated = list(hits)
    bar_hits = [(i, h) for i, h in enumerate(mutated) if h[0] == cell_bar]
    occupied = {(h[1], h[2], h[3]) for _, h in bar_hits}  # (beat, sub, instrument)

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
            if 1 <= new_beat <= 4 and (new_beat, new_sub, "kick") not in occupied:
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
             humanize=None, swing=0.0, fill_every=0, seed=None, vary=0.0):
    if seed is None:
        seed = random.randint(0, 2**31 - 1)

    # Resolve cell
    if cell_name:
        cell = get_cell(cell_name)
    elif style:
        style_lower = style.lower()
        if style_lower in STYLE_MAP:
            cell = get_cell(STYLE_MAP[style_lower])
        else:
            raise ValueError(
                f"Unknown style: '{style}'. Available: {', '.join(sorted(STYLE_MAP.keys()))}"
            )
    else:
        raise ValueError("Must provide --style or --cell")

    num, den = [int(x) for x in time_sig.split("/")]
    time_signatures = [{"bar_start": 1, "bar_end": bars + 10, "numerator": num, "denominator": den}]

    humanize_amount = humanize if humanize is not None else cell["humanize"]
    humanizer = Humanizer(humanize_amount, seed=seed)
    rng = random.Random(seed)

    ppq = DEFAULT_PPQ
    beat_ticks = ppq * 4 // den

    fill_cell = None
    if fill_every > 0:
        fill_cells = get_fill_cells()
        if fill_cells:
            fill_cell = fill_cells[0]

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
        else:
            active_hits = cell_hits
            active_cell = cell

        cell_bar = (bar_idx % active_cell["num_bars"]) + 1

        # Apply vary mutations on repeated cell_bars
        if vary > 0 and cell_bar in seen_cell_bars:
            active_hits = vary_hits(active_hits, cell_bar, vary, rng)
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


def parse_arrangement(arrangement_str):
    """Parse '4:build 8:drive 2:blast 1:silence' → [(4, 'build'), (8, 'drive'), ...]"""
    sections = []
    for token in arrangement_str.strip().split():
        if ":" not in token:
            raise ValueError(f"Invalid arrangement token '{token}' — expected N:section_type")
        count_str, section_type = token.split(":", 1)
        try:
            count = int(count_str)
        except ValueError:
            raise ValueError(f"Invalid bar count '{count_str}' in token '{token}'")
        if count < 1:
            raise ValueError(f"Bar count must be >= 1, got {count} in '{token}'")
        sections.append((count, section_type.lower()))
    if not sections:
        raise ValueError("Arrangement string is empty")
    return sections


# Section types that get a crash+kick on beat 1
_INTENSE_SECTIONS = {"chorus", "blast", "breakdown", "drive"}


def assemble_arrangement(style, arrangement_str, tempo=120, time_sig="4/4",
                         humanize=None, swing=0.0, seed=None, vary=0.0):
    if seed is None:
        seed = random.randint(0, 2**31 - 1)

    sections = parse_arrangement(arrangement_str)
    pool = get_pool(style)

    num, den = [int(x) for x in time_sig.split("/")]
    total_bars = sum(count for count, _ in sections)
    time_signatures = [{"bar_start": 1, "bar_end": total_bars + 10, "numerator": num, "denominator": den}]

    ppq = DEFAULT_PPQ
    beat_ticks = ppq * 4 // den

    humanizer = Humanizer(humanize if humanize is not None else 0.5, seed=seed)
    rng = random.Random(seed)

    events = []
    bar_cursor = 0  # 0-indexed global bar counter

    for section_bars, section_type in sections:
        cell = get_cell_for_section(pool, section_type)

        if cell is None:
            # Silence section — advance bar counter, emit nothing
            bar_cursor += section_bars
            continue

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
            cell_bar = (i % cell["num_bars"]) + 1

            vel_offset = _drift_offset(i, section_bars, drift_dir)

            current_hits = cell_hits
            if vary > 0 and cell_bar in seen_cell_bars:
                current_hits = vary_hits(cell_hits, cell_bar, vary, rng)
            seen_cell_bars.add(cell_bar)

            bar_events = _process_bar(
                bar_number, cell_bar, current_hits, cell, humanizer,
                tempo, time_signatures, ppq, beat_ticks, swing, humanize,
                velocity_offset=vel_offset,
            )
            events.extend(bar_events)

        bar_cursor += section_bars

    events.sort(key=lambda e: (e[0], e[1]))

    section_summary = " → ".join(
        f"{count}×{stype}" + (f"({get_cell_for_section(pool, stype)['name']})" if get_cell_for_section(pool, stype) else "(silence)")
        for count, stype in sections
    )

    return {
        "events": events,
        "tempo": tempo,
        "time_signatures": time_signatures,
        "seed": seed,
        "total_bars": total_bars,
        "section_summary": section_summary,
    }
