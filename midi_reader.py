#!/usr/bin/env python3
"""Import MIDI files into drumgen's cell format.

Standalone CLI and importable library. Reads .mid via mido, converts to
drumgen's native cell format (flat list of 5-tuples), saves as JSON in
user_cells/.

Usage:
    python midi_reader.py input.mid --name my_cell --tags blast,intense --kit ugritone
    python midi_reader.py input_dir/ --auto-tag
    python midi_reader.py --list
"""

import argparse
import json
import os
import sys

import mido

from midi_engine import load_kit_mapping, DEFAULT_PPQ, MIDI_CHANNEL

USER_CELLS_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "user_cells")


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

    if name is None:
        name = os.path.splitext(os.path.basename(midi_path))[0]
        # Sanitize: replace spaces/dashes with underscores
        name = name.replace(" ", "_").replace("-", "_").lower()

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

    return {
        "name": name,
        "tags": tags,
        "time_sig": (time_sig_num, time_sig_den),
        "num_bars": num_bars,
        "humanize": humanize,
        "role": role,
        "hits": hits,
        "source": "imported",
        "source_file": os.path.basename(midi_path),
    }


def save_cell(cell, directory=None):
    """Save a cell dict as JSON to user_cells/."""
    if directory is None:
        directory = USER_CELLS_DIR
    os.makedirs(directory, exist_ok=True)
    filepath = os.path.join(directory, f"{cell['name']}.json")
    # Convert tuples to lists for JSON serialization
    serializable = dict(cell)
    serializable["time_sig"] = list(cell["time_sig"])
    serializable["hits"] = [list(h) for h in cell["hits"]]
    with open(filepath, "w") as f:
        json.dump(serializable, f, indent=2)
    return filepath


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
            print(f"  {cell['name']}")
            print(f"    {ts} | {bars} bar{'s' if bars > 1 else ''} | tags: {tags}")
            print(f"    source: {src}")
        except (json.JSONDecodeError, KeyError) as e:
            print(f"  {filename} (error: {e})")


def _import_directory(dir_path, auto_tag=False, kit_name="ugritone"):
    """Import all .mid files in a directory."""
    mid_files = sorted(
        f for f in os.listdir(dir_path)
        if f.lower().endswith((".mid", ".midi"))
    )
    if not mid_files:
        print(f"No MIDI files found in {dir_path}")
        return

    for filename in mid_files:
        filepath = os.path.join(dir_path, filename)
        try:
            tags = ["imported"]
            if auto_tag:
                stem = os.path.splitext(filename)[0].lower()
                for keyword in ("blast", "dbeat", "groove", "fill", "intro",
                                "verse", "chorus", "breakdown", "build"):
                    if keyword in stem:
                        tags.append(keyword)
            cell = midi_to_cell(filepath, tags=tags, kit_name=kit_name)
            path = save_cell(cell)
            print(f"Imported: {cell['name']} -> {path}")
        except Exception as e:
            print(f"Skipping {filename}: {e}", file=sys.stderr)


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
                        help="Auto-detect tags from filename (directory mode)")
    parser.add_argument("--list", action="store_true",
                        help="List imported cells")
    parser.add_argument("--output-dir", type=str, default=None,
                        help="Output directory for cell JSON (default: user_cells/)")

    args = parser.parse_args()

    if args.list:
        list_user_cells(args.output_dir)
        return

    if not args.input:
        parser.error("input path required (or use --list)")

    if os.path.isdir(args.input):
        _import_directory(args.input, auto_tag=args.auto_tag, kit_name=args.kit)
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
    path = save_cell(cell, directory=args.output_dir)
    print(f"Imported: {cell['name']}")
    print(f"  {cell['time_sig'][0]}/{cell['time_sig'][1]} | {cell['num_bars']} bars | "
          f"{len(cell['hits'])} hits")
    print(f"  Saved: {path}")


if __name__ == "__main__":
    main()
