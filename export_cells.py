#!/usr/bin/env python3
"""Export the built-in cell library to JSON for the Rust plugin."""

import argparse
import json
import os
import sys

# Ensure the project root is on sys.path so cell_library can be imported
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from cell_library import CELLS, STYLE_POOLS, SECTION_PREFERENCES


DEFAULT_OUTPUT = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "plugin", "cells", "builtin.json"
)


def _serialize_cell(cell):
    """Convert a cell dict to a JSON-safe representation."""
    out = {
        "name": cell["name"],
        "tags": list(cell.get("tags", [])),
        "time_sig": list(cell["time_sig"]),
        "num_bars": cell["num_bars"],
        "humanize": cell.get("humanize", 0.5),
        "role": cell.get("role", "groove"),
    }

    if cell.get("type") == "probability":
        out["type"] = "probability"
        out["grid"] = [list(entry) for entry in cell["grid"]]
    elif cell.get("type") == "euclidean":
        out["type"] = "euclidean"
        out["limbs"] = [dict(limb) for limb in cell["limbs"]]
    else:
        out["type"] = "fixed"
        out["hits"] = [list(h) for h in cell["hits"]]

    return out


def build_export():
    """Build the export dict: (data, notes). Pure, writes nothing, so a test or
    `--check` can compare it with the committed file."""
    notes = []
    # Only export built-in cells (skip user-imported ones)
    builtin_cells = {}
    for name, cell in sorted(CELLS.items()):
        if cell.get("source") == "imported":
            continue
        builtin_cells[name] = _serialize_cell(cell)

    # Styles whose pools are byte-identical to another style's. In the plugin's
    # Style picker they read as duplicates (same cells = same beats), so only the
    # canonical name is exported. The CLI keeps the aliases (--style math etc.).
    # If a pool ever diverges from its canonical twin, remove it from this map.
    style_aliases = {
        "math": "faraquet",
        "blood_brothers": "atdi",
        "dry_cleaning": "preoccupations",
    }

    # Prune style pools to cells that actually ship in the plugin. STYLE_POOLS
    # picks up user-imported cell names at import time (via tag-to-pool mapping),
    # but those bodies are excluded above — leaving dangling names that inflate
    # pool sizes and would never resolve in the plugin. Keep only resolvable ones.
    pruned_pools = {}
    dropped = 0
    aliased = []
    for style, names in sorted(STYLE_POOLS.items()):
        if style in style_aliases:
            canonical = style_aliases[style]
            # Compare what actually SHIPS: imported cells auto-integrate into
            # pools via TAG_TO_POOLS but are excluded from the export, so the
            # runtime pools can diverge (mined cells routed to math but not
            # faraquet) while the exported pools stay identical.
            mine = sorted(n for n in names if n in builtin_cells)
            twin = sorted(n for n in STYLE_POOLS.get(canonical, []) if n in builtin_cells)
            if mine == twin:
                aliased.append(f"{style} -> {canonical}")
                continue
            # Pool diverged from its twin — export it and warn loudly.
            notes.append(f"WARNING: {style} no longer matches {canonical}; exporting both. "
                         f"Remove it from style_aliases in export_cells.py.")
        keep = [n for n in names if n in builtin_cells]
        dropped += len(names) - len(keep)
        if keep:
            pruned_pools[style] = keep

    data = {
        "cells": builtin_cells,
        "style_pools": pruned_pools,
        "section_preferences": {sec: list(tags) for sec, tags in sorted(SECTION_PREFERENCES.items())},
    }
    if dropped:
        notes.append(f"  pruned {dropped} dangling pool entries (user-imported cells not shipped)")
    if aliased:
        notes.append(f"  skipped {len(aliased)} alias styles (identical pools): " + ", ".join(aliased))
    return data, notes


def export(output_path):
    """Build the export dict and write it to output_path."""
    data, notes = build_export()
    builtin_cells = data["cells"]

    # Create output directory if needed
    out_dir = os.path.dirname(output_path)
    if out_dir:
        os.makedirs(out_dir, exist_ok=True)

    with open(output_path, "w") as f:
        json.dump(data, f, indent=2)

    # Summary
    kinds = {k: sum(1 for c in builtin_cells.values() if c["type"] == k)
             for k in ("fixed", "probability", "euclidean")}
    n_styles = len(data["style_pools"])
    n_sections = len(data["section_preferences"])

    print(f"Exported {len(builtin_cells)} cells ({kinds['fixed']} fixed, "
          f"{kinds['probability']} probability, {kinds['euclidean']} euclidean)")
    print(f"  {n_styles} style pools, {n_sections} section preferences")
    for note in notes:
        print(note)
    print(f"  -> {output_path}")


def is_current(path=DEFAULT_OUTPUT):
    """True when the committed JSON matches what cell_library.py would export."""
    data, _ = build_export()
    try:
        with open(path) as f:
            on_disk = json.load(f)
    except (OSError, ValueError):
        return False
    return on_disk == json.loads(json.dumps(data))


def main():
    parser = argparse.ArgumentParser(
        description="Export built-in cell library to JSON for the Rust plugin."
    )
    parser.add_argument(
        "-o", "--output",
        default=DEFAULT_OUTPUT,
        help=f"Output path (default: {DEFAULT_OUTPUT})",
    )
    parser.add_argument(
        "--check", action="store_true",
        help="Write nothing; exit 1 if the file is stale (what CI runs)",
    )
    args = parser.parse_args()
    if args.check:
        if is_current(args.output):
            print(f"{args.output} is current")
            return
        print(f"{args.output} is STALE: run `python export_cells.py` and commit it", file=sys.stderr)
        sys.exit(1)
    export(args.output)


if __name__ == "__main__":
    main()
