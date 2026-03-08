# drumgen

Algorithmic drum MIDI pattern generator. No AI at runtime — pure Python that outputs .mid files from hand-coded rhythmic cells with humanization. CLI and Streamlit GUI.

Built for Ableton Live + Ugritone drums. Targets post-hardcore, math rock, noise rock, screamo, emoviolence, euro-screamo, and experimental black metal.

## Install

```
python -m venv .venv
source .venv/bin/activate  # or .venv/bin/activate.fish
pip install -r requirements.txt
```

## GUI

```bash
streamlit run app.py
```

The Streamlit GUI provides the same features as the CLI: style/cell selection, arrangement mode, humanization controls, and kit mapping. All sidebar widgets have tooltip help — hover the (?) icon for guidance on values and ranges. After generation, the pattern preview shows a grid key (`X` = accent, `x` = normal, `o` = ghost, `.` = silent) and the selected cell's tags.

Requires: `pip install streamlit` (included in requirements.txt).

## CLI Usage

```bash
# Generate a pattern
python drumgen.py --style shellac --tempo 130 --bars 8 -o verse.mid

# Screamo (now maps to emoviolence_blast_crash, not just blast)
python drumgen.py --style screamo --tempo 180 --bars 4

# Euro-screamo
python drumgen.py --style euro_screamo --tempo 140 --bars 8

# Black metal
python drumgen.py --style black_metal --tempo 130 --bars 4

# Arrangement mode — multi-section songs
python drumgen.py --style screamo -a "4:blast 1:silence 4:breakdown" --tempo 180
python drumgen.py --style euro_screamo -a "8:build 8:drive 4:blast" --tempo 140
python drumgen.py --style black_metal -a "4:atmospheric 4:build 4:blast" --tempo 130

# Insert a fill every 4 bars
python drumgen.py --style raein --tempo 135 --bars 8 --fill-every 4

# Use a specific cell directly
python drumgen.py --cell liturgy_burst_beat --tempo 106 --bars 4

# List available cells and style pools
python drumgen.py --list-cells

# Test a kit mapping (generates one hit per instrument)
python drumgen.py --test-mapping ugritone
```

## CLI Options

| Flag | Default | Description |
|------|---------|-------------|
| `--style` / `-s` | required | Style: blast, dbeat, shellac, fugazi, faraquet, raein, posthardcore, noise_rock, screamo, emoviolence, math, euro_screamo, daitro, liturgy, black_metal, deafheaven |
| `--cell` | | Exact cell name (overrides --style) |
| `--arrangement` / `-a` | | Arrangement string, e.g. `"4:build 8:drive 2:blast 1:silence 4:breakdown"` |
| `--tempo` / `-t` | 120 | BPM |
| `--bars` / `-b` | 4 | Number of bars (ignored in arrangement mode) |
| `--time-sig` / `-ts` | 4/4 | Time signature |
| `--humanize` | per-cell | Humanization amount 0.0-1.0 |
| `--swing` | 0.0 | Swing amount 0.0-1.0 |
| `--vary` / `-v` | 0.0 | Variation amount 0.0-1.0 — mutates repeated bars (ghost adds, kick shifts, HH swaps) |
| `--fill-every` | 0 | Insert fill every N bars (0 = none) |
| `--seed` | random | Seed for reproducibility |
| `--kit` | ugritone | Kit mapping name or path |
| `--output` / `-o` | auto | Output .mid path |

## Arrangement Mode

Build multi-section drum tracks with `--arrangement` / `-a`. Each token is `bars:section_type`:

```bash
python drumgen.py --style screamo -a "4:blast 1:silence 4:breakdown"
```

Section types: `intro`, `build`, `verse`, `chorus`, `drive`, `blast`, `breakdown`, `atmospheric`, `silence`, `fill`, `outro`

The system automatically selects the best cell from the style pool for each section based on tag matching. Silence sections produce empty bars. Intense sections (chorus, blast, breakdown, drive) get a crash+kick on beat 1.

## Cells (21)

### Groove Cells

| Cell | Bars | Description |
|------|------|-------------|
| blast_traditional | 1 | K/S alternating every 16th, ride every 16th |
| dbeat_standard | 1 | X.XX kick pattern, snare backbeat, HH eighths |
| shellac_floor_tom_drive | 1 | Floor tom 1/3, snare 2/4, ride quarters |
| fugazi_driving_chorus | 1 | Syncopated kick (1, 2+, 3), snare 2/4, ride eighths |
| faraquet_displaced_4_4 | 2 | Displaced backbeat, ghost snares, ride eighths |
| raein_melodic_drive | 1 | Dynamic HH accent/ghost, ghost snares |
| emoviolence_angular_breakdown | 1 | Half-time. K 1/3/3.5, snare 3, floor tom 4.5 |
| emoviolence_blast_crash | 2 | Blast + crash on every quarter note |
| daitro_quiet_build | 8 | Ride bell → ride + kick → snare → full. Crescendo humanize. |
| daitro_tremolo_drive | 1 | Fast kick doubles, snare 2/4, ride eighths |
| daitro_blast_release | 4 | Bars 1-3 full blast, bar 4 half-blast (receding) |
| liturgy_burst_beat | 1 | K/S near-simultaneous (flammed) every 16th. 3-over-4 accent. |
| blackmetal_atmospheric | 1 | Sparse: kick 1, ride bell pings, snare 3, HH pedal |
| deafheaven_build_to_blast | 8 | Kick quarters → eighths → sixteenths → full blast |

### Fill Cells

| Cell | Description |
|------|-------------|
| fill_linear_1bar | Single-stroke roll descending through kit, velocity crescendo |
| emoviolence_chaotic_fill | Beats 1-2 silence, 3-4 sixteenths across kit |
| fill_floor_tom_sparse | 3 floor tom hits only. Massive, sparse. |

### Transition Cells

| Cell | Bars | Description |
|------|------|-------------|
| transition_crash_silence | 1 | Crash + kick on beat 1, rest silence |
| transition_half_time_shift | 2 | Half-time feel, kick syncopation, ride eighths |
| transition_snare_roll_to_crash | 1 | Beats 1-2 silence, 3-4 snare roll crescendo |
| transition_cymbal_swell | 2 | Ride bell swell ghost→accent, kick joins bar 2 |

## Style Pools

Each style maps to a pool of cells. In arrangement mode, the best cell is selected per section.

| Style | Pool |
|-------|------|
| blast | blast_traditional, emoviolence_blast_crash |
| dbeat | dbeat_standard |
| shellac | shellac_floor_tom_drive |
| fugazi | fugazi_driving_chorus |
| faraquet | faraquet_displaced_4_4 |
| raein | raein_melodic_drive |
| posthardcore | fugazi_driving_chorus, faraquet_displaced_4_4, raein_melodic_drive |
| noise_rock | shellac_floor_tom_drive |
| screamo | emoviolence_blast_crash, emoviolence_angular_breakdown, blast_traditional |
| emoviolence | emoviolence_blast_crash, emoviolence_angular_breakdown, blast_traditional |
| math | faraquet_displaced_4_4 |
| euro_screamo | daitro_tremolo_drive, daitro_quiet_build, daitro_blast_release, raein_melodic_drive |
| daitro | daitro_quiet_build, daitro_tremolo_drive, daitro_blast_release |
| liturgy | liturgy_burst_beat |
| black_metal | liturgy_burst_beat, blackmetal_atmospheric, deafheaven_build_to_blast |
| deafheaven | deafheaven_build_to_blast, blackmetal_atmospheric |

## Architecture

```
app.py              Streamlit GUI (generation + MIDI import with preview/validation)
preview.py          FluidSynth audio preview (WAV rendering)
drumgen.py          CLI entry point
assembler.py        Cell selection, bar layout, arrangement mode, humanization
cell_library.py     Cell data, style pools, section preferences, auto-pool integration
humanizer.py        Seeded RNG, per-instrument velocity/timing tables
midi_engine.py      Position math, MIDI file writing, note overlap prevention
midi_reader.py      MIDI import, auto-tagging, validation, dedup, content hashing
als_extractor.py    Ableton .als extraction, non-drum track filtering
kit_mappings/       JSON instrument-to-note mappings (ugritone, addictive_drums, GM)
user_cells/         Imported cell JSON files (gitignored, auto-loaded)
styles/             Style DNA reference (build-time only)
```

## MIDI Import

Import your own MIDI drum patterns as cells. Extracted from Ableton projects or uploaded directly.

```bash
# Extract MIDI clips from Ableton .als projects
python als_extractor.py /path/to/projects/ --recursive --drums-only -o /tmp/extract/

# Import with auto-tagging (use addictive_drums kit for AD sources)
python midi_reader.py /tmp/extract/ --auto-tag --kit addictive_drums

# Maintenance
python midi_reader.py --validate --kit addictive_drums
python midi_reader.py --stats
python midi_reader.py --retag
python midi_reader.py --dedup --confirm
```

Import features: content-based auto-tagging (blast, halftime, backbeat, fill, density, odd meter detection), content hashing for dedup, hit deduplication, trailing bar trim, non-drum track filtering, and validation. Imported cells auto-integrate into style pools based on their tags.

The GUI also supports MIDI import via the sidebar expander with preview, auto-tagging, and validation.

## Kit Mappings

- `ugritone` — Ugritone drum plugin (default)
- `addictive_drums` — Addictive Drums (note 48 = snare, alias 38 = snare)
- `general_midi` — Standard GM drums

Custom mappings: create a JSON file in `kit_mappings/` or pass a path with `--kit`. Kit files support an `aliases` field for additional note-to-instrument mappings (useful when a source kit maps multiple notes to the same instrument).
