import json
import math
import os
import sys

import mido

KIT_MAPPINGS_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "kit_mappings")
DEFAULT_PPQ = 480
NOTE_DURATION = 30  # ticks
MIDI_CHANNEL = 9


def load_kit_mapping(kit_name_or_path):
    if os.path.isfile(kit_name_or_path):
        path = kit_name_or_path
    else:
        path = os.path.join(KIT_MAPPINGS_DIR, f"{kit_name_or_path}.json")
    if not os.path.isfile(path):
        raise FileNotFoundError(f"Kit mapping not found: {path}")
    with open(path, "r") as f:
        return json.load(f)


def get_time_sig_for_bar(bar_number, time_signatures):
    for ts in time_signatures:
        if ts["bar_start"] <= bar_number <= ts["bar_end"]:
            return ts["numerator"], ts["denominator"]
    ts = time_signatures[-1]
    return ts["numerator"], ts["denominator"]


def calculate_bar_start_ticks(bar_number, time_signatures, ppq=DEFAULT_PPQ):
    total_ticks = 0
    for b in range(1, bar_number):
        num, den = get_time_sig_for_bar(b, time_signatures)
        bar_ticks = num * (ppq * 4 // den)
        total_ticks += bar_ticks
    return total_ticks


def position_to_ticks(bar_number, beat, sub, time_signatures, ppq=DEFAULT_PPQ):
    num, den = get_time_sig_for_bar(bar_number, time_signatures)
    bar_start = calculate_bar_start_ticks(bar_number, time_signatures, ppq)
    beat_ticks = ppq * 4 // den
    tick_offset = int(sub * beat_ticks)
    return bar_start + (beat - 1) * beat_ticks + tick_offset


def write_midi(events, tempo, time_signatures, kit_mapping_path, output_path, ppq=DEFAULT_PPQ):
    kit = load_kit_mapping(kit_mapping_path)
    instruments = kit["mapping"]
    inst_lookup = {k.lower(): v for k, v in instruments.items()}

    midi_events = []
    for abs_tick, instrument, velocity in events:
        inst_lower = instrument.lower()
        if inst_lower not in inst_lookup:
            print(f"Warning: Unknown instrument '{instrument}', skipping", file=sys.stderr)
            continue
        note = inst_lookup[inst_lower]
        midi_events.append(("note_on", abs_tick, note, velocity))
        midi_events.append(("note_off", abs_tick + NOTE_DURATION, note, 0))

    midi_events.sort(key=lambda e: (e[1], 0 if e[0] == "note_on" else 1, e[2]))

    mid = mido.MidiFile(type=0, ticks_per_beat=ppq)
    track = mido.MidiTrack()
    mid.tracks.append(track)

    track.append(mido.MetaMessage("set_tempo", tempo=mido.bpm2tempo(tempo), time=0))

    current_tick = 0
    for ts_entry in time_signatures:
        num, den = ts_entry["numerator"], ts_entry["denominator"]
        den_power = int(math.log2(den))
        ts_bar = ts_entry.get("bar_start", 1)
        ts_abs_tick = calculate_bar_start_ticks(ts_bar, time_signatures, ppq)
        delta = max(0, ts_abs_tick - current_tick)
        track.append(mido.MetaMessage("time_signature", numerator=num, denominator=den_power,
                                      clocks_per_click=24, notated_32nd_notes_per_beat=8, time=delta))
        current_tick = ts_abs_tick

    for event_type, abs_tick, note, vel in midi_events:
        delta = max(0, abs_tick - current_tick)
        msg = mido.Message(event_type, note=note, velocity=vel, channel=MIDI_CHANNEL, time=delta)
        track.append(msg)
        current_tick = abs_tick

    track.append(mido.MetaMessage("end_of_track", time=0))

    os.makedirs(os.path.dirname(os.path.abspath(output_path)), exist_ok=True)
    mid.save(output_path)
    print(f"MIDI file saved: {output_path}")
    return output_path


def generate_test_mapping(mapping_name, output_path=None, ppq=DEFAULT_PPQ):
    kit = load_kit_mapping(mapping_name)
    instruments = kit["mapping"]

    if output_path is None:
        output_path = f"output/test_{mapping_name}.mid"

    os.makedirs(os.path.dirname(os.path.abspath(output_path)), exist_ok=True)

    mid = mido.MidiFile(type=0, ticks_per_beat=ppq)
    track = mido.MidiTrack()
    mid.tracks.append(track)

    track.append(mido.MetaMessage("set_tempo", tempo=mido.bpm2tempo(120), time=0))
    track.append(mido.MetaMessage("time_signature", numerator=4, denominator=2,
                                  clocks_per_click=24, notated_32nd_notes_per_beat=8, time=0))

    beat_ticks = ppq
    current_tick = 0

    for i, (name, note) in enumerate(instruments.items()):
        abs_tick = i * beat_ticks
        delta = abs_tick - current_tick

        track.append(mido.Message("note_on", note=note, velocity=100, channel=MIDI_CHANNEL, time=delta))
        track.append(mido.Message("note_off", note=note, velocity=0, channel=MIDI_CHANNEL, time=NOTE_DURATION))
        current_tick = abs_tick + NOTE_DURATION

    track.append(mido.MetaMessage("end_of_track", time=0))

    mid.save(output_path)
    print(f"Test mapping MIDI saved: {output_path}")
    print(f"Instruments ({len(instruments)}):")
    for name, note in instruments.items():
        print(f"  {name}: MIDI note {note}")
    return output_path
