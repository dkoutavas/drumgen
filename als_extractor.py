#!/usr/bin/env python3
"""Extract MIDI clips from Ableton Live .als files.

Standalone CLI. Opens .als (gzip-compressed XML), finds all MidiClip
elements, writes each as a .mid file.

Usage:
    python als_extractor.py project.als -o extracted/ --drums-only --verbose
    python als_extractor.py /path/to/projects/ --recursive --drums-only
    python als_extractor.py project.als --dry-run
"""

import argparse
import gzip
import os
import sys
import xml.etree.ElementTree as ET

import mido

# GM drum note range (roughly)
DRUM_NOTE_MIN = 35
DRUM_NOTE_MAX = 81

# Track name patterns that suggest drums
DRUM_NAME_PATTERNS = [
    "drum", "drums", "perc", "percussion", "kit", "beat", "beats",
    "snare", "kick", "hihat", "hi-hat", "cymbal", "tom",
]


def _is_drum_track_name(name):
    """Check if track name suggests drums."""
    name_lower = name.lower()
    return any(p in name_lower for p in DRUM_NAME_PATTERNS)


def _notes_in_drum_range(notes, threshold=0.7):
    """Check if most notes fall in GM drum range."""
    if not notes:
        return False
    in_range = sum(1 for n in notes if DRUM_NOTE_MIN <= n <= DRUM_NOTE_MAX)
    return (in_range / len(notes)) >= threshold


def _parse_als(als_path):
    """Parse an .als file (gzip XML) and return the XML root."""
    try:
        with gzip.open(als_path, "rb") as f:
            tree = ET.parse(f)
    except gzip.BadGzipFile:
        raise ValueError(
            f"Not a gzip XML .als file (likely Ableton Live 8 or earlier binary format). "
            f"Only Live 9+ projects are supported."
        )
    return tree.getroot()


def _extract_tempo(root):
    """Extract tempo from the .als XML. Returns BPM float or 120.0 default."""
    # Try multiple paths for different Ableton versions
    tempo_paths = [
        ".//Tempo/Manual",
        ".//MasterTrack/DeviceChain/Mixer/Tempo/Manual",
        ".//LiveSet/MasterTrack/DeviceChain/Mixer/Tempo/Manual",
    ]
    for path in tempo_paths:
        elem = root.find(path)
        if elem is not None:
            val = elem.get("Value")
            if val:
                try:
                    return float(val)
                except ValueError:
                    pass
    return 120.0


def _extract_time_signature(root):
    """Extract time signature. Returns (numerator, denominator) or (4, 4)."""
    # Try paths in priority order — MasterTrack global TS first
    ts_paths = [
        ".//MasterTrack//RemoteableTimeSignature",
        ".//RemoteableTimeSignature",
        ".//MasterTrack/TimeSignature",
        ".//TimeSignature",
    ]
    for path in ts_paths:
        elem = root.find(path)
        if elem is None:
            continue
        num_elem = elem.find("Numerator")
        if num_elem is None:
            num_elem = elem.find(".//Numerator")
        den_elem = elem.find("Denominator")
        if den_elem is None:
            den_elem = elem.find(".//Denominator")
        if num_elem is not None and den_elem is not None:
            # In .als, these may have Value attrs or be direct values
            num_val = num_elem.get("Value") or num_elem.text
            den_val = den_elem.get("Value") or den_elem.text
            if num_val and den_val:
                try:
                    return int(num_val), int(den_val)
                except ValueError:
                    pass
    return 4, 4


def _extract_clips_from_track(track_elem, track_name, track_idx):
    """Extract all MidiClip elements from a track.

    Searches both Session view (ClipSlotList) and Arrangement view
    (ArrangerAutomation/Events).
    """
    clips = []
    for clip_elem in track_elem.findall(".//MidiClip"):
        clip_info = _parse_midi_clip(clip_elem, track_name, track_idx)
        if clip_info:
            clips.append(clip_info)
    return clips


def _parse_midi_clip(clip_elem, track_name, track_idx):
    """Parse a MidiClip element into a dict with notes and metadata."""
    # Get clip name
    name_elem = clip_elem.find("Name")
    clip_name = ""
    if name_elem is not None:
        val = name_elem.get("Value")
        if val:
            clip_name = val

    if not clip_name:
        # Generate a descriptive name from clip's beat position
        start_elem = clip_elem.find("CurrentStart")
        if start_elem is not None:
            start_val = start_elem.get("Value", "0")
            clip_name = f"beat_{start_val.replace('.', '_')}"
        else:
            clip_name = "clip"

    # Get loop length for determining bar count
    loop_end = 4.0  # default 1 bar in 4/4
    loop_elem = clip_elem.find(".//Loop")
    if loop_elem is not None:
        end_elem = loop_elem.find("LoopEnd")
        if end_elem is not None:
            val = end_elem.get("Value")
            if val:
                try:
                    loop_end = float(val)
                except ValueError:
                    pass

    # Check if clip is disabled
    disabled_elem = clip_elem.find("Disabled")
    if disabled_elem is not None:
        val = disabled_elem.get("Value")
        if val and val.lower() == "true":
            return None

    # Extract notes
    notes = []
    # Ableton 10+ uses KeyTracks structure
    for key_track in clip_elem.findall(".//KeyTracks/KeyTrack"):
        midi_key_elem = key_track.find("MidiKey")
        if midi_key_elem is None:
            continue
        midi_key = midi_key_elem.get("Value")
        if midi_key is None:
            continue
        try:
            note_num = int(midi_key)
        except ValueError:
            continue

        for note_elem in key_track.findall(".//MidiNoteEvent"):
            # Check if note is enabled
            is_enabled = note_elem.get("IsEnabled", "true")
            if is_enabled.lower() == "false":
                continue

            time_val = note_elem.get("Time")
            vel_val = note_elem.get("Velocity")
            dur_val = note_elem.get("Duration")

            if time_val is None or vel_val is None:
                continue

            try:
                time = float(time_val)
                velocity = int(round(float(vel_val)))
                duration = float(dur_val) if dur_val else 0.1
            except (ValueError, TypeError):
                continue

            velocity = max(1, min(127, velocity))
            notes.append({
                "note": note_num,
                "time": time,
                "velocity": velocity,
                "duration": duration,
            })

    # Also try Notes/MidiNoteEvent (Ableton 11+ flat structure)
    if not notes:
        for note_elem in clip_elem.findall(".//Notes/MidiNoteEvent"):
            is_enabled = note_elem.get("IsEnabled", "true")
            if is_enabled.lower() == "false":
                continue

            note_val = note_elem.get("NoteId")
            time_val = note_elem.get("Time")
            vel_val = note_elem.get("Velocity")
            pitch_val = note_elem.get("Pitch")

            if pitch_val is None or time_val is None or vel_val is None:
                continue

            try:
                note_num = int(pitch_val)
                time = float(time_val)
                velocity = int(round(float(vel_val)))
            except (ValueError, TypeError):
                continue

            velocity = max(1, min(127, velocity))
            notes.append({
                "note": note_num,
                "time": time,
                "velocity": velocity,
                "duration": 0.1,
            })

    if not notes:
        return None

    return {
        "track_name": track_name,
        "track_idx": track_idx,
        "clip_name": clip_name,
        "loop_end": loop_end,
        "notes": notes,
    }


def _clip_to_midi(clip_info, tempo, time_sig_num, time_sig_den, ppq=480):
    """Convert a clip dict to a mido MidiFile."""
    mid = mido.MidiFile(type=0, ticks_per_beat=ppq)
    track = mido.MidiTrack()
    mid.tracks.append(track)

    track.append(mido.MetaMessage("set_tempo", tempo=mido.bpm2tempo(tempo), time=0))
    # mido expects actual denominator (must be power of 2); it converts to log2 internally
    track.append(mido.MetaMessage(
        "time_signature", numerator=time_sig_num, denominator=time_sig_den,
        clocks_per_click=24, notated_32nd_notes_per_beat=8, time=0,
    ))

    # Ableton uses beat positions (1 beat = 1 quarter note in the XML)
    # Convert to ticks
    events = []
    for n in clip_info["notes"]:
        abs_tick = int(n["time"] * ppq)
        dur_ticks = max(1, int(n["duration"] * ppq))
        events.append(("note_on", abs_tick, n["note"], n["velocity"]))
        events.append(("note_off", abs_tick + dur_ticks, n["note"], 0))

    events.sort(key=lambda e: (e[1], 0 if e[0] == "note_on" else 1, e[2]))

    current_tick = 0
    for event_type, abs_tick, note, vel in events:
        delta = max(0, abs_tick - current_tick)
        msg = mido.Message(event_type, note=note, velocity=vel, channel=9, time=delta)
        track.append(msg)
        current_tick = abs_tick

    track.append(mido.MetaMessage("end_of_track", time=0))
    return mid


def _sanitize_filename(name):
    """Remove or replace characters unsafe for filenames."""
    keep = set("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-")
    return "".join(c if c in keep else "_" for c in name).strip("_")


def extract_als(als_path, output_dir=None, drums_only=False, dry_run=False,
                verbose=False):
    """Extract MIDI clips from an .als file.

    Args:
        als_path: Path to .als file.
        output_dir: Output directory (default: extracted/{project_name}/).
        drums_only: Only extract clips that look like drum tracks.
        dry_run: List clips without writing files.
        verbose: Print detailed info.

    Returns:
        List of output file paths (empty if dry_run).
    """
    project_name = os.path.splitext(os.path.basename(als_path))[0]
    project_name = _sanitize_filename(project_name)

    if output_dir is None:
        output_dir = os.path.join("extracted", project_name)

    if verbose:
        print(f"Parsing: {als_path}")

    root = _parse_als(als_path)
    tempo = _extract_tempo(root)
    time_sig_num, time_sig_den = _extract_time_signature(root)

    if verbose:
        print(f"  Tempo: {tempo} BPM")
        print(f"  Time sig: {time_sig_num}/{time_sig_den}")

    # Find all MIDI tracks
    all_clips = []
    track_idx = 0

    for track_elem in root.findall(".//MidiTrack"):
        # Get track name
        name_elem = track_elem.find("Name/EffectiveName")
        if name_elem is None:
            name_elem = track_elem.find("Name/UserName")
        track_name = "Track"
        if name_elem is not None:
            val = name_elem.get("Value")
            if val:
                track_name = val

        clips = _extract_clips_from_track(track_elem, track_name, track_idx)

        for clip in clips:
            is_drum = False
            if _is_drum_track_name(track_name):
                is_drum = True
            elif _notes_in_drum_range([n["note"] for n in clip["notes"]]):
                is_drum = True

            clip["is_drum"] = is_drum

            if drums_only and not is_drum:
                if verbose:
                    print(f"  Skipping non-drum: {track_name}/{clip['clip_name']}")
                continue

            all_clips.append(clip)

        track_idx += 1

    if verbose:
        print(f"  Found {len(all_clips)} clip(s)")

    if not all_clips:
        print(f"No {'drum ' if drums_only else ''}clips found in {als_path}")
        return []

    if dry_run:
        print(f"\nClips in {project_name} ({tempo} BPM, {time_sig_num}/{time_sig_den}):")
        for clip in all_clips:
            drum_tag = " [DRUM]" if clip.get("is_drum") else ""
            note_count = len(clip["notes"])
            bars = clip["loop_end"] / time_sig_num
            print(f"  {clip['track_name']}/{clip['clip_name']}: "
                  f"{note_count} notes, ~{bars:.1f} bars{drum_tag}")
        return []

    # Write MIDI files
    os.makedirs(output_dir, exist_ok=True)
    output_paths = []
    used_filenames = {}

    for clip in all_clips:
        track_safe = _sanitize_filename(clip["track_name"])
        clip_safe = _sanitize_filename(clip["clip_name"]) or "clip"
        base = f"{track_safe}_{clip_safe}"
        count = used_filenames.get(base, 0) + 1
        used_filenames[base] = count
        filename = f"{base}_{count:03d}.mid"
        filepath = os.path.join(output_dir, filename)

        mid = _clip_to_midi(clip, tempo, time_sig_num, time_sig_den)
        mid.save(filepath)
        output_paths.append(filepath)

        if verbose:
            drum_tag = " [DRUM]" if clip.get("is_drum") else ""
            print(f"  Wrote: {filepath} ({len(clip['notes'])} notes){drum_tag}")

    print(f"Extracted {len(output_paths)} clip(s) to {output_dir}/")
    return output_paths


def main():
    parser = argparse.ArgumentParser(
        description="Extract MIDI clips from Ableton Live .als files"
    )
    parser.add_argument("input", help="Path to .als file or directory of .als files")
    parser.add_argument("--output", "-o", type=str, default=None,
                        help="Output directory (default: extracted/{project}/)")
    parser.add_argument("--drums-only", action="store_true",
                        help="Only extract drum tracks")
    parser.add_argument("--recursive", "-r", action="store_true",
                        help="Recursively search directory for .als files")
    parser.add_argument("--dry-run", action="store_true",
                        help="List clips without writing files")
    parser.add_argument("--verbose", "-v", action="store_true",
                        help="Verbose output")

    args = parser.parse_args()

    if os.path.isdir(args.input):
        if args.recursive:
            als_files = []
            for dirpath, _, filenames in os.walk(args.input):
                for fn in filenames:
                    if fn.lower().endswith(".als"):
                        als_files.append(os.path.join(dirpath, fn))
        else:
            als_files = [
                os.path.join(args.input, f) for f in os.listdir(args.input)
                if f.lower().endswith(".als")
            ]

        als_files.sort()
        if not als_files:
            print(f"No .als files found in {args.input}")
            return

        print(f"Found {len(als_files)} .als file(s)\n")
        for als_path in als_files:
            try:
                extract_als(
                    als_path,
                    output_dir=args.output,
                    drums_only=args.drums_only,
                    dry_run=args.dry_run,
                    verbose=args.verbose,
                )
            except Exception as e:
                print(f"Error processing {als_path}: {e}", file=sys.stderr)
            print()
    else:
        try:
            extract_als(
                args.input,
                output_dir=args.output,
                drums_only=args.drums_only,
                dry_run=args.dry_run,
                verbose=args.verbose,
            )
        except ValueError as e:
            print(f"Error: {e}", file=sys.stderr)
            sys.exit(1)


if __name__ == "__main__":
    main()
