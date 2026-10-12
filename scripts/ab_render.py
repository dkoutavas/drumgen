#!/usr/bin/env python3
"""Render before/after audio of the same take from two versions of the engine.

For when the DAW is not to hand: each case runs the CLI (`drumgen.py`) from a
git worktree at the BEFORE commit and from the current checkout with the same
arguments and seed, then renders both through FluidSynth with a General MIDI
soundfont (MP3 for headphones), and writes a drummer chart (MusicXML, plus a
PDF when MuseScore 3 is installed). A listening sheet names what to listen for
and where.

General MIDI sounds, not Ugritone: this verifies hit placement, flams, pulse
and section shape, not the kit's feel.

    sudo apt install fluidsynth fluid-soundfont-gm ffmpeg   # musescore3 xvfb for PDFs
    python scripts/ab_render.py --out /tmp/ab               # all cases
    python scripts/ab_render.py --out /tmp/ab --only meter  # cases whose name contains "meter"
"""
import argparse
import hashlib
import os
import shutil
import subprocess
import sys
import tempfile

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SOUNDFONT = "/usr/share/sounds/sf2/FluidR3_GM.sf2"
KIT = "general_midi"   # GM note numbers, so the GM soundfont plays the right drums

# name, before-commit, CLI args, what to listen for. Seeds are fixed so both
# sides roll the same dice; the engines differ only where the fix landed.
CASES = [
    ("01-song-into-blast", "448048c",
     ["--style", "atdi", "-a", "4:verse 4:chorus 4:verse 4:blast", "--tempo", "170",
      "--seed", "7", "--humanize", "0.5"],
     "Downbeats of bar 5 (chorus, 0:05.6) and bar 13 (blast, 0:16.9). BEFORE: two "
     "kicks under 6 ms apart on each, heard as a thicker, slightly smeared hit "
     "rather than a clear flam. AFTER: one kick and one crash."),
    ("02-euro-screamo-sections", "448048c",
     ["--style", "euro_screamo", "-a", "4:build 4:drive 4:blast 4:breakdown", "--tempo", "160",
      "--seed", "3", "--humanize", "0.5"],
     "Section changes at bar 5 (0:06.0), bar 9 (0:12.0) and bar 13 (0:18.0). BEFORE: "
     "a doubled kick on each, and two crashes at bar 9. AFTER: one of each."),
    ("03-zona-pedal", "448048c",
     ["--cell", "prob_jazz_comp_4_4", "--bars", "8", "--tempo", "140", "--seed", "3",
      "--humanize", "0.4"],
     "The hi-hat foot chick on beat 2 of bars 2, 3 and 6 (0:02.1, 0:03.9, 0:09.0). "
     "BEFORE: two chicks one tick apart, heard as one louder, slightly phasey chick. "
     "AFTER: a single chick, same level as the others."),
    ("04-meter-kidcrash-7-8", "d3d5097",
     ["--style", "kidcrash", "--time-sig", "7/8", "--bars", "8", "--tempo", "150", "--seed", "5",
      "--humanize", "0.4"],
     "TASTE CALL. kidcrash owns only 4/4 cells, so a forced 7/8 adapts one. BEFORE: "
     "the 4/4 figure squeezed into eighth-note beats, i.e. double speed. AFTER: the "
     "figure at its own pulse, with the last eighth of every bar cut."),
    ("05-meter-kidcrash-6-8", "d3d5097",
     ["--style", "kidcrash", "--time-sig", "6/8", "--bars", "8", "--tempo", "150", "--seed", "5",
      "--humanize", "0.4"],
     "TASTE CALL, same as above in 6/8: AFTER keeps the quarter pulse and loses the "
     "figure's last quarter."),
    ("06-meter-screamo-6-8", "d3d5097",
     ["--style", "screamo", "--time-sig", "6/8", "--bars", "8", "--tempo", "170", "--seed", "9",
      "--humanize", "0.4"],
     "TASTE CALL on a denser style: double-speed vs held pulse."),
    ("07-layer-euclid-phasing", "448048c",
     ["--kick", "prob_jazz_comp_4_4", "--cymbal", "prob_jazz_comp_4_4",
      "--toms", "euclid_noise_polymeter_4_4", "--bars", "8", "--tempo", "130", "--seed", "4",
      "--humanize", "0.3"],
     "The floor tom. BEFORE: the same five-hit figure in all eight bars. AFTER: no two "
     "bars alike, the figure drifts against the bar line (polymeter)."),
    ("08-control-unchanged", "448048c",
     ["--cell", "blast_traditional", "--bars", "4", "--tempo", "200", "--seed", "1",
      "--humanize", "0.3"],
     "CONTROL: nothing in this take was touched. Before and after should be the "
     "same file (the sheet says whether the MIDI bytes match)."),
]


def run(cmd, cwd=None, quiet=True):
    r = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError(f"{' '.join(cmd)} failed:\n{r.stdout}\n{r.stderr}")
    return r.stdout


def worktree(commit, root):
    path = os.path.join(root, f"wt-{commit}")
    if not os.path.isdir(path):
        run(["git", "worktree", "add", "--detach", path, commit], cwd=REPO)
    return path


def generate(engine_dir, args, mid):
    run([sys.executable, "drumgen.py", *args, "--kit", KIT, "-o", mid], cwd=engine_dir)


def render(mid, mp3):
    wav = mp3[:-4] + ".wav"
    run(["fluidsynth", "-ni", "-q", "-F", wav, "-r", "44100", "-g", "0.8", SOUNDFONT, mid])
    run(["ffmpeg", "-y", "-loglevel", "error", "-i", wav, "-q:a", "2", mp3])
    os.remove(wav)


def chart(mid, base, pdf):
    sys.path.insert(0, REPO)
    import notation
    xml = base + ".musicxml"
    notation.mid_to_musicxml(mid, xml, kit_name=KIT)
    if pdf:
        try:
            run(["xvfb-run", "-a", "mscore3", "-o", base + ".pdf", xml])
        except RuntimeError as e:
            print(f"  (no PDF for {os.path.basename(base)}: {str(e).splitlines()[0]})")


def md5(path):
    return hashlib.md5(open(path, "rb").read()).hexdigest()


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--out", required=True, help="output directory")
    ap.add_argument("--only", default="", help="only cases whose name contains this")
    ap.add_argument("--no-pdf", action="store_true", help="skip MuseScore PDFs")
    args = ap.parse_args()

    for tool in ("fluidsynth", "ffmpeg"):
        if not shutil.which(tool):
            sys.exit(f"{tool} is not installed (see the docstring)")
    if not os.path.exists(SOUNDFONT):
        sys.exit(f"no soundfont at {SOUNDFONT}: install fluid-soundfont-gm")
    pdf = not args.no_pdf and bool(shutil.which("mscore3") and shutil.which("xvfb-run"))

    os.makedirs(args.out, exist_ok=True)
    scratch = tempfile.mkdtemp(prefix="ab-worktrees-")
    head = run(["git", "rev-parse", "--short", "HEAD"], cwd=REPO).strip()
    sheet = [f"# Before / after listening sheet\n",
             f"After = `{head}` (this checkout). General MIDI sounds via FluidSynth, "
             f"so judge placement, flams and pulse, not the kit.\n"]
    try:
        for name, before, cli, listen in CASES:
            if args.only not in name:
                continue
            print(f"{name}  (before {before})")
            d = os.path.join(args.out, name)
            os.makedirs(d, exist_ok=True)
            sides = {"before": worktree(before, scratch), "after": REPO}
            for side, engine in sides.items():
                mid = os.path.join(d, f"{side}.mid")
                generate(engine, cli, mid)
                render(mid, os.path.join(d, f"{side}.mp3"))
                chart(mid, os.path.join(d, side), pdf)
            same = md5(os.path.join(d, "before.mid")) == md5(os.path.join(d, "after.mid"))
            sheet.append(f"## {name}\n")
            sheet.append(f"`python drumgen.py {' '.join(cli)}` — before = `{before}`\n")
            sheet.append(f"{listen}\n")
            sheet.append(f"MIDI identical before/after: **{'yes' if same else 'no'}**\n")
    finally:
        for wt in [p for p in os.listdir(scratch) if p.startswith("wt-")]:
            subprocess.run(["git", "worktree", "remove", "--force", os.path.join(scratch, wt)],
                           cwd=REPO, capture_output=True)
        shutil.rmtree(scratch, ignore_errors=True)

    with open(os.path.join(args.out, "LISTENING.md"), "w") as f:
        f.write("\n".join(sheet))
    print(f"done -> {args.out}")


if __name__ == "__main__":
    main()
