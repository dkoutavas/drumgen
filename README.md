# drumgen

A drum-part generator for labyrinthine heavy music. Hand-authored rhythmic
cells, assembled by seeded algorithms, humanized like a player, emitted live
from a native VST3/CLAP plugin, or written to `.mid` from a CLI.

No AI. No model. No inference. Every note traces back to a musical decision
someone made on purpose. This replaces the session drummer you don't have at
your desk. The composer stays you; so does the drummer in the room.

Built for Bitwig Studio on native Linux + Ugritone drums. Targets
screamo/emoviolence, post-hardcore, math rock, noise rock, post-rock,
atmospheric black metal, and jazz-on-hardcore.

→ [PROJECT.md](PROJECT.md), purpose, beliefs, state, architecture, lessons
(read this to understand the project)
→ [BITWIG.md](BITWIG.md), plugin wiring, recording takes, yabridge
→ [NOTATION.md](NOTATION.md), handing charts to a human drummer

---

## The plugin (daily driver)

```bash
python -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt

cd plugin && ./build-linux.sh     # builds + installs to ~/.vst3 and ~/.clap
```

Then in Bitwig: one instrument track, device chain = drumgen → your drum
sampler, press play. (Restart Bitwig after reinstalling, the old `.so` stays
in memory.)

### What the plugin gives you

| Control | Does |
|---|---|
| STYLE | 30 genre pools: each produces a distinctly different beat |
| SONG | 10 song forms (Verse/Chor, Skramz Arc, Stop/Go, Quiet/Loud, Eruption, Post-Rock, Blast Fwd, Labyrinth, Ampere): a whole 14–32 bar skeleton with section dynamics, stops and meter turns. Plus your own forms (below) |
| DICE / SEED | Re-roll the groove or the whole song. Same seed = same notes, forever |
| HUMANIZE / SWING | Velocity variance, timing tendencies, flam, ghost clustering / triplet lean (0.50 = full triplet) |
| BARS / METER / FILL | Loop length; meter (Auto follows the host's time signature live, or force 3/4 · 4/4 · 5/4 · 6/4 · 6/8 · 7/8); fill every N bars |
| Telegraph + horizon | A countdown line (`CHORUS ▸ BLAST IN 2`) and a strip showing the current bar plus the next three, with a sweeping playhead: so you can keep both hands on the guitar |
| SAVE .MID | Writes the pattern to `~/drumgen_output/`: and renders a drum chart too, if you install the hook |

### Your own song forms

Edit `~/.config/drumgen/songs.txt` (the plugin plants a commented starter file
on first run), then restart the DAW:

```
My Maze | 2:atmospheric 3:verse@7/8 1:fill 2:blast 3:verse@7/8 4:build 1:fill 4:blast 2:outro
```

Sections: `intro build verse chorus drive blast breakdown atmospheric silence
fill outro`. Meters via `@N/M`. Bad lines are rejected whole and logged — a
typo never silently becomes a mediocre song.

### Charts for your drummer

```bash
./scripts/install-notation-hook.sh    # once
```

Now every SAVE .MID also writes a `.musicxml` next to it. Open it in
MuseScore 4 (free, native Linux) → export PDF: a real drum staff with ghost
notes in parentheses, x-notehead cymbals, accents, and explicit meter changes.
Details and the Guitar Pro verdict in [NOTATION.md](NOTATION.md).

---

## The CLI (authoring workshop)

```bash
# A pattern
python drumgen.py --style kidcrash --tempo 165 --bars 8 -o verse.mid

# A song form
python drumgen.py --style euro_screamo -a "2:intro 8:build 1:fill 4:blast" --tempo 140

# Mixed meters per section
python drumgen.py --style faraquet -a "4:verse@7/8 1:fill 4:drive@6/8" --tempo 150

# Generative (probability grids + Euclidean cells re-realize per seed)
python drumgen.py --style zona --generative --tempo 140 --bars 8 --swing 0.4
python drumgen.py --style faraquet --generative --variations 3 --tempo 140

# Layer instruments from different cells
python drumgen.py --kick blast_traditional --cymbal shellac_floor_tom_drive --bars 4

# Fills, notation, listings
python drumgen.py --style raein --bars 8 --fill-every 4
python drumgen.py --style zona --bars 8 --musicxml       # also writes a chart
python drumgen.py --list-cells                           # all cells + style pools
python drumgen.py --test-mapping ugritone                # one hit per instrument
```

Key options: `--style` / `--cell`, `-a/--arrangement`, `--tempo`, `--bars`,
`--time-sig`, `--humanize`, `--swing`, `--seed`, `--vary`, `--fill-every`,
`--generative`, `--variations`, `--kit`, `--musicxml`, `-o`.

A Streamlit GUI covers the same ground: `./run-drumgen` (or
`streamlit run app.py`).

---

## Library

117 cells across 30 plugin style pools — 28 probability grids, 5 Euclidean
polymeter cells, 26 fills, the rest fixed patterns. `python drumgen.py
--list-cells` prints the current inventory; `styles/drumgen-style-dna.md`
documents the rhythmic vocabulary per genre.

Three cell kinds:

- fixed: exact hits, the same every time.
- probability grid: each hit has a firing probability, re-realized per
  seed. Trig conditions (`A:B` pass ratios, `1st`, `last`, `pre`, `!pre`) give
  cells memory, so a figure can state itself and then answer itself.
- Euclidean: per-limb `E(pulses, steps)` cycles that tile across bars
  without resetting, so co-prime limbs phase for many bars.

Generated bars are steered rather than simply rolled. A syncopation guard keeps each
bar inside the groove sweet spot, a tension envelope thickens the pattern as it
progresses, and section dynamics give builds a real rise (velocity floor and
density climbing together).

## Import your own MIDI

```bash
# Ableton projects → .mid → cells
python als_extractor.py project.als --drums-only -o extracted/
python midi_reader.py extracted/ --auto-tag --kit addictive_drums
python drumgen.py --cell my_imported_cell --bars 4 --tempo 120

# Maintenance
python midi_reader.py --list | --stats | --validate | --retag | --dedup
```

Imported cells land in `user_cells/` as JSON and auto-join style pools by tag.

## Kits

`kit_mappings/*.json` map instrument names to MIDI notes: `ugritone.json`
(default, 25 instruments), `addictive_drums.json` (note 48 = snare),
`general_midi.json`. Kits support an `aliases` field. MIDI channel 10, PPQ 480.

## Development

```bash
python -m pytest test_drumgen.py -q     # 289 tests
python validate_midi.py                  # pipeline sanity across configs
python export_cells.py                   # after ANY cell_library.py edit
cd plugin && cargo test                  # 74 tests
cd plugin && ./build-linux.sh            # build + install
```

The engine is hand-duplicated: Python (`assembler.py`, `cell_library.py`,
`humanizer.py`, `midi_engine.py`) and Rust (`plugin/src/engine/*.rs`). Changes
to engine logic must land in both. Cells are data, edit `cell_library.py`,
run `export_cells.py`. See [CLAUDE.md](CLAUDE.md) for conventions and
[PROJECT.md](PROJECT.md) for the reasoning behind all of it.

CI builds Linux, Windows and macOS bundles on push; a `v*` tag cuts a release.

## License

DIY. Open source. No cloud, no accounts, no telemetry — it all runs on your box.
