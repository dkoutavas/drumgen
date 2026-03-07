#!/usr/bin/env python3
import argparse
import os
import sys

from assembler import assemble, assemble_arrangement
from cell_library import list_cells, STYLE_POOLS, CELLS, SECTION_PREFERENCES
from midi_engine import write_midi, generate_test_mapping


def _print_cells(cells, style_filter):
    if not cells:
        print(f"No cells found for style '{style_filter}'." if style_filter else "No cells found.")
        return

    # Group by role
    groups = {}
    for c in cells:
        role = c["role"]
        groups.setdefault(role, []).append(c)

    role_labels = {
        "groove": "Groove Cells",
        "fill": "Fill Cells",
        "transition": "Transition Cells",
    }

    print(f"Available cells ({len(cells)}):\n")
    for role in ("groove", "fill", "transition"):
        group = groups.get(role, [])
        if not group:
            continue
        print(f"  {role_labels.get(role, role.title())}:")
        for c in group:
            bars_str = f"{c['num_bars']} bar{'s' if c['num_bars'] > 1 else ''}"
            ts = f"{c['time_sig'][0]}/{c['time_sig'][1]}"
            tags = ", ".join(c["tags"])
            print(f"    {c['name']}")
            print(f"      {ts} | {bars_str} | humanize: {c['humanize']} | role: {c['role']}")
            print(f"      tags: {tags}")
        print()

    print("Style pools:")
    for style, cell_names in sorted(STYLE_POOLS.items()):
        print(f"  --style {style} -> {', '.join(cell_names)}")
    print()
    print("Arrangement mode:")
    print('  --style screamo -a "4:blast 1:silence 4:breakdown"')
    print(f"  Section types: {', '.join(sorted(SECTION_PREFERENCES.keys()))}")


def main():
    parser = argparse.ArgumentParser(
        description="drumgen v3 — algorithmic drum pattern generator"
    )
    parser.add_argument("--style", "-s", type=str, help="Style tag (blast, dbeat, shellac, fugazi, screamo, emoviolence, euro_screamo, black_metal, etc.)")
    parser.add_argument("--cell", type=str, help="Exact cell name (overrides --style)")
    parser.add_argument("--tempo", "-t", type=int, default=120, help="BPM (default: 120)")
    parser.add_argument("--bars", "-b", type=int, default=4, help="Number of bars (default: 4)")
    parser.add_argument("--time-sig", "-ts", type=str, default="4/4", help="Time signature as N/D (default: 4/4)")
    parser.add_argument("--humanize", type=float, default=None, help="Humanization 0.0-1.0 (default: per-cell)")
    parser.add_argument("--swing", type=float, default=0.0, help="Swing amount 0.0-1.0 (default: 0.0)")
    parser.add_argument("--fill-every", type=int, default=0, help="Insert fill every N bars (0=none)")
    parser.add_argument("--seed", type=int, default=None, help="Random seed for reproducibility")
    parser.add_argument("--vary", "-v", type=float, default=0.0,
                        help="Variation amount 0.0-1.0 (default: 0.0)")
    parser.add_argument("--kit", type=str, default="ugritone", help="Kit mapping name or path (default: ugritone)")
    parser.add_argument("--output", "-o", type=str, default=None, help="Output .mid path")
    parser.add_argument("--arrangement", "-a", type=str, default=None,
                        help='Arrangement string, e.g. "4:build 8:drive 2:blast 1:silence 4:breakdown"')
    parser.add_argument("--list-cells", action="store_true", help="List available cells")
    parser.add_argument("--test-mapping", type=str, metavar="MAPPING", help="Generate test MIDI for a kit mapping")

    args = parser.parse_args()

    # --list-cells mode
    if args.list_cells:
        cells = list_cells(style_filter=args.style)
        _print_cells(cells, args.style)
        return

    # --test-mapping mode
    if args.test_mapping:
        output = args.output or f"output/test_{args.test_mapping}.mid"
        generate_test_mapping(args.test_mapping, output)
        return

    # Arrangement mode
    if args.arrangement:
        if not args.style:
            parser.error("--arrangement requires --style")

        result = assemble_arrangement(
            style=args.style,
            arrangement_str=args.arrangement,
            tempo=args.tempo,
            time_sig=args.time_sig,
            humanize=args.humanize,
            swing=args.swing,
            seed=args.seed,
            vary=args.vary,
        )

        if args.output:
            output_path = args.output
        else:
            output_path = f"output/{args.style}_arrangement_{args.tempo}bpm.mid"

        write_midi(
            events=result["events"],
            tempo=result["tempo"],
            time_signatures=result["time_signatures"],
            kit_mapping_path=args.kit,
            output_path=output_path,
        )

        print(f"Seed: {result['seed']} (use --seed {result['seed']} to reproduce)")
        print(f"Bars: {result['total_bars']}")
        print(f"Sections: {result['section_summary']}")
        print(f"Events: {len(result['events'])}")
        return

    # Generation mode — need style or cell
    if not args.style and not args.cell:
        parser.error("--style or --cell is required for generation (or use --list-cells / --test-mapping)")

    result = assemble(
        style=args.style,
        cell_name=args.cell,
        bars=args.bars,
        tempo=args.tempo,
        time_sig=args.time_sig,
        humanize=args.humanize,
        swing=args.swing,
        fill_every=args.fill_every,
        seed=args.seed,
        vary=args.vary,
    )

    if args.output:
        output_path = args.output
    else:
        label = args.cell or args.style
        output_path = f"output/{label}_{args.tempo}bpm_{args.bars}bars.mid"

    write_midi(
        events=result["events"],
        tempo=result["tempo"],
        time_signatures=result["time_signatures"],
        kit_mapping_path=args.kit,
        output_path=output_path,
    )

    print(f"Seed: {result['seed']} (use --seed {result['seed']} to reproduce)")
    print(f"Events: {len(result['events'])}")


if __name__ == "__main__":
    main()
