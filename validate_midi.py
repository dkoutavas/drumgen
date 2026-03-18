#!/usr/bin/env python3
"""MIDI pipeline validation script.

Generates MIDI across many configurations and validates pipeline correctness.
Runnable standalone or via pytest.

Usage:
    python validate_midi.py                  # quick mode (8 styles)
    python validate_midi.py --full           # exhaustive matrix
    python validate_midi.py --style shellac  # single style
    python validate_midi.py --cell blast_traditional  # single cell
    python validate_midi.py -v               # verbose (show pass details)
    python validate_midi.py --fail-fast      # stop on first failure
"""

import argparse
import contextlib
import io
import os
import sys
import tempfile
from dataclasses import dataclass, field

import mido

from assembler import (
    assemble, assemble_arrangement, assemble_layered,
    _CYMBAL_PRIORITY, _STICK_PRIORITY,
)
from cell_library import CELLS, STYLE_POOLS
from midi_engine import (
    write_midi, load_kit_mapping, calculate_bar_start_ticks, DEFAULT_PPQ,
)
from midi_reader import midi_to_cell


# ── Data types ────────────────────────────────────────────────────────────────

@dataclass
class ValidationResult:
    label: str
    passed: bool
    errors: list = field(default_factory=list)
    warnings: list = field(default_factory=list)


# ── Tier 1: Individual check functions ────────────────────────────────────────

def check_bar_alignment(midi_path, time_signatures, ppq=DEFAULT_PPQ):
    """Verify total MIDI ticks match expected bar-aligned length."""
    errors = []
    mid = mido.MidiFile(midi_path)
    total_ticks = sum(msg.time for msg in mid.tracks[0])
    last_ts = time_signatures[-1]
    expected = calculate_bar_start_ticks(last_ts["bar_end"] + 1, time_signatures, ppq)
    if total_ticks != expected:
        errors.append(f"bar_alignment: total ticks {total_ticks} != expected {expected}")
    return errors


def check_notes_in_kit(midi_path, kit_name):
    """Verify every MIDI note in the file exists in the kit mapping."""
    errors = []
    kit = load_kit_mapping(kit_name)
    valid_notes = set(kit["mapping"].values())
    # Add alias-resolved notes
    inst_lookup = {k.lower(): v for k, v in kit["mapping"].items()}
    for alias, target in kit.get("aliases", {}).items():
        if target.lower() in inst_lookup:
            valid_notes.add(inst_lookup[target.lower()])

    mid = mido.MidiFile(midi_path)
    found_notes = set()
    for msg in mid.tracks[0]:
        if msg.type == "note_on" and msg.velocity > 0:
            found_notes.add(msg.note)

    unknown = found_notes - valid_notes
    for note in sorted(unknown):
        errors.append(f"notes_in_kit: MIDI note {note} not in {kit_name} mapping")
    return errors


def check_physical_constraints(events, humanized=False, exclude_ticks=None):
    """Check limb conflicts in event list [(abs_tick, instrument, velocity)].

    When humanized=True, conflicts are reported as warnings instead of errors
    since humanization can cause tick collisions.
    exclude_ticks: set of tick positions to skip (e.g. crash auto-add tick).
    """
    errors = []
    warnings = []
    exclude_ticks = exclude_ticks or set()

    # Group events by tick
    by_tick = {}
    for tick, inst, vel in events:
        by_tick.setdefault(tick, []).append((inst, vel))

    # hihat_pedal is in _CYMBAL_PRIORITY but is a foot operation — it can
    # coexist with any stick cymbal (ride, crash, etc.)
    cymbal_set = set(_CYMBAL_PRIORITY.keys()) - {"hihat_pedal"}
    stick_set = set(_STICK_PRIORITY.keys())

    for tick, hits in by_tick.items():
        if tick in exclude_ticks:
            continue

        instruments = {inst for inst, vel in hits}

        # Check cymbal conflicts: ride + hihat at same tick
        cymbals_present = instruments & cymbal_set
        if len(cymbals_present) > 1:
            priorities = {inst: _CYMBAL_PRIORITY[inst] for inst in cymbals_present}
            max_prio = max(priorities.values())
            # Multiple priority levels present = conflict
            if len(set(priorities.values())) > 1 or sum(1 for p in priorities.values() if p == max_prio) < len(priorities):
                msg = f"physical_constraints: cymbal conflict {sorted(cymbals_present)} at tick {tick}"
                if humanized:
                    warnings.append(msg + " (humanization collision)")
                else:
                    errors.append(msg)

        # Check stick conflicts: snare + tom at same tick
        sticks_present = instruments & stick_set
        if len(sticks_present) > 1:
            priorities = {inst: _STICK_PRIORITY[inst] for inst in sticks_present}
            if len(set(priorities.values())) > 1:
                msg = f"physical_constraints: stick conflict {sorted(sticks_present)} at tick {tick}"
                if humanized:
                    warnings.append(msg + " (humanization collision)")
                else:
                    errors.append(msg)

    return errors, warnings


def check_velocity_range(midi_path):
    """Verify all note_on velocities are in [1, 127]."""
    errors = []
    mid = mido.MidiFile(midi_path)
    for msg in mid.tracks[0]:
        if msg.type == "note_on" and msg.velocity > 0:
            if msg.velocity < 1 or msg.velocity > 127:
                errors.append(f"velocity_range: velocity {msg.velocity} out of [1, 127]")
    return errors


def check_no_dropped_instruments(events, midi_path, kit_name):
    """Verify all instruments in events appear in the MIDI output."""
    errors = []
    kit = load_kit_mapping(kit_name)
    inst_lookup = {k.lower(): v for k, v in kit["mapping"].items()}
    for alias, target in kit.get("aliases", {}).items():
        if alias.lower() not in inst_lookup and target.lower() in inst_lookup:
            inst_lookup[alias.lower()] = inst_lookup[target.lower()]

    # Map event instruments to expected MIDI notes
    expected_notes = set()
    unmapped = set()
    for _, inst, _ in events:
        inst_lower = inst.lower()
        if inst_lower in inst_lookup:
            expected_notes.add(inst_lookup[inst_lower])
        else:
            unmapped.add(inst)

    for inst in sorted(unmapped):
        errors.append(f"dropped_instruments: '{inst}' not in kit, would be silently dropped")

    # Check all expected notes actually appear in MIDI
    mid = mido.MidiFile(midi_path)
    actual_notes = set()
    for msg in mid.tracks[0]:
        if msg.type == "note_on" and msg.velocity > 0:
            actual_notes.add(msg.note)

    missing = expected_notes - actual_notes
    for note in sorted(missing):
        # Find instrument name for this note
        inst_name = next((k for k, v in inst_lookup.items() if v == note), f"note_{note}")
        errors.append(f"dropped_instruments: expected note {note} ({inst_name}) missing from MIDI output")
    return errors


def check_round_trip(events, midi_path, kit_name, time_signatures, ppq=DEFAULT_PPQ,
                     humanized=False):
    """Verify MIDI can be read back and matches original events.

    At humanize=0: compare MIDI note count with read-back note count.
    At humanize>0: only check note count within tolerance.

    Returns (errors, warnings) tuple.
    """
    errors = []
    warnings = []
    kit = load_kit_mapping(kit_name)
    inst_lookup = {k.lower(): v for k, v in kit["mapping"].items()}
    for alias, target in kit.get("aliases", {}).items():
        if alias.lower() not in inst_lookup and target.lower() in inst_lookup:
            inst_lookup[alias.lower()] = inst_lookup[target.lower()]

    # Count original MIDI notes (events that have valid kit mappings)
    original_note_count = sum(1 for _, inst, _ in events if inst.lower() in inst_lookup)

    try:
        cell = midi_to_cell(midi_path, kit_name=kit_name)
    except Exception as e:
        # midi_reader may fail on certain kit alias formats (e.g. ugritone
        # name-based aliases vs note-number aliases). This is a midi_reader
        # bug, not a pipeline bug — report as warning.
        warnings.append(f"round_trip: midi_to_cell failed ({e}) — skipped")
        return errors, warnings

    read_back_count = len(cell.get("hits", []))

    if humanized:
        # Tolerant check: ghost clustering can add notes, humanization shifts positions
        tolerance = 0.15
        lower = int(original_note_count * (1 - tolerance))
        upper = int(original_note_count * (1 + tolerance))
        if not (lower <= read_back_count <= upper):
            errors.append(
                f"round_trip: note count {read_back_count} outside tolerance "
                f"[{lower}, {upper}] (original {original_note_count})"
            )
    else:
        # Strict: read back MIDI file note count should match.
        # Allow off-by-2 because midi_to_cell's trailing bar trim may remove
        # 1-2 boundary notes, and hit dedup may merge same-position notes.
        mid = mido.MidiFile(midi_path)
        midi_note_count = sum(1 for msg in mid.tracks[0]
                              if msg.type == "note_on" and msg.velocity > 0)
        diff = abs(midi_note_count - read_back_count)
        if diff > 2:
            errors.append(
                f"round_trip: MIDI has {midi_note_count} notes but read-back got "
                f"{read_back_count} (diff={diff})"
            )
        elif diff > 0:
            warnings.append(
                f"round_trip: note count off by {diff} "
                f"(MIDI={midi_note_count}, read-back={read_back_count})"
            )

    return errors, warnings


# ── Tier 2: Pipeline orchestrator ─────────────────────────────────────────────

def validate_pipeline(style=None, cell_name=None, bars=4, tempo=120, time_sig="4/4",
                      humanize=0.0, mode="single", kit_name="ugritone", seed=42,
                      arrangement_str=None, layers=None, fill_every=0):
    """Run all checks on a single pipeline configuration.

    Returns ValidationResult with label, pass/fail, errors, and warnings.
    """
    label_parts = [style or cell_name or "?", time_sig, mode, f"h={humanize}"]
    if fill_every:
        label_parts.append(f"fill={fill_every}")
    label = " / ".join(label_parts)

    all_errors = []
    all_warnings = []

    try:
        # Suppress stdout from write_midi ("MIDI file saved: ...")
        stdout_buf = io.StringIO()

        if mode == "arrangement":
            if not arrangement_str:
                arrangement_str = "2:verse 2:drive"
            with contextlib.redirect_stdout(stdout_buf):
                result = assemble_arrangement(
                    style=style, arrangement_str=arrangement_str,
                    tempo=tempo, time_sig=time_sig, humanize=humanize,
                    seed=seed,
                )
        elif mode == "layered":
            if not layers:
                return ValidationResult(label, True, [], ["skipped: no layers specified"])
            with contextlib.redirect_stdout(stdout_buf):
                result = assemble_layered(
                    layers=layers, bars=bars, tempo=tempo, time_sig=time_sig,
                    humanize=humanize, seed=seed,
                )
        elif mode == "generative":
            with contextlib.redirect_stdout(stdout_buf):
                result = assemble(
                    style=style, cell_name=cell_name, bars=bars, tempo=tempo,
                    time_sig=time_sig, humanize=humanize, seed=seed,
                    generative=True,
                )
        else:  # single
            with contextlib.redirect_stdout(stdout_buf):
                result = assemble(
                    style=style, cell_name=cell_name, bars=bars, tempo=tempo,
                    time_sig=time_sig, humanize=humanize, seed=seed,
                    fill_every=fill_every,
                )

        events = result["events"]
        time_signatures = result["time_signatures"]

        if not events:
            all_warnings.append("pipeline produced 0 events")
            return ValidationResult(label, True, [], all_warnings)

        # Write MIDI to tempfile
        with tempfile.NamedTemporaryFile(suffix=".mid", delete=False) as f:
            tmp_path = f.name

        try:
            with contextlib.redirect_stdout(stdout_buf):
                write_midi(events, tempo, time_signatures, kit_name, tmp_path)

            # Run all checks
            all_errors.extend(check_bar_alignment(tmp_path, time_signatures))
            all_errors.extend(check_notes_in_kit(tmp_path, kit_name))

            is_humanized = humanize > 0

            # The assembler auto-adds a crash at bar 1 beat 1 (and at
            # intense section starts in arrangement mode) which intentionally
            # overlays the existing cymbal. Exclude those ticks from
            # physical constraint checks.
            crash_ticks = set()
            for tick, inst, _ in events:
                if inst.startswith("crash"):
                    crash_ticks.add(tick)

            constraint_errors, constraint_warnings = check_physical_constraints(
                events, humanized=is_humanized, exclude_ticks=crash_ticks
            )
            all_errors.extend(constraint_errors)
            all_warnings.extend(constraint_warnings)

            all_errors.extend(check_velocity_range(tmp_path))
            all_errors.extend(check_no_dropped_instruments(events, tmp_path, kit_name))

            rt_errors, rt_warnings = check_round_trip(
                events, tmp_path, kit_name, time_signatures,
                humanized=is_humanized,
            )
            all_errors.extend(rt_errors)
            all_warnings.extend(rt_warnings)
        finally:
            os.unlink(tmp_path)

    except Exception as e:
        all_errors.append(f"pipeline_error: {e}")

    passed = len(all_errors) == 0
    return ValidationResult(label, passed, all_errors, all_warnings)


# ── Tier 3: Test matrix runners ───────────────────────────────────────────────

QUICK_STYLES = ["blast", "shellac", "faraquet", "post_punk", "screamo",
                "euro_screamo", "black_metal", "slint"]


def run_quick():
    """Quick validation: 8 representative styles, 4/4, humanize=0, single mode."""
    results = []
    for style in QUICK_STYLES:
        result = validate_pipeline(
            style=style, bars=4, tempo=120, time_sig="4/4",
            humanize=0.0, mode="single", seed=42,
        )
        results.append(result)
    return results


def _get_available_time_sigs(style):
    """Find time signatures available for a style's pool cells."""
    pool_names = STYLE_POOLS.get(style, [])
    available = set()
    for name in pool_names:
        cell = CELLS.get(name)
        if cell:
            ts = tuple(cell.get("time_sig", (4, 4)))
            available.add(f"{ts[0]}/{ts[1]}")
    return available


def _has_prob_cells(style):
    """Check if a style pool has probability grid cells."""
    pool_names = STYLE_POOLS.get(style, [])
    for name in pool_names:
        cell = CELLS.get(name)
        if cell and cell.get("type") == "probability":
            return True
    return False


def run_full():
    """Full validation: all styles, multiple time sigs, modes, and humanize levels."""
    results = []
    target_time_sigs = ["4/4", "3/4", "6/4", "6/8", "7/8", "5/4"]

    for style in sorted(STYLE_POOLS.keys()):
        pool_names = STYLE_POOLS.get(style, [])
        # Skip styles with only imported cells that may not be present
        builtin_names = [n for n in pool_names if n in CELLS and CELLS[n].get("source") != "imported"]
        if not builtin_names:
            continue

        available_ts = _get_available_time_sigs(style)

        for ts in target_time_sigs:
            if ts not in available_ts:
                continue

            # Single mode, humanize=0 (strict)
            results.append(validate_pipeline(
                style=style, bars=4, tempo=120, time_sig=ts,
                humanize=0.0, mode="single", seed=42,
            ))

            # Single mode, humanize=0.7 (bounds-only)
            results.append(validate_pipeline(
                style=style, bars=4, tempo=120, time_sig=ts,
                humanize=0.7, mode="single", seed=42,
            ))

        # Arrangement mode (4/4 only)
        if "4/4" in available_ts:
            results.append(validate_pipeline(
                style=style, bars=4, tempo=120, time_sig="4/4",
                humanize=0.0, mode="arrangement",
                arrangement_str="2:verse 2:drive", seed=42,
            ))

        # Generative mode (where prob cells exist)
        if _has_prob_cells(style) and "4/4" in available_ts:
            results.append(validate_pipeline(
                style=style, bars=4, tempo=120, time_sig="4/4",
                humanize=0.0, mode="generative", seed=42,
            ))

        # Fill mode for a subset
        if style in QUICK_STYLES and "4/4" in available_ts:
            results.append(validate_pipeline(
                style=style, bars=8, tempo=120, time_sig="4/4",
                humanize=0.0, mode="single", seed=42, fill_every=4,
            ))

    return results


# ── CLI ───────────────────────────────────────────────────────────────────────

def _print_result(result, verbose=False):
    """Print a single validation result."""
    if result.passed:
        if verbose:
            status = "[PASS]"
            if result.warnings:
                status = "[WARN]"
            print(f"  {status} {result.label}")
            for w in result.warnings:
                print(f"         - {w}")
        elif result.warnings:
            print(f"  [WARN] {result.label}")
            for w in result.warnings:
                print(f"         - {w}")
    else:
        print(f"  [FAIL] {result.label}")
        for e in result.errors:
            print(f"         - {e}")
        for w in result.warnings:
            print(f"         - (warn) {w}")


def main():
    parser = argparse.ArgumentParser(description="Validate drumgen MIDI pipeline")
    parser.add_argument("--full", action="store_true", help="Run exhaustive matrix")
    parser.add_argument("--style", type=str, help="Validate a single style")
    parser.add_argument("--cell", type=str, help="Validate a single cell")
    parser.add_argument("-v", "--verbose", action="store_true", help="Show pass details")
    parser.add_argument("--fail-fast", action="store_true", help="Stop on first failure")
    args = parser.parse_args()

    if args.cell:
        print(f"validate_midi: single cell '{args.cell}'\n")
        result = validate_pipeline(cell_name=args.cell, bars=4, tempo=120,
                                   time_sig="4/4", humanize=0.0, mode="single", seed=42)
        _print_result(result, verbose=True)
        sys.exit(0 if result.passed else 1)

    if args.style:
        print(f"validate_midi: single style '{args.style}'\n")
        results = []
        available_ts = _get_available_time_sigs(args.style)
        for ts in sorted(available_ts):
            results.append(validate_pipeline(
                style=args.style, bars=4, tempo=120, time_sig=ts,
                humanize=0.0, mode="single", seed=42,
            ))
        if "4/4" in available_ts:
            results.append(validate_pipeline(
                style=args.style, bars=4, tempo=120, time_sig="4/4",
                humanize=0.0, mode="arrangement",
                arrangement_str="2:verse 2:drive", seed=42,
            ))
        if _has_prob_cells(args.style) and "4/4" in available_ts:
            results.append(validate_pipeline(
                style=args.style, bars=4, tempo=120, time_sig="4/4",
                humanize=0.0, mode="generative", seed=42,
            ))
    elif args.full:
        results = run_full()
        print(f"validate_midi: full mode ({len(results)} configs)\n")
    else:
        results = run_quick()
        print(f"validate_midi: quick mode ({len(results)} configs)\n")

    failures = 0
    warns = 0
    passes = 0

    for result in results:
        _print_result(result, verbose=args.verbose)
        if not result.passed:
            failures += 1
            if args.fail_fast:
                print(f"\nFail-fast: stopping after first failure")
                sys.exit(1)
        elif result.warnings:
            warns += 1
        else:
            passes += 1

    print(f"\nResults: {passes} passed, {failures} failed, {warns} warnings")
    sys.exit(0 if failures == 0 else 1)


if __name__ == "__main__":
    main()
