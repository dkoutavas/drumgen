#!/usr/bin/env python3
import argparse
import os
import random
import sys

import arrangement_gen
from assembler import assemble, assemble_arrangement, assemble_layered
from cell_library import list_cells, STYLE_POOLS, SECTION_PREFERENCES
from midi_engine import write_midi, generate_test_mapping, unique_filepath
from platform_paths import default_output_dir

OUTPUT_DIR = default_output_dir()


def _validate_args(args):
    """First problem with the numeric options, as a message, else None.

    The engine rejects what it cannot play (tempo, bars, time signature,
    section names); these are the ranges the --help text promises. Out-of-range
    values used to be accepted silently: --humanize 5, --swing 2, --vary 3,
    --variations 0 and --fill-every -1 all ran.
    """
    for name, value, lo, hi in (
        ("--humanize", args.humanize, 0.0, 1.0),
        ("--swing", args.swing, 0.0, 1.0),
        ("--vary", args.vary, 0.0, 1.0),
    ):
        if value is not None and not lo <= value <= hi:
            return f"{name} must be between {lo} and {hi}, got {value}"
    if args.variations < 1:
        return f"--variations must be at least 1, got {args.variations}"
    if args.fill_every < 0:
        return f"--fill-every must be 0 (off) or more, got {args.fill_every}"
    if args.form_seed is not None and not 0 <= args.form_seed <= arrangement_gen.FORM_SEED_MAX:
        return f"--form-seed must be between 0 and {arrangement_gen.FORM_SEED_MAX}, got {args.form_seed}"
    return None


def _ts_suffix(time_sig):
    """Return '_3_4' for non-4/4 time sigs, empty string for 4/4."""
    return f"_{time_sig.replace('/', '_')}" if time_sig != "4/4" else ""


def _write_musicxml(args, output_path):
    import notation
    xml_path = os.path.splitext(output_path)[0] + ".musicxml"
    notation.mid_to_musicxml(output_path, xml_path,
                             kit_name=os.path.splitext(os.path.basename(args.kit))[0])
    print(f"MusicXML saved: {xml_path}")


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
        description="drumgen — algorithmic drum pattern generator",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""\
examples:
  %(prog)s --style screamo --tempo 180 --bars 4
  %(prog)s --style fugazi --tempo 130 --bars 8 --humanize 0.8
  %(prog)s --style euro_screamo -a "8:build 8:drive 4:blast" --tempo 140
  %(prog)s --style fugazi -a "4:verse@7/8 2:verse@4/4 4:verse@7/8" --tempo 130
  %(prog)s --style fugazi --form arc --form-seed 42 --tempo 140
  %(prog)s --kick blast_traditional --cymbal shellac_floor_tom_drive --bars 4 --tempo 160
  %(prog)s --style faraquet --generative --variations 3 --tempo 140 --bars 8
  %(prog)s --list-cells
  %(prog)s --list-cells --style screamo
  %(prog)s --test-mapping ugritone
""",
    )
    parser.add_argument("--style", "-s", type=str, help="Style (see --list-cells for every style pool: screamo, fugazi, zona, euro_screamo, kidcrash, ...)")
    parser.add_argument("--cell", type=str, help="Exact cell name (overrides --style)")
    parser.add_argument("--tempo", "-t", type=int, default=120, help="BPM (default: 120)")
    parser.add_argument("--bars", "-b", type=int, default=4, help="Number of bars (default: 4)")
    parser.add_argument("--time-sig", "-ts", type=str, default=None,
                        help="Time signature as N/D (default: 4/4; with --form, leaving it out lets the form turn meters)")
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
    parser.add_argument("--form", type=str, default=None, choices=list(arrangement_gen.FORMS),
                        help="Generate a song form of this shape (prints the arrangement it played)")
    parser.add_argument("--form-seed", type=int, default=None,
                        help=f"Seed for the form's structure, 0-{arrangement_gen.FORM_SEED_MAX} "
                             "(default: random; --seed still rolls the notes)")
    parser.add_argument("--generative", "-g", action="store_true",
                        help="Use probability grids for generative patterns")
    parser.add_argument("--variations", "-n", type=int, default=1,
                        help="Generate N variations (each with different seed)")
    parser.add_argument("--kick", type=str, default=None, help="Layer mode: cell for kick layer")
    parser.add_argument("--snare", type=str, default=None, help="Layer mode: cell for snare layer")
    parser.add_argument("--cymbal", type=str, default=None, help="Layer mode: cell for cymbal layer")
    parser.add_argument("--toms", type=str, default=None, help="Layer mode: cell for toms layer")
    parser.add_argument("--list-cells", action="store_true", help="List available cells")
    parser.add_argument("--musicxml", action="store_true",
                        help="Also write drummer-readable MusicXML next to the .mid (open in MuseScore)")
    parser.add_argument("--test-mapping", type=str, metavar="MAPPING", help="Generate test MIDI for a kit mapping")

    args = parser.parse_args()
    problem = _validate_args(args)
    if problem:
        parser.error(problem)
    # A form may turn meters only while the meter is left on "auto".
    meter_forced = args.time_sig is not None
    args.time_sig = args.time_sig or "4/4"

    # --list-cells mode
    if args.list_cells:
        cells = list_cells(style_filter=args.style)
        _print_cells(cells, args.style)
        return

    # --test-mapping mode
    if args.test_mapping:
        output = args.output or f"output/test_{args.test_mapping}.mid"
        output = unique_filepath(output)
        generate_test_mapping(args.test_mapping, output)
        return

    # Layer mode
    layer_args = {k: v for k, v in [("kick", args.kick), ("snare", args.snare),
                                     ("cymbal", args.cymbal), ("toms", args.toms)] if v}
    if layer_args:
        if args.arrangement or args.form:
            parser.error("--kick/--snare/--cymbal/--toms cannot be used with --arrangement or --form")
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
            output_path = f"{OUTPUT_DIR}/layered_{layer_label}_{args.tempo}bpm{_ts_suffix(args.time_sig)}.mid"
        output_path = unique_filepath(output_path)

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

    # Generated form: becomes an arrangement string, then plays like one.
    form_label = None
    if args.form:
        if args.arrangement:
            parser.error("--form and --arrangement are two ways to say the same thing; use one")
        if not args.style:
            parser.error("--form requires --style")
        if args.form_seed is None:
            args.form_seed = random.randint(0, arrangement_gen.FORM_SEED_MAX)
        args.arrangement = arrangement_gen.generate_form(
            args.form, args.form_seed, style=args.style,
            home=tuple(int(x) for x in args.time_sig.split("/")), meter_auto=not meter_forced)
        form_label = arrangement_gen.form_name(args.form, args.form_seed)
    elif args.form_seed is not None:
        parser.error("--form-seed needs --form")

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
            what = f"{args.form}{args.form_seed}" if form_label else "arrangement"
            output_path = f"{OUTPUT_DIR}/{args.style}_{what}_{args.tempo}bpm{_ts_suffix(args.time_sig)}.mid"
        output_path = unique_filepath(output_path)

        write_midi(
            events=result["events"],
            tempo=result["tempo"],
            time_signatures=result["time_signatures"],
            kit_mapping_path=args.kit,
            output_path=output_path,
        )

        if args.musicxml:
            _write_musicxml(args, output_path)
        if form_label:
            print(f"Form: {form_label} (use --form {args.form} --form-seed {args.form_seed} to reproduce)")
            print(f"  songs.txt line:  {form_label} | {args.arrangement}")
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

        base_output = args.output or f"{OUTPUT_DIR}/{args.cell or args.style}_{args.tempo}bpm{_ts_suffix(args.time_sig)}_{args.bars}bars.mid"
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
            output_path = unique_filepath(f"{base}_v{vi}{ext}")
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
        output_path = f"{OUTPUT_DIR}/{label}_{args.tempo}bpm{_ts_suffix(args.time_sig)}_{args.bars}bars.mid"
    output_path = unique_filepath(output_path)

    write_midi(
        events=result["events"],
        tempo=result["tempo"],
        time_signatures=result["time_signatures"],
        kit_mapping_path=args.kit,
        output_path=output_path,
    )

    if args.musicxml:
        _write_musicxml(args, output_path)

    print(f"Seed: {result['seed']} (use --seed {result['seed']} to reproduce)")
    print(f"Events: {len(result['events'])}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, FileNotFoundError) as e:
        # str(KeyError) is the repr of its message: quotes and a literal \n.
        msg = e.args[0] if isinstance(e, KeyError) and e.args else e
        print(f"Error: {msg}", file=sys.stderr)
        sys.exit(1)
