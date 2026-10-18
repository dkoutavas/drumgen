#!/usr/bin/env python3
"""Render one generated song form per shape, for a listen before the plugin
gets a FORM stepper.

Each case is a shape, a form seed, and a style it suits. The form string is
generated (arrangement_gen), played through the same assemble_arrangement the
CLI uses, and rendered with ab_render's toolchain (FluidSynth + GM soundfont
to MP3, MusicXML and a MuseScore PDF). The sheet prints the form, the cells
that played each section, and where each section starts in the audio.

    python scripts/audition_forms.py --out /tmp/forms
"""
import argparse
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
sys.path.insert(0, REPO)
sys.path.insert(0, HERE)

import ab_render  # noqa: E402  (render/chart helpers and tool checks)
import arrangement_gen as ag  # noqa: E402
from assembler import assemble_arrangement, parse_arrangement  # noqa: E402
from midi_engine import write_midi  # noqa: E402

# shape, form seed, style, tempo, what to listen for. Seeds were picked so
# each form shows its shape's signature (a bridge, a meter turn, three gaps).
CASES = [
    ("verse_chorus", 1, "atdi", 165,
     "Two verse/chorus rounds, then the bridge (atmospheric) and a last chorus. "
     "Does the bridge earn the last chorus, or is it a dip for its own sake?"),
    ("arc", 6, "posthardcore", 140,
     "The 8-bar build, a fill, then the peak TURNS to 6/8 (posthardcore owns a "
     "6/8 cell). Is a meter turn at the peak a lift or a stumble?"),
    ("stop_go", 1, "screamo", 180,
     "Short bursts and three bars of dead air, then the breakdown and a restart. "
     "Are the gaps a gasp or a dropout?"),
    ("quiet_loud", 5, "euro_screamo", 150,
     "Fragile, erupts for two bars, holds its breath, fragile again, then 8 bars "
     "of wall. Does the first eruption tease or spoil the second?"),
    ("maze", 2, "fugazi", 140,
     "Short sections, fills between, and two turns (5/4 verse, 7/8 drive). "
     "Labyrinth or lost?"),
    ("slow_burn", 8, "daitro", 130,
     "The new shape: 8 bars of intro, verse, an 8-bar build, a drive, one bar of "
     "silence where the climax should land, then 8 bars of comedown. The payoff "
     "never comes, on purpose. Tension or anticlimax?"),
]
KIT = ab_render.KIT
NOTE_SEED = 11


def section_starts(arrangement, tempo):
    """[(section, bar, seconds)] for the sheet."""
    out, bar, secs = [], 1, 0.0
    for count, section, (num, den) in parse_arrangement(arrangement):
        out.append((section, bar, secs))
        bar += count
        secs += count * num * (4 / den) * 60 / tempo
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--out", required=True)
    ap.add_argument("--no-pdf", action="store_true")
    args = ap.parse_args()
    pdf = not args.no_pdf and bool(ab_render.shutil.which("mscore3"))
    os.makedirs(args.out, exist_ok=True)
    kit = os.path.join(REPO, "kit_mappings", f"{KIT}.json")

    sheet = ["# Generated forms: audition sheet\n",
             "One form per shape. The skeleton is generated; every note still comes from "
             "the style's authored cells. General MIDI sounds, so judge the SHAPE: where "
             "it goes, how long it stays, whether the turns and gaps land. "
             f"Note seed {NOTE_SEED} throughout.\n"]
    for shape, form_seed, style, tempo, listen in CASES:
        name = ag.form_name(shape, form_seed)
        arrangement = ag.generate_form(shape, form_seed, style=style)
        d = os.path.join(args.out, f"{shape}-{form_seed}-{style}")
        os.makedirs(d, exist_ok=True)
        print(f"{name}  {style}  {arrangement}")

        song = assemble_arrangement(style, arrangement, tempo=tempo, humanize=0.5, seed=NOTE_SEED)
        mid = os.path.join(d, "form.mid")
        write_midi(song["events"], tempo, song["time_signatures"], kit, mid)
        ab_render.render(mid, os.path.join(d, "form.mp3"))
        ab_render.chart(mid, os.path.join(d, "form"), pdf)

        starts = section_starts(arrangement, tempo)
        rows = [f"| {bar} | {int(t // 60)}:{t % 60:04.1f} | {sec} | `{cell or '(silence)'}` |"
                for (sec, bar, t), cell in zip(starts, song["section_cells"])]
        sheet.append(f"## {ag.FORMS[shape]['label']}: {name} ({style}, {tempo} BPM)\n")
        sheet.append(f"_{ag.FORMS[shape]['about']}_\n")
        sheet.append(f"`python drumgen.py --style {style} --form {shape} --form-seed {form_seed} "
                     f"--seed {NOTE_SEED} --tempo {tempo}`\n")
        sheet.append(f"`{arrangement}` ({sum(b for b, _, _ in parse_arrangement(arrangement))} bars)\n")
        sheet.append("| bar | at | section | cell |\n|---|---|---|---|\n" + "\n".join(rows) + "\n")
        sheet.append(f"Listen for: {listen}\n")

    with open(os.path.join(args.out, "FORMS.md"), "w") as f:
        f.write("\n".join(sheet))
    print(f"done -> {args.out}")


if __name__ == "__main__":
    main()
