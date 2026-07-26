# drumgen — Quick Start

Five minutes from clone to drums in Bitwig.

## 1. Setup (once)

```bash
python -m venv .venv
source .venv/bin/activate          # or .venv/bin/activate.fish
pip install -r requirements.txt
```

## 2. Build + install the plugin (once, then after every change)

```bash
cd plugin && ./build-linux.sh      # → ~/.vst3/ and ~/.clap/
```

Bitwig: Settings → Locations → make sure `~/.vst3` and `~/.clap` are scanned.

> After reinstalling, restart Bitwig, the old plugin binary stays loaded
> in memory otherwise.

## 3. Play

One instrument track, device chain in this order:

```
drumgen  →  your drum sampler (e.g. Ugritone via yabridge)
```

Press play. Notes flow downstream in the chain — no routing needed. drumgen
itself is silent by design; the sampler makes the sound.

Then:

- STYLE: pick a genre (posthardcore, screamo, zona, black_metal, …)
- SONG: step past *Off* for a whole song skeleton (try Skramz Arc)
- DICE: re-roll. Same SEED always gives the same notes back.
- SAVE .MID: drops the pattern in `~/drumgen_output/`

Full wiring, recording takes to audio or MIDI clips, and yabridge notes:
[BITWIG.md](BITWIG.md).

## 4. Optional: charts for your drummer

```bash
./scripts/install-notation-hook.sh
```

Every SAVE .MID now also writes a `.musicxml`, open it in MuseScore 4 and
export a PDF. See [NOTATION.md](NOTATION.md).

## 5. Optional: your own song forms

Edit `~/.config/drumgen/songs.txt` (a commented starter file is created on
first plugin load), add a line, restart the DAW:

```
My Maze | 2:atmospheric 3:verse@7/8 1:fill 2:blast 4:build 1:fill 4:blast 2:outro
```

---

## Without a DAW: the CLI

```bash
python drumgen.py --style screamo --tempo 180 --bars 4
python drumgen.py --style euro_screamo -a "2:intro 8:build 1:fill 4:blast" --tempo 140
python drumgen.py --list-cells
```

Files land in `output/`. A browser GUI is available too: `./run-drumgen`.

Optional WAV preview in the GUI needs FluidSynth
(`sudo zypper install fluidsynth fluid-soundfont-gm` on openSUSE) — it uses
General MIDI sounds, so it won't match your real kit.

---

New here and after the reasoning behind the buttons? Read
[PROJECT.md](PROJECT.md).
