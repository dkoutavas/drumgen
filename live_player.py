#!/usr/bin/env python3
"""Real-time MIDI player for drumgen patterns via python-rtmidi.

Phase 0 of the VST3 plugin plan: validates real-time MIDI output from
drumgen's existing Python pipeline to Ableton Live through a virtual
MIDI port (loopMIDI on Windows).

IMPORTANT: This script must run on Windows Python (not WSL Python)
because WSL2 cannot access Windows MIDI devices. Install loopMIDI
(https://www.tobias-erichsen.de/software/loopmidi.html) and create a
port named "drumgen", then point Ableton Live's MIDI input to it.

Usage:
    python live_player.py --style screamo --tempo 180 --bars 4
    python live_player.py --cell blast_traditional --tempo 200 --bars 2
    python live_player.py --style shellac -a "4:verse 4:drive 2:blast" --tempo 130
    python live_player.py --list-ports
    python live_player.py --port "drumgen" --style screamo --tempo 180
"""

import argparse
import signal
import sys
import time

try:
    import rtmidi
except ImportError:
    print("ERROR: python-rtmidi is not installed.")
    print("Install it with:  pip install python-rtmidi")
    print()
    print("On Windows, you may need the Visual C++ Build Tools.")
    print("Alternatively:    pip install python-rtmidi --only-binary :all:")
    sys.exit(1)

from assembler import assemble, assemble_arrangement
from cell_library import STYLE_POOLS, CELLS
from midi_engine import load_kit_mapping, calculate_bar_start_ticks, DEFAULT_PPQ


# MIDI constants for channel 10 (0-indexed channel 9)
NOTE_ON = 0x99
NOTE_OFF = 0x89
CC = 0xB9
ALL_NOTES_OFF_CC = 123
NOTE_DURATION_TICKS = 30


def resolve_midi_note(instrument, kit):
    """Resolve an instrument name to a MIDI note number using kit mapping.

    Checks the main mapping first, then aliases. Returns None for
    unknown instruments (which are silently skipped during playback).
    """
    mapping = kit["mapping"]
    aliases = kit.get("aliases", {})

    inst_lower = instrument.lower()

    # Direct lookup (case-insensitive)
    for key, note in mapping.items():
        if key.lower() == inst_lower:
            return note

    # Alias lookup
    for alias, target in aliases.items():
        if alias.lower() == inst_lower:
            for key, note in mapping.items():
                if key.lower() == target.lower():
                    return note

    return None


def list_midi_ports():
    """List all available MIDI output ports."""
    midi_out = rtmidi.MidiOut()
    ports = midi_out.get_ports()
    del midi_out

    if not ports:
        print("No MIDI output ports found.")
        print()
        print("On Windows, install loopMIDI to create a virtual MIDI port:")
        print("  https://www.tobias-erichsen.de/software/loopmidi.html")
        print("Create a port named 'drumgen' in loopMIDI, then run this script.")
    else:
        print(f"Available MIDI output ports ({len(ports)}):")
        for i, name in enumerate(ports):
            print(f"  [{i}] {name}")
    return ports


def open_midi_port(port_name="drumgen"):
    """Open a MIDI output port by name, or create a virtual port as fallback.

    Returns the opened MidiOut instance, or None on failure.
    """
    midi_out = rtmidi.MidiOut()
    ports = midi_out.get_ports()

    # Search for existing port by name (case-insensitive substring match)
    for i, name in enumerate(ports):
        if port_name.lower() in name.lower():
            midi_out.open_port(i)
            print(f"Opened MIDI port: {name}")
            return midi_out

    # No matching port found — try to create a virtual port
    # Note: virtual ports work on macOS/Linux but NOT on Windows.
    # On Windows, users must use loopMIDI.
    if sys.platform == "win32":
        print(f"ERROR: No MIDI port matching '{port_name}' found.")
        print()
        print("On Windows, virtual MIDI ports cannot be created programmatically.")
        print("Install loopMIDI and create a port named 'drumgen':")
        print("  https://www.tobias-erichsen.de/software/loopmidi.html")
        print()
        if ports:
            print("Available ports:")
            for i, name in enumerate(ports):
                print(f"  [{i}] {name}")
        del midi_out
        return None

    # macOS/Linux: create virtual port
    try:
        midi_out.open_virtual_port(port_name)
        print(f"Created virtual MIDI port: {port_name}")
        return midi_out
    except Exception as e:
        print(f"ERROR: Could not create virtual port '{port_name}': {e}")
        del midi_out
        return None


def prepare_events(result, kit):
    """Convert assembler result events to MIDI-ready events.

    Takes assembler output (abs_tick, instrument, velocity) and resolves
    instrument names to MIDI note numbers.

    Returns sorted list of (abs_tick, midi_note, velocity).
    """
    midi_events = []
    skipped = set()

    for abs_tick, instrument, velocity in result["events"]:
        note = resolve_midi_note(instrument, kit)
        if note is None:
            skipped.add(instrument)
            continue
        midi_events.append((abs_tick, note, velocity))

    midi_events.sort(key=lambda e: (e[0], e[1]))

    if skipped:
        print(f"Warning: skipped unknown instruments: {', '.join(sorted(skipped))}")

    return midi_events


def calculate_total_ticks(result):
    """Calculate the total tick length of the pattern from time signatures."""
    time_sigs = result["time_signatures"]
    last_ts = time_sigs[-1]
    total_bars = last_ts["bar_end"]
    # Total ticks = start tick of (last bar + 1)
    return calculate_bar_start_ticks(total_bars + 1, time_sigs, DEFAULT_PPQ)


def run_playback(midi_out, midi_events, total_ticks, tempo, stop_flag):
    """Main playback loop. Loops the pattern until stop_flag[0] is set.

    Uses time.perf_counter() for high-resolution timing. Detects loop
    wraps and sends all-notes-off on each wrap to prevent stuck notes.
    """
    ticks_per_second = (tempo * DEFAULT_PPQ) / 60.0
    note_duration_seconds = NOTE_DURATION_TICKS / ticks_per_second

    # Sort events by tick for cursor-based playback
    num_events = len(midi_events)
    if num_events == 0:
        print("No events to play.")
        return

    # Calculate total bars for display
    beats_per_bar = 4  # simplified display
    ticks_per_bar = DEFAULT_PPQ * beats_per_bar

    # Track active notes for cleanup
    active_notes = set()
    # Track pending note-offs: list of (off_time_seconds, midi_note)
    pending_offs = []

    cursor = 0  # index into midi_events
    last_effective_tick = 0.0
    loop_count = 0

    start_time = time.perf_counter()

    print(f"\nPlaying... (Ctrl+C to stop)")
    print(f"{'='*50}")

    while not stop_flag[0]:
        now = time.perf_counter()
        elapsed = now - start_time
        current_tick = elapsed * ticks_per_second
        effective_tick = current_tick % total_ticks

        # Detect loop wrap
        if effective_tick < last_effective_tick:
            loop_count += 1
            # Send all-notes-off
            _send_all_notes_off(midi_out, active_notes)
            pending_offs.clear()
            cursor = 0

        # Process pending note-offs
        remaining_offs = []
        for off_time, note in pending_offs:
            if now >= off_time:
                midi_out.send_message([NOTE_OFF, note, 0])
                active_notes.discard(note)
            else:
                remaining_offs.append((off_time, note))
        pending_offs = remaining_offs

        # Emit events in [last_effective_tick, effective_tick) window
        while cursor < num_events:
            event_tick, note, velocity = midi_events[cursor]
            if event_tick < effective_tick:
                if event_tick >= last_effective_tick:
                    # Send note-on
                    midi_out.send_message([NOTE_ON, note, velocity])
                    active_notes.add(note)
                    # Schedule note-off
                    off_time = now + note_duration_seconds
                    pending_offs.append((off_time, note))
                cursor += 1
            else:
                break

        # Update bar display
        current_bar = int(effective_tick / ticks_per_bar) + 1
        total_bars_display = max(1, int(total_ticks / ticks_per_bar))
        sys.stdout.write(
            f"\r  Loop {loop_count + 1} | Bar {current_bar:>3}/{total_bars_display}  "
        )
        sys.stdout.flush()

        last_effective_tick = effective_tick

        # Sleep ~1ms to avoid busy-waiting while keeping latency low
        time.sleep(0.001)

    # Clean up any remaining notes
    _send_all_notes_off(midi_out, active_notes)
    print()


def _send_all_notes_off(midi_out, active_notes):
    """Send note-off for all active notes, then all-notes-off CC."""
    for note in list(active_notes):
        midi_out.send_message([NOTE_OFF, note, 0])
    active_notes.clear()
    midi_out.send_message([CC, ALL_NOTES_OFF_CC, 0])


def generate_pattern(args):
    """Generate a drum pattern using the assembler, based on CLI args.

    Returns the assembler result dict, or None on error.
    """
    try:
        if args.arrangement:
            if not args.style:
                print("ERROR: --arrangement requires --style")
                return None
            result = assemble_arrangement(
                style=args.style,
                arrangement_str=args.arrangement,
                tempo=args.tempo,
                time_sig=args.time_sig,
                humanize=args.humanize,
                swing=args.swing,
                seed=args.seed,
                generative=args.generative,
            )
        else:
            result = assemble(
                style=args.style,
                cell_name=args.cell,
                bars=args.bars,
                tempo=args.tempo,
                time_sig=args.time_sig,
                humanize=args.humanize,
                swing=args.swing,
                seed=args.seed,
                generative=args.generative,
            )
        return result
    except ValueError as e:
        print(f"ERROR: {e}")
        return None


def main():
    parser = argparse.ArgumentParser(
        description="drumgen live MIDI player — real-time output to virtual MIDI port",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""\
examples:
  %(prog)s --style screamo --tempo 180 --bars 4
  %(prog)s --cell blast_traditional --tempo 200 --bars 2
  %(prog)s --style shellac -a "4:verse 4:drive 2:blast" --tempo 130
  %(prog)s --list-ports
  %(prog)s --port "drumgen" --style screamo --tempo 180 --bars 4

NOTE: This script must run on Windows Python (not WSL Python)
because WSL2 cannot access Windows MIDI devices.
""",
    )

    # MIDI port options
    parser.add_argument("--list-ports", action="store_true",
                        help="List available MIDI output ports and exit")
    parser.add_argument("--port", type=str, default="drumgen",
                        help="MIDI port name to use (default: 'drumgen')")

    # Pattern options (mirror drumgen.py)
    parser.add_argument("--style", "-s", type=str,
                        help="Style name (screamo, shellac, faraquet, etc.)")
    parser.add_argument("--cell", type=str,
                        help="Exact cell name (overrides --style)")
    parser.add_argument("--tempo", "-t", type=int, default=120,
                        help="BPM (default: 120)")
    parser.add_argument("--bars", "-b", type=int, default=4,
                        help="Number of bars (default: 4)")
    parser.add_argument("--time-sig", "-ts", type=str, default="4/4",
                        help="Time signature as N/D (default: 4/4)")
    parser.add_argument("--humanize", type=float, default=None,
                        help="Humanization 0.0-1.0 (default: per-cell)")
    parser.add_argument("--swing", type=float, default=0.0,
                        help="Swing amount 0.0-1.0 (default: 0.0)")
    parser.add_argument("--seed", type=int, default=None,
                        help="Random seed for reproducibility")
    parser.add_argument("--arrangement", "-a", type=str, default=None,
                        help='Arrangement: "4:verse 4:drive 2:blast"')
    parser.add_argument("--generative", "-g", action="store_true",
                        help="Use probability grids for generative patterns")
    parser.add_argument("--kit", type=str, default="ugritone",
                        help="Kit mapping name (default: ugritone)")

    args = parser.parse_args()

    # --list-ports mode
    if args.list_ports:
        list_midi_ports()
        return

    # Need style or cell to generate
    if not args.style and not args.cell and not args.arrangement:
        parser.error("--style or --cell is required (or use --list-ports)")

    # Generate pattern
    print(f"Generating pattern...")
    result = generate_pattern(args)
    if result is None:
        sys.exit(1)

    # Load kit mapping
    try:
        kit = load_kit_mapping(args.kit)
    except FileNotFoundError as e:
        print(f"ERROR: {e}")
        sys.exit(1)

    # Prepare MIDI events
    midi_events = prepare_events(result, kit)
    total_ticks = calculate_total_ticks(result)
    tempo = result["tempo"]

    # Print pattern info
    label = args.cell or args.style or "arrangement"
    total_bars = result["time_signatures"][-1]["bar_end"]
    print()
    print(f"  Pattern:  {label}")
    print(f"  Tempo:    {tempo} BPM")
    print(f"  Bars:     {total_bars}")
    print(f"  Time sig: {args.time_sig}")
    print(f"  Seed:     {result['seed']}")
    print(f"  Events:   {len(midi_events)}")
    print(f"  Duration: {total_ticks} ticks ({total_ticks / ((tempo * DEFAULT_PPQ) / 60.0):.1f}s per loop)")

    if args.arrangement and "section_summary" in result:
        print(f"  Sections: {result['section_summary']}")

    # Open MIDI port
    midi_out = open_midi_port(args.port)
    if midi_out is None:
        sys.exit(1)

    # Use a mutable flag so the signal handler can stop the playback loop
    stop_flag = [False]

    def shutdown(signum=None, frame=None):
        stop_flag[0] = True

    signal.signal(signal.SIGINT, shutdown)

    # Run playback loop
    run_playback(midi_out, midi_events, total_ticks, tempo, stop_flag)

    print("Stopping playback...")
    midi_out.close_port()
    print("MIDI port closed.")


if __name__ == "__main__":
    main()
