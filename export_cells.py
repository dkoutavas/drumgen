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
    else:
        out["type"] = "fixed"
        out["hits"] = [list(h) for h in cell["hits"]]

    return out


def export(output_path):
    """Build the export dict and write it to output_path."""
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
            if sorted(names) == sorted(STYLE_POOLS.get(canonical, [])):
                aliased.append(f"{style} -> {canonical}")
                continue
            # Pool diverged from its twin — export it and warn loudly.
            print(f"WARNING: {style} no longer matches {canonical}; exporting both. "
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

    # Create output directory if needed
    out_dir = os.path.dirname(output_path)
    if out_dir:
        os.makedirs(out_dir, exist_ok=True)

    with open(output_path, "w") as f:
        json.dump(data, f, indent=2)

    # Summary
    n_fixed = sum(1 for c in builtin_cells.values() if c["type"] == "fixed")
    n_prob = sum(1 for c in builtin_cells.values() if c["type"] == "probability")
    n_styles = len(data["style_pools"])
    n_sections = len(data["section_preferences"])

    print(f"Exported {len(builtin_cells)} cells ({n_fixed} fixed, {n_prob} probability)")
    print(f"  {n_styles} style pools, {n_sections} section preferences")
    if dropped:
        print(f"  pruned {dropped} dangling pool entries (user-imported cells not shipped)")
    if aliased:
        print(f"  skipped {len(aliased)} alias styles (identical pools): " + ", ".join(aliased))
    print(f"  -> {output_path}")


def main():
    parser = argparse.ArgumentParser(
        description="Export built-in cell library to JSON for the Rust plugin."
    )
    parser.add_argument(
        "-o", "--output",
        default=DEFAULT_OUTPUT,
        help=f"Output path (default: {DEFAULT_OUTPUT})",
    )
    args = parser.parse_args()
    export(args.output)


if __name__ == "__main__":
    main()
