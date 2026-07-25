import random

import sys

from cell_library import get_cell, get_fill_cells, get_pool, get_cell_for_section, STYLE_MAP, STYLE_POOLS, CELLS
from humanizer import Humanizer, get_cluster_amount, infer_section_type
from midi_engine import position_to_ticks, DEFAULT_PPQ


# ── Layer mode constants ──────────────────────────────────────────────────────

LAYER_GROUPS = {
    "kick":   {"kick"},
    "snare":  {"snare", "snare_rim", "snare_ghost"},
    "cymbal": {"hihat_closed", "hihat_open", "hihat_wide_open", "hihat_pedal",
               "ride", "ride_bell", "ride_crash",
               "crash_1", "crash_2", "crash_1_choke", "crash_2_choke",
               "china", "china_2", "splash", "fx_cymbal_1", "fx_cymbal_2"},
    "toms":   {"tom_high", "tom_mid_high", "tom_mid", "tom_low", "tom_floor"},
}

# ── Cymbal/stick priority for constraint resolution ──────────────────────────

_CYMBAL_PRIORITY = {"crash_1": 3, "crash_2": 3, "crash_1_choke": 3, "crash_2_choke": 3,
                    "china": 3, "china_2": 3, "splash": 2,
                    "ride": 1, "ride_bell": 1, "ride_crash": 2,
                    "fx_cymbal_1": 2, "fx_cymbal_2": 2,
                    "hihat_closed": 0, "hihat_open": 0, "hihat_wide_open": 0, "hihat_pedal": 0}
_STICK_PRIORITY = {"snare": 2, "snare_rim": 2, "snare_ghost": 1,
                   "tom_high": 0, "tom_mid_high": 0, "tom_mid": 0, "tom_low": 0, "tom_floor": 0}
_VEL_RANK = {"accent": 3, "normal": 2, "soft": 1, "ghost": 0}


# Per-section dynamics: (vel_base, vel_slope_per_bar, tension_mult).
# vel_base shifts every hit's velocity for the section; vel_slope ramps it per
# bar (a build rises ~15 velocity across 8 bars); tension_mult scales the
# probability-grid tension envelope so loud sections also run DENSER, not just
# harder. Supersedes the old up/down/none drift.
SECTION_DYNAMICS = {
    "intro":       (-6.0, 0.5, 0.9),
    "atmospheric": (-14.0, 0.0, 0.85),
    "verse":       (-3.0, 0.6, 1.0),
    "build":       (-12.0, 2.2, 1.05),
    "chorus":      (5.0, 0.4, 1.08),
    "drive":       (3.0, 0.5, 1.05),
    "blast":       (7.0, 0.0, 1.1),
    "breakdown":   (7.0, -0.8, 0.92),
    "outro":       (-2.0, -2.0, 0.9),
    "fill":        (4.0, 0.0, 1.0),
}


def _section_vel_offset(section_type, bar_index):
    """Velocity offset for a bar within a section: base + slope * bar."""
    base, slope, _ = SECTION_DYNAMICS.get(section_type, (0.0, 0.0, 1.0))
    off = int(base + slope * bar_index)
    return max(-20, min(20, off))


def _section_tension(section_type):
    return SECTION_DYNAMICS.get(section_type, (0.0, 0.0, 1.0))[2]


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
    """Normalize grid entries to 7-tuples (bar, beat, sub, inst, prob, vel, cond).

    Accepted authoring forms:
      5-tuple (beat, sub, inst, prob, vel)             — single-bar, no condition
      6-tuple (bar, beat, sub, inst, prob, vel)        — multi-bar, no condition
      6-tuple (beat, sub, inst, prob, vel, cond)       — single-bar + condition
      7-tuple (bar, beat, sub, inst, prob, vel, cond)  — multi-bar + condition
    The two 6-tuple forms are disambiguated by position 2: a string there is an
    instrument (condition form), a number is a sub position (bar form).
    """
    normalized = []
    for entry in prob_cell["grid"]:
        if len(entry) == 5:
            normalized.append((1, entry[0], entry[1], entry[2], entry[3], entry[4], ""))
        elif len(entry) == 6:
            if isinstance(entry[2], str):
                # (beat, sub, inst, prob, vel, cond)
                normalized.append((1, entry[0], entry[1], entry[2], entry[3], entry[4], entry[5]))
            else:
                normalized.append((*entry, ""))
        else:
            normalized.append(tuple(entry))
    return normalized


# ── Stage 1: shaped randomness ──────────────────────────────────────────────
# Trig conditions (Elektron-style) as an optional trailing string on a grid
# entry. Evaluated BEFORE the probability roll, so a failed condition consumes
# no RNG (cells without conditions keep their exact per-seed streams).
#   "A:B"  fire on pass A of every B cycles of the cell (1-indexed)
#   "1st"  first pass only          "last"  final pass of the baked pattern
#   "pre"  previous entry in this bar fired    "!pre"  it did not
def _trig_allows(cond, pass_num, total_passes, prev_fired):
    if not cond:
        return True
    if cond == "1st":
        return pass_num == 1
    if cond == "last":
        return pass_num == total_passes
    if cond == "pre":
        return prev_fired
    if cond == "!pre":
        return not prev_fired
    if ":" in cond:
        a, b = cond.split(":", 1)
        try:
            a, b = int(a), int(b)
        except ValueError:
            return True  # malformed ratio: fail open (mirrors Rust)
        if b <= 0:
            return True
        return (pass_num - 1) % b == (a - 1) % b
    return True  # unknown condition: fail open


# Syncopation guard rails (Witek et al. 2014: groove pleasure peaks at MEDIUM
# syncopation). If a bar's kick/snare offbeat ratio falls outside the band the
# bar is re-rolled once; the second roll is kept regardless (bounded work,
# deterministic per seed).
SYNC_LO = 0.10
SYNC_HI = 0.60
# Tension envelope: mid-band probabilities ramp across the pattern so later
# bars run slightly hotter — a shaped arc instead of a flat texture.
TENSION_START = 0.88
TENSION_END = 1.10


def _syncopation_ok(bar_hits):
    core = [(h[1], h[2]) for h in bar_hits if h[3] in ("kick", "snare")]
    if len(core) < 3:
        return True
    off = sum(1 for _, sub in core if sub != 0.0) / len(core)
    return SYNC_LO <= off <= SYNC_HI


def _roll_bar(grid, cell_bar, output_bar, pass_num, total_passes, tension, rng):
    """One realization attempt for one bar of a probability grid."""
    bar_hits = []
    prev_fired = False
    for g_bar, beat, sub, inst, prob, vel, cond in grid:
        if g_bar != cell_bar:
            continue
        if not _trig_allows(cond, pass_num, total_passes, prev_fired):
            prev_fired = False
            continue
        p = prob * tension if 0.15 < prob < 0.85 else prob
        fired = rng.random() < min(1.0, p)
        prev_fired = fired
        if fired:
            bar_hits.append((output_bar, beat, sub, inst, vel))
    return bar_hits


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


def realize_probability_grid(prob_cell, bars, rng, tension_mult=1.0):
    """Realize a probability grid cell into concrete 5-tuple hits.

    Per bar: trig conditions gate entries by cell pass (memory across the
    baked pattern), a tension envelope ramps mid-band probabilities across the
    bars (scaled by tension_mult — section dynamics run loud sections denser),
    and a syncopation guard re-rolls a bar once when its kick/snare offbeat
    ratio leaves the musical sweet spot. Returns hits in the same format as
    _normalize_hits() output: (bar, beat, sub, inst, vel).
    """
    grid = _normalize_grid(prob_cell)
    cell_num_bars = prob_cell["num_bars"]
    total_passes = (bars + cell_num_bars - 1) // cell_num_bars
    all_hits = []

    for bar_idx in range(bars):
        output_bar = bar_idx + 1
        cell_bar = (bar_idx % cell_num_bars) + 1
        pass_num = bar_idx // cell_num_bars + 1
        tension = tension_mult * (TENSION_START + (TENSION_END - TENSION_START) * (
            bar_idx / max(1, bars - 1)
        ))

        bar_hits = _roll_bar(grid, cell_bar, output_bar, pass_num, total_passes, tension, rng)
        if not _syncopation_ok(bar_hits):
            bar_hits = _roll_bar(grid, cell_bar, output_bar, pass_num, total_passes, tension, rng)

        bar_hits = _validate_physical_constraints(bar_hits)
        all_hits.extend(bar_hits)

    return all_hits


def _euclid_pattern(pulses, steps):
    """Euclidean rhythm, downbeat-anchored: slot i is an onset iff
    (i * pulses) mod steps < pulses. Equivalent to Bjorklund's algorithm up to
    rotation, with slot 0 always an onset (E(3,8) -> x..x..x.)."""
    return [(i * pulses) % steps < pulses for i in range(steps)]


def realize_euclidean(cell, bars, seed):
    """Realize a Euclidean cell's per-limb patterns over the whole output.

    Each limb tiles its own `steps`-slot cycle across the pattern with NO bar
    reset (polymeter): limbs with co-prime lengths phase against each other
    and take many bars to realign. `dice_rotate` limbs (default true) add a
    seed-derived rotation so the dice re-voices the cell; anchor limbs
    (dice_rotate false) stay put. Slots are sixteenths: 4 per beat in /4
    meters, 2 per (eighth-)beat in /8. Deterministic — no RNG stream used.
    """
    num, den = cell["time_sig"]
    slots_per_beat = 2 if den == 8 else 4
    spb = num * slots_per_beat
    hits = []
    for li, limb in enumerate(cell["limbs"]):
        steps = max(1, limb["steps"])  # clamp like the Rust loader
        pattern = _euclid_pattern(limb["pulses"], steps)
        rot = limb.get("rotation", 0)
        if limb.get("dice_rotate", True):
            rot += (seed >> li) % steps
        vel = limb.get("velocity", "normal")
        inst = limb["instrument"]
        for g in range(bars * spb):
            if pattern[(g + rot) % steps]:
                bar = g // spb + 1
                within = g % spb
                beat = within // slots_per_beat + 1
                sub = (within % slots_per_beat) * (0.5 if den == 8 else 0.25)
                hits.append((bar, beat, sub, inst, vel))

    all_hits = []
    for bar_number in range(1, bars + 1):
        bar_hits = [h for h in hits if h[0] == bar_number]
        all_hits.extend(_validate_physical_constraints(bar_hits))
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
                 velocity_offset=0, section_drift_ms=0.0):
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

        # Section drift: shift all hits by drift amount
        if section_drift_ms != 0.0:
            ms_per_tick = (60000.0 / tempo) / ppq
            abs_tick = max(0, abs_tick + int(section_drift_ms / ms_per_tick))

        velocity = humanizer.humanize_velocity(vel_level, instrument)
        # Velocity contour: wrist pattern + beat-1 emphasis for cymbals
        velocity = humanizer.velocity_contour(velocity, instrument, beat, sub)
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
            # In generative mode, prefer per-seed-varying cells (prob + euclidean)
            if generative:
                prob_match = [c for c in pool
                              if c.get("type") in ("probability", "euclidean")
                              and tuple(c["time_sig"]) == requested_ts]
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
            from cell_library import _suggest_match
            available = sorted(STYLE_POOLS.keys())
            suggestions = _suggest_match(style.lower(), available)
            hint = f" Did you mean: {', '.join(suggestions)}?" if suggestions else ""
            raise ValueError(
                f"Unknown style: '{style}'.{hint}\n"
                f"Available styles: {', '.join(available)}\n"
                f"Run 'python drumgen.py --list-cells' to see all styles and cells."
            )
    else:
        raise ValueError(
            "No style or cell specified. Use --style or --cell to pick a pattern.\n"
            f"Available styles: {', '.join(sorted(STYLE_POOLS.keys()))}\n"
            "Run 'python drumgen.py --list-cells' for full details."
        )
    time_signatures = [{"bar_start": 1, "bar_end": bars, "numerator": num, "denominator": den}]

    humanize_amount = humanize if humanize is not None else cell["humanize"]
    humanizer = Humanizer(humanize_amount, seed=seed)
    rng = random.Random(seed)

    ppq = DEFAULT_PPQ
    beat_ticks = ppq * 4 // den

    # Handle generative cell types
    is_prob = cell.get("type") == "probability"
    is_euclid = cell.get("type") == "euclidean"

    # Fill candidates: meter-matched (a 4/4 fill dropped into a 3/4 bar pushes
    # its beat-4 hits past the bar end), tag-scored against the groove. The
    # actual fill is drawn PER FILL BAR inside the loop so consecutive fill
    # bars alternate instead of repeating one choice; when the top-scored set
    # is a single cell, fall back to all meter-matched fills for variety.
    fill_candidates = []
    if fill_every > 0:
        fill_cells = [f for f in get_fill_cells()
                      if tuple(f.get("time_sig", (4, 4))) == (num, den)]
        if fill_cells:
            cell_tags = set(cell.get("tags", []))
            scored = [(len(cell_tags & set(f.get("tags", []))), f) for f in fill_cells]
            best_score = max(s for s, _ in scored)
            top_fills = [f for s, f in scored if s == best_score]
            fill_candidates = top_fills if len(top_fills) > 1 else fill_cells

    if is_prob:
        cell_hits = realize_probability_grid(cell, bars, rng)
    elif is_euclid:
        cell_hits = realize_euclidean(cell, bars, seed)
    else:
        cell_hits = _normalize_hits(cell)

    events = []
    seen_cell_bars = set()
    section_type = infer_section_type(cell)
    last_fill = None

    for bar_idx in range(bars):
        bar_number = bar_idx + 1

        is_fill = fill_every > 0 and fill_candidates and (bar_number % fill_every == 0)

        if is_fill:
            fill_cell = fill_candidates[rng.randrange(len(fill_candidates))]
            # Avoid the same fill twice in a row when there is a choice.
            if len(fill_candidates) > 1 and fill_cell is last_fill:
                fill_cell = fill_candidates[rng.randrange(len(fill_candidates))]
            last_fill = fill_cell
            active_hits = _normalize_hits(fill_cell)
            active_cell = fill_cell
            cell_bar = (bar_idx % active_cell["num_bars"]) + 1
        elif is_prob or is_euclid:
            # Realized hits already carry correct output bar numbers
            active_hits = cell_hits
            active_cell = cell
            cell_bar = bar_number  # hits are keyed by output bar
        else:
            active_hits = cell_hits
            active_cell = cell
            cell_bar = (bar_idx % active_cell["num_bars"]) + 1

        # Apply vary mutations on repeated cell_bars (skip for generative cells
        # — prob re-realizes per seed and euclidean phasing never repeats)
        if not (is_prob or is_euclid) and vary > 0 and cell_bar in seen_cell_bars:
            active_hits = vary_hits(active_hits, cell_bar, vary, rng, time_sig=(num, den))
        seen_cell_bars.add(cell_bar)

        drift_ms = humanizer.compute_section_drift_ms(section_type, bar_idx, bars)
        bar_events = _process_bar(
            bar_number, cell_bar, active_hits, active_cell, humanizer,
            tempo, time_signatures, ppq, beat_ticks, swing, humanize,
            section_drift_ms=drift_ms,
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

    events = humanizer.apply_flam(events, tempo, ppq)
    cluster_amt = get_cluster_amount(cell)
    events = humanizer.apply_ghost_clustering(events, cluster_amt, tempo, ppq)
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

    humanizer = Humanizer(humanize if humanize is not None else 0.3, seed=seed)
    rng = random.Random(seed)

    events = []
    bar_cursor = 0  # 0-indexed global bar counter
    used_cells = []

    for sec_idx, (section_bars, section_type, (sec_num, sec_den)) in enumerate(sections):
        beat_ticks = ppq * 4 // sec_den
        # The upcoming section steers fill choice (into_* tags).
        next_section = sections[sec_idx + 1][1] if sec_idx + 1 < len(sections) else None

        # Score the WHOLE pool against the section, then prefer a per-seed
        # varying cell only among equal scorers.
        #
        # This used to narrow the pool to probability/euclidean cells BEFORE
        # scoring, which subordinated section intent to generativity: most
        # styles own one or two grids, so build, blast and breakdown all
        # collapsed onto the same cell and the fixed blast cell was
        # unreachable. The plugin's telegraph announced "BLAST NOW" over the
        # same groove as the verse, eight bars louder. blast_traditional
        # scores 7 for a blast section against prob_screamo_4_4's 3 — let the
        # score say so.
        cell = get_cell_for_section(pool, section_type, requested_time_sig=(sec_num, sec_den),
                                    rng=rng, next_section=next_section,
                                    prefer_generative=generative)

        if cell is None:
            # Silence section — advance bar counter, emit nothing
            used_cells.append(None)
            bar_cursor += section_bars
            continue

        used_cells.append(cell)

        if tuple(cell.get("time_sig", (4, 4))) != (sec_num, sec_den):
            print(f"Warning: section '{section_type}' using {cell['time_sig'][0]}/{cell['time_sig'][1]} "
                  f"cell '{cell['name']}' for {sec_num}/{sec_den} — no matching cell", file=sys.stderr)

        is_prob = cell.get("type") == "probability"
        is_euclid = cell.get("type") == "euclidean"

        if is_prob:
            cell_hits = realize_probability_grid(cell, section_bars, rng,
                                                 tension_mult=_section_tension(section_type))
        elif is_euclid:
            cell_hits = realize_euclidean(cell, section_bars, seed)
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

        seen_cell_bars = set()

        for i in range(section_bars):
            bar_number = bar_cursor + i + 1  # 1-indexed global

            if is_prob or is_euclid:
                # Realized hits (prob AND euclidean) are keyed by output bar
                # within the section: 1..section_bars. Using the fixed-cell
                # modulo here would discard euclidean phasing (every bar would
                # replay local bar 1).
                cell_bar = i + 1
            else:
                cell_bar = (i % cell["num_bars"]) + 1

            vel_offset = _section_vel_offset(section_type, i)

            current_hits = cell_hits
            if not (is_prob or is_euclid) and vary > 0 and cell_bar in seen_cell_bars:
                current_hits = vary_hits(cell_hits, cell_bar, vary, rng, time_sig=(sec_num, sec_den))
            seen_cell_bars.add(cell_bar)

            drift_ms = humanizer.compute_section_drift_ms(section_type, i, section_bars)

            # For realized cells, remap cell_bar hits to the correct global bar_number
            if is_prob or is_euclid:
                remapped_hits = []
                for h in current_hits:
                    if h[0] == cell_bar:
                        remapped_hits.append((bar_number, h[1], h[2], h[3], h[4]))
                bar_events = _process_bar(
                    bar_number, bar_number, remapped_hits, cell, humanizer,
                    tempo, time_signatures, ppq, beat_ticks, swing, humanize,
                    velocity_offset=vel_offset, section_drift_ms=drift_ms,
                )
            else:
                bar_events = _process_bar(
                    bar_number, cell_bar, current_hits, cell, humanizer,
                    tempo, time_signatures, ppq, beat_ticks, swing, humanize,
                    velocity_offset=vel_offset, section_drift_ms=drift_ms,
                )
            events.extend(bar_events)

        bar_cursor += section_bars

    events = humanizer.apply_flam(events, tempo, ppq)
    cluster_amt = max(
        (get_cluster_amount(c) for c in used_cells if c is not None),
        default=0.3
    )
    events = humanizer.apply_ghost_clustering(events, cluster_amt, tempo, ppq)
    events.sort(key=lambda e: (e[0], e[1]))

    # Report the cells that were ACTUALLY used. This used to re-call
    # get_cell_for_section without the rng, so the summary named different
    # cells than the ones that played whenever a tie was broken randomly.
    section_cells = [c["name"] if c is not None else "" for c in used_cells]
    section_summary = " → ".join(
        f"{count}×{stype}" + (f"@{sn}/{sd}" if (sn, sd) != (default_num, default_den) else "")
        + (f"({name})" if name else "(silence)")
        for (count, stype, (sn, sd)), name in zip(sections, section_cells)
    )

    return {
        "events": events,
        "tempo": tempo,
        "time_signatures": time_signatures,
        "seed": seed,
        "total_bars": total_bars,
        "section_summary": section_summary,
        # Cell name per section in order ("" for silence) — what PLAYS, as
        # opposed to what the section type asked for. Mirrors the Rust
        # AssembleResult.section_cells the plugin GUI reads.
        "section_cells": section_cells,
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
    time_signatures = [{"bar_start": 1, "bar_end": bars, "numerator": num, "denominator": den}]

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
        humanize_amount = min(humanize_values) if humanize_values else 0.3
    else:
        humanize_amount = humanize

    humanizer = Humanizer(humanize_amount, seed=seed)

    # Build a dummy cell for _process_bar (it only reads humanize and humanize_per_bar)
    dummy_cell = {"humanize": humanize_amount, "humanize_per_bar": None, "num_bars": 1}

    events = []
    section_type = "verse"

    for bar_idx in range(bars):
        bar_number = bar_idx + 1
        merged_hits = []

        for layer_name, cell in layer_cells.items():
            is_prob = cell.get("type") == "probability"
            is_euclid = cell.get("type") == "euclidean"
            cell_bar = (bar_idx % cell["num_bars"]) + 1

            if is_prob:
                # Realize just this one bar
                realized = realize_probability_grid(cell, cell["num_bars"], rng)
                layer_hits = [h for h in realized if h[0] == cell_bar]
            elif is_euclid:
                realized = realize_euclidean(cell, cell["num_bars"], seed)
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
        drift_ms = humanizer.compute_section_drift_ms(section_type, bar_idx, bars)
        bar_events = _process_bar(
            bar_number, bar_number, merged_hits, dummy_cell, humanizer,
            tempo, time_signatures, ppq, beat_ticks, swing, humanize,
            section_drift_ms=drift_ms,
        )
        events.extend(bar_events)

    events = humanizer.apply_flam(events, tempo, ppq)
    cluster_amt = max(
        (get_cluster_amount(c) for c in layer_cells.values()),
        default=0.0
    )
    events = humanizer.apply_ghost_clustering(events, cluster_amt, tempo, ppq)
    events.sort(key=lambda e: (e[0], e[1]))

    return {
        "events": events,
        "tempo": tempo,
        "time_signatures": time_signatures,
        "seed": seed,
    }
