# Cell Library — Phase 1 MVP
#
# Each cell is a dict with:
#   name, tags, time_sig, num_bars, humanize, role, hits
#
# Single-bar cells: hits are (beat, sub, instrument, velocity_level) 4-tuples
# Multi-bar cells:  hits are (bar, beat, sub, instrument, velocity_level) 5-tuples
#   where bar is 1-indexed within the cell.
#
# Sub values: 0.0 = on beat, 0.25 = sixteenth, 0.5 = eighth, 0.75 = dotted eighth


def _blast_traditional():
    """Traditional blast beat: K/S alternate every sixteenth, ride every sixteenth."""
    hits = []
    for beat in range(1, 5):
        # Kick on 0.0 and 0.5 (1st and 3rd sixteenth of each beat)
        hits.append((beat, 0.0, "kick", "accent"))
        hits.append((beat, 0.5, "kick", "accent"))
        # Snare on 0.25 and 0.75 (2nd and 4th sixteenth of each beat)
        hits.append((beat, 0.25, "snare", "accent"))
        hits.append((beat, 0.75, "snare", "accent"))
        # Ride on every sixteenth
        hits.append((beat, 0.0, "ride", "accent"))
        hits.append((beat, 0.25, "ride", "normal"))
        hits.append((beat, 0.5, "ride", "accent"))
        hits.append((beat, 0.75, "ride", "normal"))
    return {
        "name": "blast_traditional",
        "tags": ["blast", "extreme", "screamo", "metal"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.8,
        "role": "groove",
        "hits": hits,
    }


def _dbeat_standard():
    """Standard d-beat: X.XX kick pattern, snare on 2/4 upbeats, HH eighths."""
    # K: 1.0, 2.0, 2.5, 3.0, 4.0, 4.5
    # S: 1.5, 3.5 (snare on upbeats of 1 and 3 = backbeat on 2 and 4 in feel)
    # HH: all eighths
    hits = [
        # Kick
        (1, 0.0, "kick", "accent"),
        (2, 0.0, "kick", "normal"),
        (2, 0.5, "kick", "normal"),
        (3, 0.0, "kick", "accent"),
        (4, 0.0, "kick", "normal"),
        (4, 0.5, "kick", "normal"),
        # Snare (backbeat)
        (1, 0.5, "snare", "accent"),
        (3, 0.5, "snare", "accent"),
        # Hi-hat eighths
        (1, 0.0, "hihat_closed", "normal"),
        (1, 0.5, "hihat_closed", "normal"),
        (2, 0.0, "hihat_closed", "normal"),
        (2, 0.5, "hihat_closed", "normal"),
        (3, 0.0, "hihat_closed", "normal"),
        (3, 0.5, "hihat_closed", "normal"),
        (4, 0.0, "hihat_closed", "normal"),
        (4, 0.5, "hihat_closed", "normal"),
    ]
    return {
        "name": "dbeat_standard",
        "tags": ["dbeat", "punk", "crust", "hardcore"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.4,
        "role": "groove",
        "hits": hits,
    }


def _shellac_floor_tom_drive():
    """Shellac floor tom drive: floor tom 1/3, snare 2/4, ride quarters. NO ghost notes."""
    hits = [
        (1, 0.0, "tom_floor", "accent"),
        (3, 0.0, "tom_floor", "accent"),
        (2, 0.0, "snare", "accent"),
        (4, 0.0, "snare", "accent"),
        (1, 0.0, "ride", "normal"),
        (2, 0.0, "ride", "normal"),
        (3, 0.0, "ride", "normal"),
        (4, 0.0, "ride", "normal"),
    ]
    return {
        "name": "shellac_floor_tom_drive",
        "tags": ["shellac", "noise_rock", "sparse", "precise"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.2,
        "role": "groove",
        "hits": hits,
    }


def _fugazi_driving_chorus():
    """Fugazi driving chorus: K on 1, 2+, 3. S on 2, 4. Ride eighths."""
    hits = [
        # Kick: 1, 2.5, 3
        (1, 0.0, "kick", "accent"),
        (2, 0.5, "kick", "accent"),
        (3, 0.0, "kick", "accent"),
        # Snare: 2, 4
        (2, 0.0, "snare", "accent"),
        (4, 0.0, "snare", "accent"),
        # Ride: eighths
        (1, 0.0, "ride", "normal"),
        (1, 0.5, "ride", "normal"),
        (2, 0.0, "ride", "normal"),
        (2, 0.5, "ride", "normal"),
        (3, 0.0, "ride", "normal"),
        (3, 0.5, "ride", "normal"),
        (4, 0.0, "ride", "normal"),
        (4, 0.5, "ride", "normal"),
    ]
    return {
        "name": "fugazi_driving_chorus",
        "tags": ["fugazi", "posthardcore", "driving", "chorus"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.5,
        "role": "groove",
        "hits": hits,
    }


def _faraquet_displaced_4_4():
    """Faraquet displaced backbeat: 2-bar cell. Snare on 2+ and 4, ghost notes, ride eighths."""
    # Bar 1: K 1, 2, 3.5 | S(accent) 2.5, 4 | Ghost snare 1.5, 3.0, 4.5 | Ride eighths
    # Bar 2: displaced kick pattern, shifted accents
    hits = [
        # --- Bar 1 ---
        # Kick
        (1, 1, 0.0, "kick", "accent"),
        (1, 2, 0.0, "kick", "normal"),
        (1, 3, 0.5, "kick", "normal"),
        # Snare accents (displaced backbeat: 2+ and 4)
        (1, 2, 0.5, "snare", "accent"),
        (1, 4, 0.0, "snare", "accent"),
        # Ghost snares
        (1, 1, 0.5, "snare_ghost", "ghost"),
        (1, 3, 0.0, "snare_ghost", "ghost"),
        (1, 4, 0.5, "snare_ghost", "ghost"),
        # Ride eighths
        (1, 1, 0.0, "ride", "normal"),
        (1, 1, 0.5, "ride", "normal"),
        (1, 2, 0.0, "ride", "normal"),
        (1, 2, 0.5, "ride", "normal"),
        (1, 3, 0.0, "ride", "normal"),
        (1, 3, 0.5, "ride", "normal"),
        (1, 4, 0.0, "ride", "normal"),
        (1, 4, 0.5, "ride", "normal"),
        # --- Bar 2 (displaced) ---
        # Kick (shifted pattern)
        (2, 1, 0.5, "kick", "normal"),
        (2, 2, 0.5, "kick", "accent"),
        (2, 4, 0.0, "kick", "normal"),
        # Snare accents (displaced: 2 and 3+)
        (2, 2, 0.0, "snare", "accent"),
        (2, 3, 0.5, "snare", "accent"),
        # Ghost snares
        (2, 1, 0.0, "snare_ghost", "ghost"),
        (2, 3, 0.0, "snare_ghost", "ghost"),
        (2, 4, 0.5, "snare_ghost", "ghost"),
        # Ride eighths
        (2, 1, 0.0, "ride", "normal"),
        (2, 1, 0.5, "ride", "normal"),
        (2, 2, 0.0, "ride", "normal"),
        (2, 2, 0.5, "ride", "normal"),
        (2, 3, 0.0, "ride", "normal"),
        (2, 3, 0.5, "ride", "normal"),
        (2, 4, 0.0, "ride", "normal"),
        (2, 4, 0.5, "ride", "normal"),
    ]
    return {
        "name": "faraquet_displaced_4_4",
        "tags": ["faraquet", "angular", "math", "posthardcore"],
        "time_sig": (4, 4),
        "num_bars": 2,
        "humanize": 0.65,
        "role": "groove",
        "hits": hits,
    }


def _raein_melodic_drive():
    """Raein melodic drive: K 1/3, S 2/4, HH eighths alternating accent/ghost, ghost snares."""
    hits = [
        # Kick
        (1, 0.0, "kick", "accent"),
        (3, 0.0, "kick", "accent"),
        # Snare backbeat
        (2, 0.0, "snare", "accent"),
        (4, 0.0, "snare", "accent"),
        # Ghost snares on 2.5 and 4.5
        (2, 0.5, "snare_ghost", "ghost"),
        (4, 0.5, "snare_ghost", "ghost"),
        # Hi-hat eighths: accent on downbeats, ghost on upbeats
        (1, 0.0, "hihat_closed", "accent"),
        (1, 0.5, "hihat_closed", "ghost"),
        (2, 0.0, "hihat_closed", "accent"),
        (2, 0.5, "hihat_closed", "ghost"),
        (3, 0.0, "hihat_closed", "accent"),
        (3, 0.5, "hihat_closed", "ghost"),
        (4, 0.0, "hihat_closed", "accent"),
        (4, 0.5, "hihat_closed", "ghost"),
    ]
    return {
        "name": "raein_melodic_drive",
        "tags": ["raein", "euro_screamo", "melodic", "groovy"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.6,
        "role": "groove",
        "hits": hits,
    }


def _fill_linear_1bar():
    """Linear fill: single-stroke roll descending through kit on sixteenths. Velocity crescendo."""
    # 16 sixteenths across 4 beats, no cymbals during fill body
    # Pattern from style DNA 10A: S, HiTom, S, MidTom, HiTom, S, MidTom, FlrTom,
    #   K, S, HiTom, MidTom, FlrTom, K, FlrTom, K
    sequence = [
        (1, 0.0, "snare"),
        (1, 0.25, "tom_high"),
        (1, 0.5, "snare"),
        (1, 0.75, "tom_mid"),
        (2, 0.0, "tom_high"),
        (2, 0.25, "snare"),
        (2, 0.5, "tom_mid"),
        (2, 0.75, "tom_floor"),
        (3, 0.0, "kick"),
        (3, 0.25, "snare"),
        (3, 0.5, "tom_high"),
        (3, 0.75, "tom_mid"),
        (4, 0.0, "tom_floor"),
        (4, 0.25, "kick"),
        (4, 0.5, "tom_floor"),
        (4, 0.75, "kick"),
    ]
    hits = []
    for i, (beat, sub, inst) in enumerate(sequence):
        # Velocity crescendo 80 -> 120 across 16 hits
        vel_value = 80 + int((120 - 80) * i / 15)
        # Map to velocity level based on value
        if vel_value < 50:
            vel_level = "ghost"
        elif vel_value < 75:
            vel_level = "soft"
        elif vel_value < 105:
            vel_level = "normal"
        else:
            vel_level = "accent"
        hits.append((beat, sub, inst, vel_level))
    return {
        "name": "fill_linear_1bar",
        "tags": ["fill", "posthardcore", "general"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.4,
        "role": "fill",
        "hits": hits,
    }


# --- Registry ---

CELLS = {cell["name"]: cell for cell in [
    _blast_traditional(),
    _dbeat_standard(),
    _shellac_floor_tom_drive(),
    _fugazi_driving_chorus(),
    _faraquet_displaced_4_4(),
    _raein_melodic_drive(),
    _fill_linear_1bar(),
]}

STYLE_MAP = {
    "blast": "blast_traditional",
    "dbeat": "dbeat_standard",
    "shellac": "shellac_floor_tom_drive",
    "fugazi": "fugazi_driving_chorus",
    "faraquet": "faraquet_displaced_4_4",
    "raein": "raein_melodic_drive",
    "posthardcore": "fugazi_driving_chorus",
    "noise_rock": "shellac_floor_tom_drive",
    "screamo": "blast_traditional",
    "math": "faraquet_displaced_4_4",
    "euro_screamo": "raein_melodic_drive",
}


def get_cell(name):
    if name not in CELLS:
        raise KeyError(f"Unknown cell: '{name}'. Available: {', '.join(sorted(CELLS.keys()))}")
    return CELLS[name]


def get_cells_by_style(style):
    style_lower = style.lower()
    return [c for c in CELLS.values() if style_lower in c["tags"]]


def get_fill_cells():
    return [c for c in CELLS.values() if c["role"] == "fill"]


def list_cells(style_filter=None):
    cells = CELLS.values()
    if style_filter:
        style_lower = style_filter.lower()
        cells = [c for c in cells if style_lower in c["tags"]]
    return list(cells)
