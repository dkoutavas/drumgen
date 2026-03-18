# drumgen — Quick Start

## Windows (first time)

1. Install Python from [python.org](https://www.python.org/downloads/) — check **"Add Python to PATH"**
2. Double-click `setup.bat`
3. Double-click `drumgen.bat`

The GUI opens in your browser. Output files go to `Documents/drumgen_output/`.

## Linux / macOS / WSL

```bash
./setup.sh
source .venv/bin/activate
```

## Generate your first pattern

```bash
python drumgen.py --style screamo --tempo 180 --bars 4
```

Output goes to `output/`. Open the `.mid` file in your DAW.

## More examples

```bash
# Arrangement mode — chain sections into one MIDI file
python drumgen.py --style euro_screamo -a "8:build 8:drive 4:blast" --tempo 140

# Mixed meters
python drumgen.py --style shellac -a "4:verse@7/8 2:verse@4/4 4:verse@7/8" --tempo 130

# Layer mode — mix kick from one cell, cymbals from another
python drumgen.py --kick blast_traditional --cymbal shellac_floor_tom_drive --bars 4 --tempo 160

# Generative mode — probability-based, different each time
python drumgen.py --style faraquet --generative --tempo 140 --bars 8

# See all available styles and cells
python drumgen.py --list-cells
```

## GUI

```bash
./run-drumgen
```

Opens a browser-based interface with the same generation options.

## Audio preview (optional)

The GUI can render a WAV preview if FluidSynth is installed:
- **Debian/Ubuntu:** `sudo apt install fluidsynth fluid-soundfont-gm`
- **macOS:** `brew install fluid-synth`
- **openSUSE:** `sudo zypper install fluidsynth fluid-soundfont-gm`

The preview uses General MIDI sounds — your Ugritone/AD kit will sound different.
