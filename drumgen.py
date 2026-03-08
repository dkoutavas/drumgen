#!/usr/bin/env python3
import argparse
import os
import random
import sys

from assembler import assemble, assemble_arrangement, assemble_layered
from cell_library import list_cells, STYLE_POOLS, CELLS, SECTION_PREFERENCES
from midi_engine import write_midi, generate_test_mapping


def _print_cells(cells, style_filter):
    if not cells:
        print(f"No cells found for style '{style_filter}'." if style_filter else "No cells found.")
        return

    # Separate imported from built-in, then group by role
    imported = [c for c in cells if c.get("source") == "imported"]
    builtin = [c for c in cells if c.get("source") != "imported"]

    groups = {}
    for c in builtin:
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

    if imported:
        print(f"  Imported Cells ({len(imported)}):")
        for c in imported:
            bars_str = f"{c['num_bars']} bar{'s' if c['num_bars'] > 1 else ''}"
            ts = f"{c['time_sig'][0]}/{c['time_sig'][1]}"
            tags = ", ".join(c["tags"])
            src = c.get("source_file", "")
            bpm = c.get("source_bpm")
            bpm_str = f" | {bpm} BPM" if bpm else ""
            pools = c.get("_pools", [])
            pools_str = f" | pools: {', '.join(pools)}" if pools else ""
            print(f"    {c['name']}")
            print(f"      {ts} | {bars_str}{bpm_str} | humanize: {c['humanize']} | role: {c['role']}")
            print(f"      tags: {tags}{pools_str}")
            if src:
                print(f"      source: {src}")
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
                        help='Arrangement: "4:build 8:drive@7/8 2:blast". Use @N/M for per-section time sig.')
    parser.add_argument("--generative", "-g", action="store_true",
                        help="Use probability grids for generative patterns")
    parser.add_argument("--variations", "-n", type=int, default=1,
                        help="Generate N variations (each with different seed)")
    parser.add_argument("--kick", type=str, default=None, help="Layer mode: cell for kick layer")
    parser.add_argument("--snare", type=str, default=None, help="Layer mode: cell for snare layer")
    parser.add_argument("--cymbal", type=str, default=None, help="Layer mode: cell for cymbal layer")
    parser.add_argument("--toms", type=str, default=None, help="Layer mode: cell for toms layer")
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

    # Layer mode
    layer_args = {k: v for k, v in [("kick", args.kick), ("snare", args.snare),
                                     ("cymbal", args.cymbal), ("toms", args.toms)] if v}
    if layer_args:
        if args.arrangement:
            parser.error("--kick/--snare/--cymbal/--toms cannot be used with --arrangement")
        if args.style or args.cell:
            print("Warning: --style/--cell ignored in layer mode", file=sys.stderr)

        result = assemble_layered(
            layers=layer_args,
            bars=args.bars,
            tempo=args.tempo,
            time_sig=args.time_sig,
            humanize=args.humanize,
            swing=args.swing,
            vary=args.vary,
            seed=args.seed,
        )

        if args.output:
            output_path = args.output
        else:
            layer_label = "+".join(sorted(layer_args.keys()))
            output_path = f"output/layered_{layer_label}_{args.tempo}bpm.mid"

        write_midi(
            events=result["events"],
            tempo=result["tempo"],
            time_signatures=result["time_signatures"],
            kit_mapping_path=args.kit,
            output_path=output_path,
        )

        print(f"Layers: {', '.join(f'{k}={v}' for k, v in layer_args.items())}")
        print(f"Seed: {result['seed']} (use --seed {result['seed']} to reproduce)")
        print(f"Events: {len(result['events'])}")
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
            generative=args.generative,
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

    # Handle variations
    num_variations = max(1, args.variations)
    if num_variations > 1:
        if args.seed is not None:
            seeds = [args.seed + i for i in range(num_variations)]
        else:
            base_rng = random.Random()
            seeds = [base_rng.randint(0, 2**31 - 1) for _ in range(num_variations)]

        base_output = args.output or f"output/{args.cell or args.style}_{args.tempo}bpm_{args.bars}bars.mid"
        base, ext = os.path.splitext(base_output)

        for vi, var_seed in enumerate(seeds, 1):
            result = assemble(
                style=args.style,
                cell_name=args.cell,
                bars=args.bars,
                tempo=args.tempo,
                time_sig=args.time_sig,
                humanize=args.humanize,
                swing=args.swing,
                fill_every=args.fill_every,
                seed=var_seed,
                vary=args.vary,
                generative=args.generative,
            )
            output_path = f"{base}_v{vi}{ext}"
            write_midi(
                events=result["events"],
                tempo=result["tempo"],
                time_signatures=result["time_signatures"],
                kit_mapping_path=args.kit,
                output_path=output_path,
            )
            print(f"  v{vi}: seed={result['seed']}, events={len(result['events'])}")

        print(f"\nGenerated {num_variations} variations")
        return

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
        generative=args.generative,
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
