import random

from cell_library import get_cell, get_fill_cells, STYLE_MAP
from humanizer import Humanizer
from midi_engine import position_to_ticks, DEFAULT_PPQ


def assemble(style=None, cell_name=None, bars=4, tempo=120, time_sig="4/4",
             humanize=None, swing=0.0, fill_every=0, seed=None):
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

    # Parse time signature
    num, den = [int(x) for x in time_sig.split("/")]
    time_signatures = [{"bar_start": 1, "bar_end": bars + 10, "numerator": num, "denominator": den}]

    # Resolve humanize amount
    humanize_amount = humanize if humanize is not None else cell["humanize"]
    humanizer = Humanizer(humanize_amount, seed=seed)

    ppq = DEFAULT_PPQ
    beat_ticks = ppq * 4 // den

    # Resolve fill cell
    fill_cell = None
    if fill_every > 0:
        fill_cells = get_fill_cells()
        if fill_cells:
            fill_cell = fill_cells[0]

    # Normalize hits: ensure all are 5-tuples (bar, beat, sub, inst, vel_level)
    def normalize_hits(c):
        normalized = []
        for hit in c["hits"]:
            if len(hit) == 4:
                normalized.append((1, hit[0], hit[1], hit[2], hit[3]))
            else:
                normalized.append(hit)
        return normalized

    cell_hits = normalize_hits(cell)
    fill_hits = normalize_hits(fill_cell) if fill_cell else []

    events = []

    for bar_idx in range(bars):
        bar_number = bar_idx + 1  # 1-indexed for position_to_ticks

        # Determine if this is a fill bar
        is_fill = fill_every > 0 and fill_cell and (bar_number % fill_every == 0)

        if is_fill:
            active_hits = fill_hits
            active_cell = fill_cell
        else:
            active_hits = cell_hits
            active_cell = cell

        # Which bar within the cell?
        cell_bar = (bar_idx % active_cell["num_bars"]) + 1

        for hit_bar, beat, sub, instrument, vel_level in active_hits:
            if hit_bar != cell_bar:
                continue

            abs_tick = position_to_ticks(bar_number, beat, sub, time_signatures, ppq)

            # Apply swing (upbeat = sub is 0.5 in eighth-note grid)
            if swing > 0:
                is_upbeat = abs(sub - 0.5) < 0.01
                abs_tick = humanizer.apply_swing(abs_tick, is_upbeat, swing, beat_ticks)

            # Humanize
            abs_tick = humanizer.humanize_timing(abs_tick, instrument, tempo, ppq)
            velocity = humanizer.humanize_velocity(vel_level, instrument)

            events.append((abs_tick, instrument, velocity))

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
