#!/usr/bin/env python3
import argparse
import os
import sys

from assembler import assemble
from cell_library import list_cells, STYLE_MAP
from midi_engine import write_midi, generate_test_mapping


def main():
    parser = argparse.ArgumentParser(
        description="drumgen v3 — algorithmic drum pattern generator"
    )
    parser.add_argument("--style", "-s", type=str, help="Style tag (blast, dbeat, shellac, fugazi, faraquet, raein, etc.)")
    parser.add_argument("--cell", type=str, help="Exact cell name (overrides --style)")
    parser.add_argument("--tempo", "-t", type=int, default=120, help="BPM (default: 120)")
    parser.add_argument("--bars", "-b", type=int, default=4, help="Number of bars (default: 4)")
    parser.add_argument("--time-sig", "-ts", type=str, default="4/4", help="Time signature as N/D (default: 4/4)")
    parser.add_argument("--humanize", type=float, default=None, help="Humanization 0.0-1.0 (default: per-cell)")
    parser.add_argument("--swing", type=float, default=0.0, help="Swing amount 0.0-1.0 (default: 0.0)")
    parser.add_argument("--fill-every", type=int, default=0, help="Insert fill every N bars (0=none)")
    parser.add_argument("--seed", type=int, default=None, help="Random seed for reproducibility")
    parser.add_argument("--kit", type=str, default="ugritone", help="Kit mapping name or path (default: ugritone)")
    parser.add_argument("--output", "-o", type=str, default=None, help="Output .mid path")
    parser.add_argument("--list-cells", action="store_true", help="List available cells")
    parser.add_argument("--test-mapping", type=str, metavar="MAPPING", help="Generate test MIDI for a kit mapping")

    args = parser.parse_args()

    # --list-cells mode
    if args.list_cells:
        cells = list_cells(style_filter=args.style)
        if not cells:
            print(f"No cells found for style '{args.style}'." if args.style else "No cells found.")
            return
        print(f"Available cells ({len(cells)}):\n")
        for c in cells:
            bars_str = f"{c['num_bars']} bar{'s' if c['num_bars'] > 1 else ''}"
            ts = f"{c['time_sig'][0]}/{c['time_sig'][1]}"
            tags = ", ".join(c["tags"])
            print(f"  {c['name']}")
            print(f"    {ts} | {bars_str} | humanize: {c['humanize']} | role: {c['role']}")
            print(f"    tags: {tags}")
            print()
        print("Style shortcuts:")
        for style, cell_name in sorted(STYLE_MAP.items()):
            print(f"  --style {style} -> {cell_name}")
        return

    # --test-mapping mode
    if args.test_mapping:
        output = args.output or f"output/test_{args.test_mapping}.mid"
        generate_test_mapping(args.test_mapping, output)
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
    )

    # Output path
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
