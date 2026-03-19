#!/usr/bin/env python3
"""Import MIDI files into drumgen's cell format.

Standalone CLI and importable library. Reads .mid via mido, converts to
drumgen's native cell format (flat list of 5-tuples), saves as JSON in
user_cells/.

Usage:
    python midi_reader.py input.mid --name my_cell --tags blast,intense --kit ugritone
    python midi_reader.py input_dir/ --auto-tag
    python midi_reader.py --list
    python midi_reader.py --stats
    python midi_reader.py --validate
    python midi_reader.py --retag
    python midi_reader.py --dedup [--confirm]
"""

import argparse
import hashlib
import json
import math
import os
import sys
from collections import Counter

import mido

from midi_engine import load_kit_mapping, DEFAULT_PPQ, MIDI_CHANNEL

USER_CELLS_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "user_cells")

VALID_VELOCITY_LEVELS = {"ghost", "soft", "normal", "accent"}
VALID_SUBS = {0.0, 0.25, 0.5, 0.75}


def _build_reverse_mapping(kit):
    """Build {midi_note: instrument_name} from kit mapping.

    When multiple instruments share a note (e.g. snare and snare_ghost both
    map to 38), prefer the shorter/primary name so velocity classification
    handles ghost detection.
    """
    mapping = kit["mapping"]
    reverse = {}
    # Sort so shorter names come last and overwrite longer ones
    for name in sorted(mapping.keys(), key=len, reverse=True):
        note = mapping[name]
        reverse[note] = name
    # Explicit preference: snare over snare_ghost
    if "snare" in mapping and "snare_ghost" in mapping:
        if mapping["snare"] == mapping["snare_ghost"]:
            reverse[mapping["snare"]] = "snare"
    # Apply aliases: additional note -> instrument mappings
    # Aliases can be note-number keys ("38": "snare") or name keys
    # ("tom_mid_h": "tom_high") — handle both formats.
    aliases = kit.get("aliases", {})
    for alias_key, target in aliases.items():
        try:
            # Note-number key: map MIDI note directly to instrument
            reverse[int(alias_key)] = target
        except ValueError:
            # Name key: resolve target instrument's MIDI note, then add alias
            if target in mapping:
                reverse[mapping[target]] = target
    return reverse


def _classify_velocity(velocity, velocity_ranges):
    """Map a MIDI velocity (0-127) to a drumgen velocity level."""
    for level in ("accent", "normal", "soft", "ghost"):
        lo, hi = velocity_ranges[level]
        if lo <= velocity <= hi:
            return level
    # Fallback by thresholds if outside all ranges
    if velocity >= 105:
        return "accent"
    if velocity >= 75:
        return "normal"
    if velocity >= 50:
        return "soft"
    return "ghost"


def _quantize_sub(raw_sub):
    """Snap sub-beat fraction to nearest sixteenth: {0.0, 0.25, 0.5, 0.75}.

    Returns (quantized_sub, snap_distance).
    """
    grid = [0.0, 0.25, 0.5, 0.75, 1.0]
    best = min(grid, key=lambda g: abs(raw_sub - g))
    snap = abs(raw_sub - best)
    if best >= 1.0:
        best = 0.0  # wrapped to next beat, caller handles
        snap = abs(raw_sub - 1.0)
    return best, snap


# ── Content hashing ──────────────────────────────────────────────────────────

def _hash_hits(hits):
    """MD5 hash of sorted, serialized hits list. Used for dedup."""
    # Normalize: sort and convert to lists for consistent serialization
    normalized = sorted([list(h) for h in hits])
    data = json.dumps(normalized, sort_keys=True).encode()
    return hashlib.md5(data).hexdigest()


# ── BPM extraction ───────────────────────────────────────────────────────────

def _extract_bpm(mid):
    """Extract BPM from set_tempo meta message. Returns float or None."""
    for track in mid.tracks:
        for msg in track:
            if msg.type == "set_tempo":
                return round(mido.tempo2bpm(msg.tempo), 1)
    return None


# ── Validation ───────────────────────────────────────────────────────────────

def validate_cell(cell, kit_name="ugritone"):
    """Validate a cell dict. Returns (errors, warnings) lists of strings."""
    errors = []
    warnings = []

    hits = cell.get("hits", [])
    time_sig = cell.get("time_sig", (4, 4))
    num_bars = cell.get("num_bars", 1)

    # Min notes
    if len(hits) < 3:
        errors.append(f"Only {len(hits)} notes (min 3 for musical usefulness)")

    # Valid instruments
    try:
        kit = load_kit_mapping(kit_name)
        valid_instruments = set(kit["mapping"].keys())
        for h in hits:
            inst = h[3] if len(h) == 5 else h[2]
            if inst not in valid_instruments:
                errors.append(f"Unknown instrument '{inst}' not in {kit_name} mapping")
                break  # one error is enough
    except Exception:
        warnings.append(f"Could not load kit '{kit_name}' for instrument validation")

    # Valid velocity levels
    for h in hits:
        vel = h[4] if len(h) == 5 else h[3]
        if vel not in VALID_VELOCITY_LEVELS:
            errors.append(f"Invalid velocity level '{vel}'")
            break

    # Time sig denominator must be power of 2
    den = time_sig[1]
    if den < 1 or (den & (den - 1)) != 0:
        errors.append(f"Time sig denominator {den} is not a power of 2")

    # Beat values within time sig range
    ts_num = time_sig[0]
    for h in hits:
        beat = h[1] if len(h) == 5 else h[0]
        if beat < 1 or beat > ts_num:
            warnings.append(f"Beat {beat} outside time sig {ts_num}/{den}")
            break

    # Sub values in valid set
    for h in hits:
        sub = h[2] if len(h) == 5 else h[1]
        if sub not in VALID_SUBS:
            # Allow small deviations (flam offsets like +0.02)
            closest = min(VALID_SUBS, key=lambda s: abs(sub - s))
            if abs(sub - closest) > 0.05:
                warnings.append(f"Sub value {sub} not in standard grid")
                break

    # Must have at least one kick, snare, or tom
    core_instruments = {"kick", "snare", "snare_ghost", "snare_rim",
                        "tom_high", "tom_mid_high", "tom_mid", "tom_low", "tom_floor"}
    instruments_present = set()
    for h in hits:
        inst = h[3] if len(h) == 5 else h[2]
        instruments_present.add(inst)
    if not instruments_present & core_instruments:
        errors.append("No kick, snare, or tom — pure cymbal patterns rejected")

    # Name-based non-drum detection
    cell_name = cell.get("name", "").lower()
    non_drum_patterns = ["synth", "absynth", "sampler", "harmony",
                         "dhg2", "cornflake", "bend_synth", "omni"]
    if any(p in cell_name for p in non_drum_patterns):
        errors.append("Name matches non-drum pattern")

    # Large empty bar gaps
    num_bars_val = max(cell.get("num_bars", 1), 1)
    if num_bars_val > 2:
        bar_hit_counts = Counter()
        for h in hits:
            bar = h[0] if len(h) == 5 else 1
            bar_hit_counts[bar] += 1
        empty_bars = sum(1 for b in range(1, num_bars_val + 1) if bar_hit_counts[b] == 0)
        if empty_bars > num_bars_val * 0.3:
            warnings.append(f"{empty_bars}/{num_bars_val} bars empty — possible multi-section clip")

    return errors, warnings


# ── Auto-tagging ─────────────────────────────────────────────────────────────

def auto_tag_cell(cell):
    """Analyze hit patterns and assign tags from SECTION_PREFERENCES vocabulary.

    Modifies cell in place (tags and role). Always keeps 'imported' tag.
    Returns the cell for chaining.
    """
    hits = cell.get("hits", [])
    time_sig = cell.get("time_sig", (4, 4))
    num_bars = cell.get("num_bars", 1)
    ts_num = time_sig[0]

    tags = {"imported"}

    if not hits:
        cell["tags"] = sorted(tags)
        return cell

    # Count instruments
    inst_counts = Counter()
    vel_counts = Counter()
    for h in hits:
        inst = h[3] if len(h) == 5 else h[2]
        vel = h[4] if len(h) == 5 else h[3]
        inst_counts[inst] += 1
        vel_counts[vel] += 1

    total_hits = len(hits)
    hits_per_bar = total_hits / max(num_bars, 1)

    # ── Density tags ──
    if hits_per_bar > 30:
        tags.update(["intense", "extreme"])
    elif hits_per_bar > 20:
        tags.update(["driving", "intense"])
    elif hits_per_bar >= 10:
        tags.update(["driving", "groovy"])
    elif hits_per_bar < 6:
        tags.update(["sparse", "atmospheric"])

    # ── Blast beat detection ──
    # Rapid alternating kick+snare, high density
    kick_count = inst_counts.get("kick", 0)
    snare_count = inst_counts.get("snare", 0) + inst_counts.get("snare_ghost", 0)
    if (hits_per_bar > 20 and kick_count >= 4 * num_bars
            and snare_count >= 4 * num_bars):
        # Check for alternating pattern (kick and snare on different subdivisions)
        kick_positions = set()
        snare_positions = set()
        for h in hits:
            bar = h[0] if len(h) == 5 else 1
            beat = h[1] if len(h) == 5 else h[0]
            sub = h[2] if len(h) == 5 else h[1]
            inst = h[3] if len(h) == 5 else h[2]
            pos = (bar, beat, sub)
            if inst == "kick":
                kick_positions.add(pos)
            elif inst in ("snare", "snare_ghost"):
                snare_positions.add(pos)
        # If kick and snare rarely share positions, it's blast-like
        overlap = len(kick_positions & snare_positions)
        if overlap < len(kick_positions) * 0.3:
            tags.update(["blast", "intense", "extreme"])

    # ── Ghost notes ──
    ghost_snare = inst_counts.get("snare_ghost", 0)
    if ghost_snare > 0 or vel_counts.get("ghost", 0) > total_hits * 0.1:
        tags.add("groovy")

    # ── Fill pattern ──
    tom_instruments = {"tom_high", "tom_mid_high", "tom_mid", "tom_low", "tom_floor"}
    tom_count = sum(inst_counts.get(t, 0) for t in tom_instruments)
    ride_hh = (inst_counts.get("ride", 0) + inst_counts.get("ride_crash", 0)
               + inst_counts.get("hihat_closed", 0) + inst_counts.get("hihat_open", 0)
               + inst_counts.get("hihat_wide_open", 0))
    if tom_count > total_hits * 0.3 and ride_hh == 0:
        tags.add("fill")
        cell["role"] = "fill"

    # ── Halftime detection ──
    # Snare on beat 3 only (1 snare per bar)
    snare_beats = []
    for h in hits:
        inst = h[3] if len(h) == 5 else h[2]
        if inst == "snare":
            beat = h[1] if len(h) == 5 else h[0]
            sub = h[2] if len(h) == 5 else h[1]
            if sub == 0.0:
                snare_beats.append(beat)
    snares_per_bar = len(snare_beats) / max(num_bars, 1)
    if 0.5 <= snares_per_bar <= 1.5 and snare_beats:
        avg_beat = sum(snare_beats) / len(snare_beats)
        if avg_beat >= 2.5:  # snare on beat 3 or later
            tags.update(["halftime", "heavy", "breakdown"])

    # ── Backbeat detection ──
    # Snare on beats 2 and 4
    snare_on_2 = 0
    snare_on_4 = 0
    for h in hits:
        inst = h[3] if len(h) == 5 else h[2]
        if inst == "snare":
            beat = h[1] if len(h) == 5 else h[0]
            sub = h[2] if len(h) == 5 else h[1]
            if beat == 2 and sub == 0.0:
                snare_on_2 += 1
            elif beat == 4 and sub == 0.0:
                snare_on_4 += 1
    if snare_on_2 >= num_bars * 0.5 and snare_on_4 >= num_bars * 0.5:
        tags.update(["driving", "groovy"])

    # ── Odd meter ──
    if ts_num not in {2, 3, 4}:
        tags.update(["angular", "math"])

    # ── China cymbal ──
    if inst_counts.get("china", 0) > 0 or inst_counts.get("china_2", 0) > 0:
        tags.add("intense")

    # ── Crash accents ──
    crash_count = (inst_counts.get("crash_1", 0) + inst_counts.get("crash_2", 0)
                   + inst_counts.get("crash_1_choke", 0) + inst_counts.get("crash_2_choke", 0))
    if crash_count > 0:
        tags.add("accent")

    cell["tags"] = sorted(tags)
    return cell


# ── Core conversion ──────────────────────────────────────────────────────────

def midi_to_cell(midi_path, name=None, tags=None, kit_name="ugritone",
                 humanize=0.5, role="groove"):
    """Convert a MIDI file to a drumgen cell dict.

    Args:
        midi_path: Path to .mid file.
        name: Cell name. Defaults to filename stem.
        tags: List of tag strings. Defaults to ["imported"].
        kit_name: Kit mapping name or path.
        humanize: Humanization level (0.0-1.0).
        role: Cell role (groove/fill/transition).

    Returns:
        Cell dict in drumgen's native format.
    """
    kit = load_kit_mapping(kit_name)
    reverse_map = _build_reverse_mapping(kit)
    velocity_ranges = kit.get("velocity_ranges", {
        "ghost": [20, 50], "soft": [50, 75],
        "normal": [75, 105], "accent": [105, 127],
    })

    mid = mido.MidiFile(midi_path)
    ppq = mid.ticks_per_beat

    # Extract BPM early so it can be included in auto-generated names
    source_bpm = _extract_bpm(mid)

    if name is None:
        name = os.path.splitext(os.path.basename(midi_path))[0]
        # Sanitize: replace spaces/dashes with underscores
        name = name.replace(" ", "_").replace("-", "_").lower()
        # Append BPM if available (only for auto-generated names)
        if source_bpm is not None:
            name = f"{name}_{int(round(source_bpm))}bpm"

    if tags is None:
        tags = ["imported"]

    # Extract time signature from MIDI meta messages
    time_sig_num, time_sig_den = 4, 4
    for track in mid.tracks:
        for msg in track:
            if msg.type == "time_signature":
                time_sig_num = msg.numerator
                time_sig_den = msg.denominator
                break

    beat_ticks = ppq * 4 // time_sig_den
    bar_ticks = time_sig_num * beat_ticks

    # Collect note-on events, trying channel 9 first
    notes = []
    for track in mid.tracks:
        abs_tick = 0
        for msg in track:
            abs_tick += msg.time
            if msg.type == "note_on" and msg.velocity > 0:
                notes.append((abs_tick, msg.note, msg.velocity, msg.channel))

    # Filter to drum channel
    ch9_notes = [(t, n, v) for t, n, v, ch in notes if ch == MIDI_CHANNEL]
    if ch9_notes:
        raw_notes = ch9_notes
    else:
        if notes:
            print(f"Warning: no notes on channel 10 (idx 9), using all channels",
                  file=sys.stderr)
        raw_notes = [(t, n, v) for t, n, v, ch in notes]

    if not raw_notes:
        raise ValueError(f"No notes found in {midi_path}")

    # Convert to hits
    hits = []
    snap_warnings = 0
    sixteenth_ticks = beat_ticks / 4

    for abs_tick, note, velocity in raw_notes:
        if note not in reverse_map:
            continue

        instrument = reverse_map[note]
        vel_level = _classify_velocity(velocity, velocity_ranges)

        # Handle ghost detection: if snare and ghost velocity, use snare_ghost
        if instrument == "snare" and vel_level == "ghost" and "snare_ghost" in kit["mapping"]:
            instrument = "snare_ghost"

        # Calculate bar, beat, sub
        bar = int(abs_tick // bar_ticks) + 1
        tick_in_bar = abs_tick % bar_ticks
        beat = int(tick_in_bar // beat_ticks) + 1
        tick_in_beat = tick_in_bar % beat_ticks

        raw_sub = tick_in_beat / beat_ticks
        sub, snap = _quantize_sub(raw_sub)

        # Warn if snap is large (>10% of a sixteenth)
        if snap > 0.025:  # 0.25 * 0.10
            snap_warnings += 1

        # Handle wrap to next beat
        if sub == 0.0 and raw_sub > 0.875:
            beat += 1
            if beat > time_sig_num:
                beat = 1
                bar += 1

        hits.append((bar, beat, sub, instrument, vel_level))

    # Deduplicate hits at same position — keep highest velocity
    vel_priority = {"ghost": 0, "soft": 1, "normal": 2, "accent": 3}
    best_hits = {}
    for h in hits:
        key = (h[0], h[1], h[2], h[3])  # (bar, beat, sub, instrument)
        if key not in best_hits or vel_priority.get(h[4], 2) > vel_priority.get(best_hits[key][4], 2):
            best_hits[key] = h
    if len(best_hits) < len(hits):
        print(f"Info: removed {len(hits) - len(best_hits)} duplicate hits", file=sys.stderr)
    hits = list(best_hits.values())

    if snap_warnings > 0:
        print(f"Warning: {snap_warnings} notes snapped by >10% of a sixteenth "
              f"(possible triplet content)", file=sys.stderr)

    # Determine num_bars
    if hits:
        num_bars = max(h[0] for h in hits)
    else:
        num_bars = 1

    # Sort hits by position
    hits.sort(key=lambda h: (h[0], h[1], h[2]))

    # Trim trailing single-hit bar (clip boundary bleed)
    if num_bars > 1:
        last_bar_hits = [h for h in hits if h[0] == num_bars]
        if len(last_bar_hits) <= 1:
            if not last_bar_hits or (last_bar_hits[0][1] == 1 and last_bar_hits[0][2] <= 0.25):
                hits = [h for h in hits if h[0] != num_bars]
                num_bars -= 1

    cell = {
        "name": name,
        "tags": tags,
        "time_sig": (time_sig_num, time_sig_den),
        "num_bars": num_bars,
        "humanize": humanize,
        "role": role,
        "hits": hits,
        "source": "imported",
        "source_file": os.path.basename(midi_path),
        "content_hash": _hash_hits(hits),
    }

    if source_bpm is not None:
        cell["source_bpm"] = source_bpm

    return cell


# ── Save with dedup ──────────────────────────────────────────────────────────

def save_cell(cell, directory=None, dedup=True):
    """Save a cell dict as JSON to user_cells/.

    If dedup=True, checks for existing cells with same content_hash.
    Returns filepath on success, None if skipped as duplicate.
    """
    if directory is None:
        directory = USER_CELLS_DIR
    os.makedirs(directory, exist_ok=True)

    # Dedup check
    if dedup and "content_hash" in cell:
        for existing_file in os.listdir(directory):
            if not existing_file.endswith(".json"):
                continue
            existing_path = os.path.join(directory, existing_file)
            try:
                with open(existing_path) as f:
                    existing = json.load(f)
                if existing.get("content_hash") == cell["content_hash"]:
                    return None  # duplicate
            except (json.JSONDecodeError, KeyError):
                continue

    filepath = os.path.join(directory, f"{cell['name']}.json")
    # Convert tuples to lists for JSON serialization
    serializable = dict(cell)
    serializable["time_sig"] = list(cell["time_sig"])
    serializable["hits"] = [list(h) for h in cell["hits"]]
    with open(filepath, "w") as f:
        json.dump(serializable, f, indent=2)
    return filepath


# ── Listing ──────────────────────────────────────────────────────────────────

def list_user_cells(directory=None):
    """List all user cell JSON files."""
    if directory is None:
        directory = USER_CELLS_DIR
    if not os.path.isdir(directory):
        print("No user_cells/ directory found.")
        return
    files = sorted(f for f in os.listdir(directory) if f.endswith(".json"))
    if not files:
        print("No imported cells found.")
        return
    print(f"Imported cells ({len(files)}):\n")
    for filename in files:
        filepath = os.path.join(directory, filename)
        try:
            with open(filepath) as f:
                cell = json.load(f)
            ts = f"{cell['time_sig'][0]}/{cell['time_sig'][1]}"
            bars = cell["num_bars"]
            tags = ", ".join(cell.get("tags", []))
            src = cell.get("source_file", "?")
            bpm = cell.get("source_bpm")
            bpm_str = f" | {bpm} BPM" if bpm else ""
            print(f"  {cell['name']}")
            print(f"    {ts} | {bars} bar{'s' if bars > 1 else ''}{bpm_str} | tags: {tags}")
            print(f"    source: {src}")
        except (json.JSONDecodeError, KeyError) as e:
            print(f"  {filename} (error: {e})")


# ── Directory import ─────────────────────────────────────────────────────────

def _collect_mid_files(dir_path):
    """Recursively collect .mid/.midi files from a directory."""
    mid_files = []
    for root, dirs, files in os.walk(dir_path):
        for f in sorted(files):
            if f.lower().endswith((".mid", ".midi")):
                mid_files.append(os.path.join(root, f))
    return sorted(mid_files)


def _import_directory(dir_path, auto_tag=False, kit_name="ugritone", force=False):
    """Import all .mid files in a directory (recursively)."""
    mid_files = _collect_mid_files(dir_path)
    if not mid_files:
        print(f"No MIDI files found in {dir_path}")
        return

    imported = 0
    skipped_dup = 0
    skipped_err = 0
    skipped_val = 0

    for filepath in mid_files:
        filename = os.path.basename(filepath)
        try:
            cell = midi_to_cell(filepath, kit_name=kit_name)

            # Auto-tag from content analysis
            if auto_tag:
                auto_tag_cell(cell)

            # Validate
            errors, warnings = validate_cell(cell, kit_name=kit_name)
            if errors and not force:
                for e in errors:
                    print(f"  REJECT {filename}: {e}", file=sys.stderr)
                skipped_val += 1
                continue
            for w in warnings:
                print(f"  WARN {filename}: {w}", file=sys.stderr)

            path = save_cell(cell)
            if path is None:
                skipped_dup += 1
            else:
                imported += 1
                tags_str = ", ".join(cell["tags"])
                bpm_str = f" @ {cell['source_bpm']}bpm" if cell.get("source_bpm") else ""
                print(f"  Imported: {cell['name']} [{tags_str}]{bpm_str}")
        except Exception as e:
            print(f"  ERROR {filename}: {e}", file=sys.stderr)
            skipped_err += 1

    print(f"\nSummary: {imported} imported, {skipped_dup} duplicates skipped, "
          f"{skipped_val} validation failures, {skipped_err} errors")


# ── Maintenance commands ─────────────────────────────────────────────────────

def _load_all_cells(directory=None):
    """Load all cell JSON files from directory. Returns list of (filepath, cell)."""
    if directory is None:
        directory = USER_CELLS_DIR
    if not os.path.isdir(directory):
        return []
    cells = []
    for filename in sorted(os.listdir(directory)):
        if not filename.endswith(".json"):
            continue
        filepath = os.path.join(directory, filename)
        try:
            with open(filepath) as f:
                cell = json.load(f)
            cell["time_sig"] = tuple(cell["time_sig"])
            cell["hits"] = [tuple(h) for h in cell["hits"]]
            cells.append((filepath, cell))
        except (json.JSONDecodeError, KeyError) as e:
            print(f"Warning: skipping {filepath}: {e}", file=sys.stderr)
    return cells


def cmd_dedup(directory=None, confirm=False):
    """Find and optionally remove duplicate cells."""
    cells = _load_all_cells(directory)
    if not cells:
        print("No cells found.")
        return

    # Compute hashes (recompute from hits for cells missing content_hash)
    hash_groups = {}
    for filepath, cell in cells:
        h = cell.get("content_hash") or _hash_hits(cell["hits"])
        hash_groups.setdefault(h, []).append((filepath, cell))

    dups = {h: group for h, group in hash_groups.items() if len(group) > 1}

    if not dups:
        print(f"No duplicates found among {len(cells)} cells.")
        return

    total_removable = sum(len(group) - 1 for group in dups.values())
    print(f"Found {len(dups)} duplicate groups ({total_removable} removable files):\n")

    to_remove = []
    for h, group in sorted(dups.items()):
        keep = group[0]
        remove = group[1:]
        print(f"  Keep: {os.path.basename(keep[0])} ({keep[1]['name']})")
        for filepath, cell in remove:
            print(f"    Remove: {os.path.basename(filepath)} ({cell['name']})")
            to_remove.append(filepath)
        print()

    if not confirm:
        print(f"Dry run — would remove {len(to_remove)} files. Use --confirm to execute.")
    else:
        for fp in to_remove:
            os.remove(fp)
            print(f"  Removed: {os.path.basename(fp)}")
        print(f"\nRemoved {len(to_remove)} duplicate files.")


def cmd_validate(directory=None, kit_name="ugritone"):
    """Validate all existing cells, print report."""
    cells = _load_all_cells(directory)
    if not cells:
        print("No cells found.")
        return

    total_errors = 0
    total_warnings = 0
    clean = 0

    for filepath, cell in cells:
        errors, warnings = validate_cell(cell, kit_name=kit_name)
        if errors or warnings:
            print(f"  {cell.get('name', os.path.basename(filepath))}:")
            for e in errors:
                print(f"    ERROR: {e}")
                total_errors += 1
            for w in warnings:
                print(f"    WARN:  {w}")
                total_warnings += 1
        else:
            clean += 1

    print(f"\nValidation: {clean} clean, {total_errors} errors, {total_warnings} warnings "
          f"across {len(cells)} cells")


def cmd_retag(directory=None):
    """Re-run auto-tagger on all existing cells."""
    cells = _load_all_cells(directory)
    if not cells:
        print("No cells found.")
        return

    updated = 0
    for filepath, cell in cells:
        old_tags = set(cell.get("tags", []))
        auto_tag_cell(cell)
        new_tags = set(cell.get("tags", []))
        if old_tags != new_tags:
            # Save updated cell
            with open(filepath, "w") as f:
                serializable = dict(cell)
                serializable["time_sig"] = list(cell["time_sig"])
                serializable["hits"] = [list(h) for h in cell["hits"]]
                json.dump(serializable, f, indent=2)
            added = new_tags - old_tags
            removed = old_tags - new_tags
            changes = []
            if added:
                changes.append(f"+{','.join(sorted(added))}")
            if removed:
                changes.append(f"-{','.join(sorted(removed))}")
            print(f"  {cell['name']}: {' '.join(changes)}")
            updated += 1

    print(f"\nRetagged: {updated}/{len(cells)} cells updated")


def cmd_stats(directory=None):
    """Print cell statistics."""
    cells = _load_all_cells(directory)
    if not cells:
        print("No cells found.")
        return

    # Basic counts
    total = len(cells)
    hashes = set()
    for _, cell in cells:
        h = cell.get("content_hash") or _hash_hits(cell["hits"])
        hashes.add(h)
    unique = len(hashes)

    # Tag distribution
    tag_counts = Counter()
    for _, cell in cells:
        for tag in cell.get("tags", []):
            tag_counts[tag] += 1

    # Time signatures
    ts_counts = Counter()
    for _, cell in cells:
        ts = cell.get("time_sig", (4, 4))
        ts_counts[f"{ts[0]}/{ts[1]}"] += 1

    # BPM range
    bpms = [cell.get("source_bpm") for _, cell in cells if cell.get("source_bpm")]

    # Bars distribution
    bar_counts = Counter()
    for _, cell in cells:
        bar_counts[cell.get("num_bars", 1)] += 1

    # Hits per bar
    hpb_values = []
    for _, cell in cells:
        nb = max(cell.get("num_bars", 1), 1)
        hpb_values.append(len(cell.get("hits", [])) / nb)

    # Role distribution
    role_counts = Counter()
    for _, cell in cells:
        role_counts[cell.get("role", "groove")] += 1

    print(f"Cell Statistics ({total} cells, {unique} unique patterns):\n")

    print(f"  Time signatures:")
    for ts, count in ts_counts.most_common():
        print(f"    {ts}: {count}")

    if bpms:
        print(f"\n  BPM range: {min(bpms):.0f} - {max(bpms):.0f}")
        bpm_counts = Counter(int(b) for b in bpms)
        print(f"  BPM distribution:")
        for bpm, count in sorted(bpm_counts.items()):
            print(f"    {bpm}: {count}")

    print(f"\n  Bars per cell:")
    for bars, count in sorted(bar_counts.items()):
        print(f"    {bars}: {count}")

    print(f"\n  Hits/bar: {min(hpb_values):.0f} - {max(hpb_values):.0f} "
          f"(avg {sum(hpb_values)/len(hpb_values):.1f})")

    print(f"\n  Roles:")
    for role, count in role_counts.most_common():
        print(f"    {role}: {count}")

    print(f"\n  Tags ({len(tag_counts)} unique):")
    for tag, count in tag_counts.most_common():
        print(f"    {tag}: {count}")


# ── CLI ──────────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(
        description="Import MIDI files into drumgen's cell format"
    )
    parser.add_argument("input", nargs="?", help="MIDI file or directory to import")
    parser.add_argument("--name", "-n", type=str, help="Cell name (default: filename)")
    parser.add_argument("--tags", type=str, default="imported",
                        help="Comma-separated tags (default: imported)")
    parser.add_argument("--kit", type=str, default="ugritone",
                        help="Kit mapping name (default: ugritone)")
    parser.add_argument("--role", type=str, default="groove",
                        choices=["groove", "fill", "transition"],
                        help="Cell role (default: groove)")
    parser.add_argument("--humanize", type=float, default=0.5,
                        help="Humanization level 0.0-1.0 (default: 0.5)")
    parser.add_argument("--auto-tag", action="store_true",
                        help="Auto-detect tags from content analysis")
    parser.add_argument("--force", action="store_true",
                        help="Import even if validation fails")
    parser.add_argument("--list", action="store_true",
                        help="List imported cells")
    parser.add_argument("--output-dir", type=str, default=None,
                        help="Output directory for cell JSON (default: user_cells/)")

    # Maintenance commands
    parser.add_argument("--dedup", action="store_true",
                        help="Find and remove duplicate cells (dry-run unless --confirm)")
    parser.add_argument("--confirm", action="store_true",
                        help="Actually execute dedup removal")
    parser.add_argument("--validate", action="store_true",
                        help="Validate all existing cells")
    parser.add_argument("--retag", action="store_true",
                        help="Re-run auto-tagger on all existing cells")
    parser.add_argument("--stats", action="store_true",
                        help="Print cell statistics")

    args = parser.parse_args()

    # Maintenance commands (no input required)
    if args.stats:
        cmd_stats(args.output_dir)
        return
    if args.validate:
        cmd_validate(args.output_dir, kit_name=args.kit)
        return
    if args.retag:
        cmd_retag(args.output_dir)
        return
    if args.dedup:
        cmd_dedup(args.output_dir, confirm=args.confirm)
        return
    if args.list:
        list_user_cells(args.output_dir)
        return

    if not args.input:
        parser.error("input path required (or use --list, --stats, --validate, --retag, --dedup)")

    if os.path.isdir(args.input):
        _import_directory(args.input, auto_tag=args.auto_tag, kit_name=args.kit,
                          force=args.force)
        return

    tags = [t.strip() for t in args.tags.split(",") if t.strip()]

    cell = midi_to_cell(
        args.input,
        name=args.name,
        tags=tags,
        kit_name=args.kit,
        humanize=args.humanize,
        role=args.role,
    )

    # Auto-tag if requested
    if args.auto_tag:
        auto_tag_cell(cell)

    # Validate
    errors, warnings = validate_cell(cell, kit_name=args.kit)
    for w in warnings:
        print(f"Warning: {w}", file=sys.stderr)
    if errors and not args.force:
        for e in errors:
            print(f"Error: {e}", file=sys.stderr)
        print("Use --force to import anyway.", file=sys.stderr)
        sys.exit(1)

    path = save_cell(cell, directory=args.output_dir)
    if path is None:
        print(f"Skipped: {cell['name']} (duplicate of existing cell)")
        return

    bpm_str = f" | {cell['source_bpm']} BPM" if cell.get("source_bpm") else ""
    print(f"Imported: {cell['name']}")
    print(f"  {cell['time_sig'][0]}/{cell['time_sig'][1]} | {cell['num_bars']} bars | "
          f"{len(cell['hits'])} hits{bpm_str}")
    print(f"  Tags: {', '.join(cell['tags'])}")
    print(f"  Saved: {path}")


if __name__ == "__main__":
    main()
