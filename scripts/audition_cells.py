#!/usr/bin/env python3
"""Render candidate cells for a listen before they join any style pool.

For each candidate: an 8-bar loop of the cell alone, and a short song in a
style it is meant for, with the cell added to that style's pool IN THIS
PROCESS ONLY (nothing is written to cell_library.py). The song shows whether
the cell actually wins the sections it was written for; the sheet lists which
cell played each section. Same toolchain as ab_render.py (FluidSynth + GM
soundfont to MP3, MusicXML and a MuseScore PDF).

    python scripts/audition_cells.py --out /tmp/audition
"""
import argparse
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
sys.path.insert(0, REPO)
sys.path.insert(0, HERE)

import ab_render  # noqa: E402  (render/chart helpers and tool checks)
from assembler import assemble, assemble_arrangement  # noqa: E402
from cell_library import STYLE_POOLS  # noqa: E402
from midi_engine import write_midi  # noqa: E402

# cell, styles it is written for (first one is used for the song), tempo,
# song form, what it is meant to be.
CANDIDATES = [
    ("dc_pulse_intro_4_4", ["fugazi", "posthardcore", "q_and_not_u", "drive_like_jehu", "atdi"],
     140, "4:intro 8:verse 8:chorus 4:outro",
     "DC intro: pedal quarters and cross-stick, the kick answers on even bars, "
     "closed hats open on every 4th bar."),
    ("motorik_pulse_intro_4_4", ["wipers", "post_punk", "preoccupations", "shame"],
     150, "4:intro 8:verse 8:chorus 4:outro",
     "Post-punk intro: floor-tom eighths as the motor, kick on 1 and 3, the rim "
     "answering on 4 every other bar."),
    ("euro_bell_intro_4_4", ["raein", "faraquet"],
     150, "4:intro 8:verse 8:chorus 4:outro",
     "Euro screamo intro: ride-bell pings over a feathered kick, a quiet snare "
     "statement, a ghost-run answer on even bars."),
    ("feedback_wash_outro_4_4", ["blast", "dbeat", "liturgy", "lord_snow"],
     170, "4:intro 8:blast 4:outro",
     "Heavy outro (and intro): a crash on every downbeat left to ring, half-time "
     "kick and snare, a floor-tom swell on every 4th bar."),
]
KIT = ab_render.KIT
SEED = 11


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--out", required=True)
    ap.add_argument("--no-pdf", action="store_true")
    args = ap.parse_args()
    pdf = not args.no_pdf and bool(ab_render.shutil.which("mscore3"))
    os.makedirs(args.out, exist_ok=True)
    kit = os.path.join(REPO, "kit_mappings", f"{KIT}.json")

    sheet = ["# Intro/outro sketches: audition sheet\n",
             "Unheard sketches. None of them is in a style pool yet: adding one also "
             "re-maps which cell each seed picks in the plugin's loop mode for that "
             "style, so that is a decision for after the listen. General MIDI sounds.\n"]
    for name, styles, tempo, form, what in CANDIDATES:
        d = os.path.join(args.out, name)
        os.makedirs(d, exist_ok=True)
        print(name)

        loop = assemble(cell_name=name, bars=8, tempo=tempo, humanize=0.5, seed=SEED)
        mid = os.path.join(d, "loop.mid")
        write_midi(loop["events"], tempo, loop["time_signatures"], kit, mid)
        ab_render.render(mid, os.path.join(d, "loop.mp3"))
        ab_render.chart(mid, os.path.join(d, "loop"), pdf)

        style = styles[0]
        STYLE_POOLS[style].append(name)          # this process only
        try:
            song = assemble_arrangement(style, form, tempo=tempo, humanize=0.5, seed=SEED)
        finally:
            STYLE_POOLS[style].remove(name)
        mid = os.path.join(d, f"song-{style}.mid")
        write_midi(song["events"], tempo, song["time_signatures"], kit, mid)
        ab_render.render(mid, os.path.join(d, f"song-{style}.mp3"))
        ab_render.chart(mid, os.path.join(d, f"song-{style}"), pdf)

        sections = [tok.split(":")[1] for tok in form.split()]
        played = ", ".join(f"{sec} = `{cell}`" for sec, cell in zip(sections, song["section_cells"]))
        won = [sec for sec, cell in zip(sections, song["section_cells"]) if cell == name]
        sheet.append(f"## `{name}`\n")
        sheet.append(f"{what}\n")
        sheet.append(f"Meant for: {', '.join(styles)}. Loop at {tempo} BPM, seed {SEED}.\n")
        sheet.append(f"Song (`{style}`, `{form}`): {played}.\n")
        sheet.append(f"It won: **{', '.join(won) if won else 'nothing'}**.\n")

    with open(os.path.join(args.out, "AUDITION.md"), "w") as f:
        f.write("\n".join(sheet))
    print(f"done -> {args.out}")


if __name__ == "__main__":
    main()
