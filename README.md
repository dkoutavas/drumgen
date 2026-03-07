# drumgen

Algorithmic drum MIDI pattern generator. No AI at runtime — pure Python CLI that outputs .mid files from hand-coded rhythmic cells with humanization.

Built for Ableton Live + Ugritone drums. Targets post-hardcore, math rock, noise rock, screamo, emoviolence, and experimental black metal.

## Install

```
python -m venv .venv
source .venv/bin/activate  # or .venv/bin/activate.fish
pip install -r requirements.txt
```

## Usage

```bash
# Generate a pattern
python drumgen.py --style shellac --tempo 130 --bars 8 -o verse.mid

# All options
python drumgen.py --style blast --tempo 180 --bars 4 --humanize 0.9 --swing 0.0 --seed 42

# Insert a fill every 4 bars
python drumgen.py --style raein --tempo 135 --bars 8 --fill-every 4

# List available cells and style shortcuts
python drumgen.py --list-cells

# Test a kit mapping (generates one hit per instrument)
python drumgen.py --test-mapping ugritone
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--style` / `-s` | required | Style tag: blast, dbeat, shellac, fugazi, faraquet, raein, posthardcore, noise_rock, screamo, math, euro_screamo |
| `--cell` | | Exact cell name (overrides --style) |
| `--tempo` / `-t` | 120 | BPM |
| `--bars` / `-b` | 4 | Number of bars |
| `--time-sig` / `-ts` | 4/4 | Time signature |
| `--humanize` | per-cell | Humanization amount 0.0-1.0 |
| `--swing` | 0.0 | Swing amount 0.0-1.0 |
| `--fill-every` | 0 | Insert fill every N bars (0 = none) |
| `--seed` | random | Seed for reproducibility |
| `--kit` | ugritone | Kit mapping name or path |
| `--output` / `-o` | auto | Output .mid path |

## Cells (Phase 1)

- **blast_traditional** — K/S alternating every 16th, ride every 16th
- **dbeat_standard** — X.XX kick pattern, snare backbeat, HH eighths
- **shellac_floor_tom_drive** — Floor tom 1/3, snare 2/4, ride quarters. No ghost notes.
- **fugazi_driving_chorus** — Syncopated kick, snare 2/4, ride eighths
- **faraquet_displaced_4_4** — 2-bar cell, displaced backbeat, ghost snares, ride eighths
- **raein_melodic_drive** — Dynamic HH (accent/ghost alternating), ghost snares
- **fill_linear_1bar** — Single-stroke roll descending through kit, velocity crescendo

## Architecture

```
drumgen.py          CLI entry point
assembler.py        Cell selection, bar layout, humanization
cell_library.py     Cell data + lookup functions
humanizer.py        Seeded RNG, per-instrument velocity/timing tables
midi_engine.py      Position math, MIDI file writing
kit_mappings/       JSON instrument-to-note mappings
styles/             Style DNA reference (build-time only)
```

## Kit Mappings

- `ugritone` — Ugritone drum plugin (default)
- `general_midi` — Standard GM drums

Custom mappings: create a JSON file in `kit_mappings/` or pass a path with `--kit`.
