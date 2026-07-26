#!/usr/bin/env python3
"""notation.py — drum .mid → standard-notation MusicXML for a human drummer.

Pipeline: plugin SAVE .MID (or CLI output) → this tool → .musicxml →
MuseScore 4 → PDF. The engine's whole vocabulary lives on a 16th grid, so
snapping every tick to the nearest sixteenth is lossless (humanize jitter is
well under half a sixteenth) and the score contains no tuplets: MusicXML
``divisions=4`` (one division = one 16th) represents everything exactly.

Two voices, drummer-standard: hands (snare/toms/cymbals, stems up) and feet
(kick/pedal hat, stems down). Ghost snares render in parentheses, cymbals as
x noteheads, open hats circle-x, accents marked. Meter changes get a fresh
time signature.

Usage:
    python notation.py in.mid -o out.musicxml [--kit ugritone]
Then in MuseScore 4: open, File → Export → PDF (or headless:
``MuseScore4 --export-to out.pdf out.musicxml``).

Stdlib + mido only (mido is already a drumgen dependency, used read-only).
"""

import argparse
import json
import os
import sys
import xml.etree.ElementTree as ET

import mido

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from midi_reader import _build_reverse_mapping, _classify_velocity

KIT_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "kit_mappings")

# Feet go stems-down in voice 2; everything else is hands, voice 1.
FEET = {"kick", "hihat_pedal"}

# Cymbal-family instruments render with x noteheads.
CYMBALS = {
    "hihat_closed", "hihat_open", "hihat_wide_open", "ride", "ride_bell",
    "ride_crash", "crash_1", "crash_2", "crash_1_choke", "crash_2_choke",
    "china", "china_2", "splash", "fx_cymbal_1", "fx_cymbal_2",
}

# Staff placement (display-step/display-octave) on a 5-line percussion staff,
# following common MuseScore/Gould conventions. MuseScore itself re-maps via
# <midi-unpitched> into its own drumset, so these mostly matter for other
# readers (Dorico, TuxGuitar) — emit both, always.
DISPLAY = {
    "kick": ("F", 4),
    "hihat_pedal": ("D", 4),
    "snare": ("C", 5),
    "snare_rim": ("C", 5),
    "snare_ghost": ("C", 5),
    "tom_high": ("E", 5),
    "tom_mid_high": ("D", 5),
    "tom_mid": ("D", 5),
    "tom_low": ("A", 4),
    "tom_floor": ("G", 4),
    "hihat_closed": ("G", 5),
    "hihat_open": ("G", 5),
    "hihat_wide_open": ("G", 5),
    "ride": ("F", 5),
    "ride_bell": ("F", 5),
    "ride_crash": ("F", 5),
    "crash_1": ("A", 5),
    "crash_1_choke": ("A", 5),
    "crash_2": ("A", 5),
    "crash_2_choke": ("A", 5),
    "china": ("B", 5),
    "china_2": ("B", 5),
    "splash": ("B", 5),
    "fx_cymbal_1": ("B", 5),
    "fx_cymbal_2": ("B", 5),
}

# Note durations expressible without ties, in sixteenth units.
# type name per MusicXML; dot flag.
DURATIONS = [
    (16, "whole", False),
    (12, "half", True),
    (8, "half", False),
    (6, "quarter", True),
    (4, "quarter", False),
    (3, "eighth", True),
    (2, "eighth", False),
    (1, "16th", False),
]


def load_kit(kit_name):
    path = os.path.join(KIT_DIR, f"{kit_name}.json")
    with open(path) as f:
        return json.load(f)


def read_events(midi_path, kit_name="ugritone"):
    """Parse an SMF into snapped, classified events + a meter map.

    Returns (events, meters, ppq, tempo_bpm) where events is a list of
    (slot, instrument, level) with slot = global sixteenth index from tick 0,
    and meters is a list of (start_slot, numerator, denominator).
    """
    mid = mido.MidiFile(midi_path)
    ppq = mid.ticks_per_beat
    sixteenth = max(1, ppq // 4)
    kit = load_kit(kit_name)
    reverse = _build_reverse_mapping(kit)
    velocity_ranges = kit.get("velocity_ranges", {
        "accent": [106, 127], "normal": [76, 105],
        "soft": [51, 75], "ghost": [1, 50],
    })

    events = []
    meters = []
    tempo_bpm = 120.0
    for track in mid.tracks:
        tick = 0
        for msg in track:
            tick += msg.time
            if msg.type == "time_signature":
                slot = round(tick / sixteenth)
                meters.append((slot, msg.numerator, msg.denominator))
            elif msg.type == "set_tempo":
                tempo_bpm = mido.tempo2bpm(msg.tempo)
            elif msg.type == "note_on" and msg.velocity > 0:
                inst = reverse.get(msg.note)
                if inst is None:
                    continue
                level = _classify_velocity(msg.velocity, velocity_ranges)
                # Snap to the nearest sixteenth — lossless for this engine.
                events.append((round(tick / sixteenth), inst, level))

    if not meters:
        meters = [(0, 4, 4)]
    meters.sort()
    events.sort()
    return events, meters, ppq, tempo_bpm


def build_measures(events, meters):
    """Slice the global slot stream into measures.

    Returns a list of measures: (num, den, slots_per_bar, {slot_in_bar:
    [(instrument, level), ...]}). Runs until the last event is consumed.
    """
    if not events:
        return []
    last_slot = events[-1][0]
    measures = []
    ei = 0
    slot_cursor = 0
    mi = 0
    while slot_cursor <= last_slot:
        # Active meter: the latest meter entry at or before this measure.
        while mi + 1 < len(meters) and meters[mi + 1][0] <= slot_cursor:
            mi += 1
        _, num, den = meters[mi]
        spb = num * 16 // den  # sixteenths per bar
        bar_events = {}
        end = slot_cursor + spb
        while ei < len(events) and events[ei][0] < end:
            slot, inst, level = events[ei]
            bar_events.setdefault(slot - slot_cursor, []).append((inst, level))
            ei += 1
        measures.append((num, den, spb, bar_events))
        slot_cursor = end
    return measures


def _segments(onsets, spb, beat_len):
    """Split a voice's bar into (start, length, is_note) segments.

    Notes last until the next onset or the end of their beat; the remaining
    space is packed with rests that never cross a beat boundary (so the beat
    structure stays readable).
    """
    segs = []
    cursor = 0
    onset_list = sorted(onsets)
    for i, start in enumerate(onset_list):
        # Rests from cursor to this onset, split at beat boundaries.
        while cursor < start:
            beat_end = (cursor // beat_len + 1) * beat_len
            seg_end = min(start, beat_end)
            segs.append((cursor, seg_end - cursor, False))
            cursor = seg_end
        nxt = onset_list[i + 1] if i + 1 < len(onset_list) else spb
        beat_end = (start // beat_len + 1) * beat_len
        length = min(nxt, beat_end) - start
        segs.append((start, max(1, length), True))
        cursor = start + max(1, length)
    while cursor < spb:
        beat_end = (cursor // beat_len + 1) * beat_len
        seg_end = min(spb, beat_end)
        segs.append((cursor, seg_end - cursor, False))
        cursor = seg_end
    return segs


def _emit_duration(parent, length, voice_num, is_rest, spb):
    """Append duration/type(/dot) children; decompose odd lengths greedily."""
    for units, tname, dotted in DURATIONS:
        if units == length:
            ET.SubElement(parent, "duration").text = str(length)
            ET.SubElement(parent, "voice").text = str(voice_num)
            t = ET.SubElement(parent, "type")
            t.text = tname
            if dotted:
                ET.SubElement(parent, "dot")
            return True
    return False


def _greedy_lengths(length):
    out = []
    remaining = length
    while remaining > 0:
        for units, _, _ in DURATIONS:
            if units <= remaining:
                out.append(units)
                remaining -= units
                break
    return out


def _note_elem(measure, inst, level, length, voice_num, chord, stem_dir):
    note = ET.SubElement(measure, "note")
    if chord:
        ET.SubElement(note, "chord")
    unp = ET.SubElement(note, "unpitched")
    step, octave = DISPLAY.get(inst, ("C", 5))
    ET.SubElement(unp, "display-step").text = step
    ET.SubElement(unp, "display-octave").text = str(octave)
    ET.SubElement(note, "duration").text = str(length)
    ET.SubElement(note, "voice").text = str(voice_num)
    for units, tname, dotted in DURATIONS:
        if units == length:
            ET.SubElement(note, "type").text = tname
            if dotted:
                ET.SubElement(note, "dot")
            break
    ET.SubElement(note, "stem").text = stem_dir
    ET.SubElement(note, "instrument", {"id": f"P1-I{INSTRUMENT_IDS[inst]}"})
    if inst in CYMBALS:
        nh = ET.SubElement(note, "notehead")
        nh.text = "circle-x" if inst in ("hihat_open", "hihat_wide_open") else "x"
    if inst == "snare_ghost" or level == "ghost":
        nh = ET.SubElement(note, "notehead", {"parentheses": "yes"})
        nh.text = "normal"
    if level == "accent":
        notations = ET.SubElement(note, "notations")
        artic = ET.SubElement(notations, "articulations")
        ET.SubElement(artic, "accent")
    return note


def _rest_elems(measure, length, voice_num):
    for units in _greedy_lengths(length):
        note = ET.SubElement(measure, "note")
        ET.SubElement(note, "rest")
        ET.SubElement(note, "duration").text = str(units)
        ET.SubElement(note, "voice").text = str(voice_num)
        for u, tname, dotted in DURATIONS:
            if u == units:
                ET.SubElement(note, "type").text = tname
                if dotted:
                    ET.SubElement(note, "dot")
                break


# Stable instrument ids for the part-list (order = DISPLAY key order).
INSTRUMENT_IDS = {inst: i + 1 for i, inst in enumerate(DISPLAY)}


def write_musicxml(measures, out_path, kit_name="ugritone", tempo_bpm=120.0,
                   title="drumgen"):
    """Write score-partwise MusicXML with a single percussion part."""
    kit = load_kit(kit_name)
    mapping = kit["mapping"]

    root = ET.Element("score-partwise", {"version": "4.0"})
    work = ET.SubElement(root, "work")
    ET.SubElement(work, "work-title").text = title

    part_list = ET.SubElement(root, "part-list")
    score_part = ET.SubElement(part_list, "score-part", {"id": "P1"})
    ET.SubElement(score_part, "part-name").text = "Drums"
    for inst, iid in INSTRUMENT_IDS.items():
        si = ET.SubElement(score_part, "score-instrument", {"id": f"P1-I{iid}"})
        ET.SubElement(si, "instrument-name").text = inst.replace("_", " ")
    for inst, iid in INSTRUMENT_IDS.items():
        mi = ET.SubElement(score_part, "midi-instrument", {"id": f"P1-I{iid}"})
        ET.SubElement(mi, "midi-channel").text = "10"
        # MusicXML midi-unpitched is 1-BASED: MIDI note + 1. The classic
        # silent failure — forget this and every drum lands one line off.
        note = mapping.get(inst)
        if note is not None:
            ET.SubElement(mi, "midi-unpitched").text = str(note + 1)

    part = ET.SubElement(root, "part", {"id": "P1"})
    prev_meter = None
    for mnum, (num, den, spb, bar_events) in enumerate(measures, start=1):
        measure = ET.SubElement(part, "measure", {"number": str(mnum)})
        if mnum == 1 or (num, den) != prev_meter:
            attrs = ET.SubElement(measure, "attributes")
            ET.SubElement(attrs, "divisions").text = "4"
            if mnum == 1:
                key = ET.SubElement(attrs, "key")
                ET.SubElement(key, "fifths").text = "0"
            time = ET.SubElement(attrs, "time")
            ET.SubElement(time, "beats").text = str(num)
            ET.SubElement(time, "beat-type").text = str(den)
            if mnum == 1:
                clef = ET.SubElement(attrs, "clef")
                ET.SubElement(clef, "sign").text = "percussion"
                ET.SubElement(clef, "line").text = "2"
        if mnum == 1:
            direction = ET.SubElement(measure, "direction", {"placement": "above"})
            dt = ET.SubElement(direction, "direction-type")
            metro = ET.SubElement(dt, "metronome")
            ET.SubElement(metro, "beat-unit").text = "quarter"
            ET.SubElement(metro, "per-minute").text = str(int(round(tempo_bpm)))
            ET.SubElement(direction, "sound", {"tempo": str(int(round(tempo_bpm)))})
        prev_meter = (num, den)

        beat_len = 2 if den == 8 else 4  # sixteenths per notated beat
        hands = {s: [e for e in evs if e[0] not in FEET]
                 for s, evs in bar_events.items()}
        feet = {s: [e for e in evs if e[0] in FEET]
                for s, evs in bar_events.items()}
        hands = {s: evs for s, evs in hands.items() if evs}
        feet = {s: evs for s, evs in feet.items() if evs}

        for voice_num, (voice_events, stem) in enumerate(
            ((hands, "up"), (feet, "down")), start=1
        ):
            if voice_num == 2:
                backup = ET.SubElement(measure, "backup")
                ET.SubElement(backup, "duration").text = str(spb)
            if not voice_events:
                _rest_elems(measure, spb, voice_num)
                continue
            for start, length, is_note in _segments(set(voice_events), spb, beat_len):
                if not is_note:
                    _rest_elems(measure, length, voice_num)
                    continue
                evs = voice_events[start]
                for lengths_i, note_len in enumerate(_greedy_lengths(length)):
                    if lengths_i == 0:
                        for ci, (inst, level) in enumerate(evs):
                            _note_elem(measure, inst, level, note_len, voice_num,
                                       ci > 0, stem)
                    else:
                        _rest_elems(measure, note_len, voice_num)

    ET.indent(root)
    body = ET.tostring(root, encoding="unicode")
    doctype = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 '
        'Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">\n'
    )
    with open(out_path, "w") as f:
        f.write(doctype + body + "\n")


def mid_to_musicxml(midi_path, out_path, kit_name="ugritone"):
    events, meters, _, tempo_bpm = read_events(midi_path, kit_name)
    measures = build_measures(events, meters)
    if not measures:
        raise SystemExit(f"No drum events found in {midi_path}")
    title = os.path.splitext(os.path.basename(midi_path))[0]
    write_musicxml(measures, out_path, kit_name, tempo_bpm, title)
    return out_path


def main():
    parser = argparse.ArgumentParser(
        description="Convert a drumgen .mid to drummer-readable MusicXML."
    )
    parser.add_argument("midi", help="Input .mid (e.g. from SAVE .MID)")
    parser.add_argument("-o", "--output", help="Output .musicxml path")
    parser.add_argument("--kit", default="ugritone", help="Kit mapping name")
    args = parser.parse_args()
    out = args.output or os.path.splitext(args.midi)[0] + ".musicxml"
    mid_to_musicxml(args.midi, out, args.kit)
    print(f"Wrote {out}\nOpen in MuseScore 4, then File > Export > PDF "
          f"(or: MuseScore4 --export-to out.pdf {out})")


if __name__ == "__main__":
    main()
