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
./run-drumgen            # Linux / macOS / WSL
.\run-drumgen.ps1        # Windows PowerShell
```

Or manually: `streamlit run app.py` (venv must be active).

The Streamlit GUI provides the same features as the CLI: style/cell selection, arrangement mode, generative mode, layer mode, humanization controls, and kit mapping. All sidebar widgets have tooltip help — hover the (?) icon for guidance on values and ranges. After generation, the pattern preview shows a grid key (`X` = accent, `x` = normal, `o` = ghost, `.` = silent) and the selected cell's tags.

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

# Mixed meters in arrangement mode
python drumgen.py --style shellac -a "4:verse@7/8 2:fill@4/4 4:verse@7/8" --tempo 130

# Generative mode — probability-based patterns, different each seed
python drumgen.py --style faraquet --generative --tempo 140 --bars 8
python drumgen.py --style faraquet --generative --variations 3 --tempo 140 --bars 8

# Layer mode — mix instrument layers from different cells
python drumgen.py --kick blast_traditional --cymbal shellac_floor_tom_drive --bars 4 --tempo 160
python drumgen.py --kick dbeat_standard --snare blast_traditional --cymbal faraquet_displaced_4_4 --bars 4

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
| `--arrangement` / `-a` | | Arrangement: `"4:build 8:drive@7/8 2:blast"`. Use `@N/M` for per-section time sig. |
| `--generative` / `-g` | | Use probability grids for generative patterns. Same style, different each time. |
| `--variations` / `-n` | 1 | Generate N variations (each with different seed). Outputs `_v1.mid`, `_v2.mid`, etc. |
| `--kick` | | Layer mode: cell name for kick layer |
| `--snare` | | Layer mode: cell name for snare layer |
| `--cymbal` | | Layer mode: cell name for cymbal layer |
| `--toms` | | Layer mode: cell name for toms layer |
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

Build multi-section drum tracks with `--arrangement` / `-a`. Each token is `bars:section_type` with optional `@time_sig`:

```bash
python drumgen.py --style screamo -a "4:blast 1:silence 4:breakdown"
python drumgen.py --style shellac -a "4:verse@7/8 2:fill@4/4 4:verse@7/8" --tempo 130
```

Section types: `intro`, `build`, `verse`, `chorus`, `drive`, `blast`, `breakdown`, `atmospheric`, `silence`, `fill`, `outro`

The system automatically selects the best cell from the style pool for each section based on tag matching. Silence sections produce empty bars. Intense sections (chorus, blast, breakdown, drive) get a crash+kick on beat 1. Per-section time signatures (`@N/M`) insert MIDI time signature changes at section boundaries.

## Generative Mode

Probability grids produce unique patterns from each seed while staying true to the style's rhythmic DNA.

```bash
python drumgen.py --style faraquet --generative --tempo 140 --bars 8
python drumgen.py --style shellac --generative --variations 5 --tempo 130 --bars 4
```

Each grid entry has a probability (0.0-1.0). Near-deterministic styles like Shellac (0.98) sound almost identical each time. Angular styles like Faraquet (0.4-0.9) produce wide variation. Physical constraints (no ride+hihat, no snare+tom at same position) are enforced after realization.

Available probability cells: `prob_faraquet_4_4`, `prob_shellac_4_4`, `prob_posthardcore_4_4`, `prob_dbeat_4_4`, `prob_blast_4_4`, `prob_euro_screamo_4_4`, `prob_faraquet_7_8`.

## Layer Mode

Mix instrument layers from different cells into one pattern:

```bash
python drumgen.py --kick blast_traditional --cymbal shellac_floor_tom_drive --bars 4 --tempo 160
python drumgen.py --kick dbeat_standard --snare blast_traditional --cymbal faraquet_displaced_4_4 --bars 4
```

Layer groups: `kick`, `snare` (snare + ghost + rim), `cymbal` (hihat, ride, crash, china, splash), `toms` (high, mid, low, floor). Conflicts at the same beat position are resolved by priority (crash > ride > hihat, snare > tom). Mutually exclusive with `--arrangement`.

## Cells (28)

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

### Probability Grid Cells (Generative)

| Cell | Time Sig | Description |
|------|----------|-------------|
| prob_faraquet_4_4 | 4/4 | Angular math rock. Ride 0.9 eighths, syncopated kicks 0.4-0.7, displaced snare |
| prob_shellac_4_4 | 4/4 | Near-deterministic. Floor tom/snare 0.98, ride 1.0 quarters |
| prob_posthardcore_4_4 | 4/4 | Fugazi driving. Kick 0.9 on 1/3, snare 0.85 on 2/4, ride 0.95 eighths |
| prob_dbeat_4_4 | 4/4 | D-beat. X.XX kick pattern 0.95, HH 0.9 eighths |
| prob_blast_4_4 | 4/4 | Blast beat. K/S alternating 16ths 0.92, ride 16ths 0.88 |
| prob_euro_screamo_4_4 | 4/4 | Daitro-style. Kick 0.85, snare 0.9, ghost 0.35, ride 0.95 |
| prob_faraquet_7_8 | 7/8 | Angular 7/8. 2+2+3 grouping, ride 0.95, kick on 1/3/5 |

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

| Style | Pool (includes odd-meter and probability variants) |
|-------|------|
| blast | blast_traditional, emoviolence_blast_crash, + odd meters, **prob_blast_4_4** |
| dbeat | dbeat_standard, dbeat_7_8, **prob_dbeat_4_4** |
| shellac | shellac_floor_tom_drive, + odd meters, **prob_shellac_4_4** |
| fugazi | fugazi_driving_chorus, + odd meters |
| faraquet | faraquet_displaced_4_4, faraquet_7_8, faraquet_5_4, **prob_faraquet_4_4**, **prob_faraquet_7_8** |
| raein | raein_melodic_drive |
| posthardcore | fugazi_driving_chorus, faraquet_displaced_4_4, raein_melodic_drive, + odd meters, **prob_posthardcore_4_4** |
| noise_rock | shellac_floor_tom_drive, + odd meters, **prob_shellac_4_4** |
| screamo | emoviolence_blast_crash, emoviolence_angular_breakdown, blast_traditional |
| emoviolence | emoviolence_blast_crash, emoviolence_angular_breakdown, blast_traditional |
| math | faraquet_displaced_4_4, faraquet_7_8, faraquet_5_4, **prob_faraquet_4_4**, **prob_faraquet_7_8** |
| euro_screamo | daitro_tremolo_drive, daitro_quiet_build, daitro_blast_release, raein_melodic_drive, **prob_euro_screamo_4_4** |
| daitro | daitro_quiet_build, daitro_tremolo_drive, daitro_blast_release |
| liturgy | liturgy_burst_beat |
| black_metal | liturgy_burst_beat, blackmetal_atmospheric, deafheaven_build_to_blast, atmospheric_7_8 |
| deafheaven | deafheaven_build_to_blast, blackmetal_atmospheric |

## Architecture

```
app.py              Streamlit GUI (generation, generative mode, layer mode, MIDI import)
preview.py          FluidSynth audio preview (WAV rendering)
drumgen.py          CLI entry point (generative, variations, layer mode, mixed meters)
assembler.py        Cell selection, bar layout, arrangement mode, probability grid
                    realization, layer extraction/conflict resolution, humanization
cell_library.py     Cell data (fixed + probability grids), style pools, section prefs
humanizer.py        Seeded RNG, per-instrument velocity/timing tables
midi_engine.py      Position math, MIDI file writing, note overlap prevention,
                    interleaved time sig + note event write for mixed meters
midi_reader.py      MIDI import, auto-tagging, validation, dedup, content hashing
als_extractor.py    Ableton .als extraction, non-drum track filtering
test_drumgen.py     Test suite (pytest) — 125 tests
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
