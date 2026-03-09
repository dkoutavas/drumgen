# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

drumgen is a pure-algorithmic drum MIDI pattern generator (no AI at runtime). Hand-coded rhythmic "cells" are assembled into patterns, humanized, and written to .mid files. Built for Ableton Live + Ugritone drums. Targets post-hardcore, math rock, noise rock, screamo, emoviolence, euro-screamo, and experimental black metal.

## Commands

```bash
# Setup
python -m venv .venv
source .venv/bin/activate  # or .venv/bin/activate.fish
pip install -r requirements.txt

# Run CLI
python drumgen.py --style screamo --tempo 180 --bars 4
python drumgen.py --style euro_screamo -a "8:build 8:drive 4:blast" --tempo 140
python drumgen.py --list-cells

# Generative mode (probability grids — different output each seed)
python drumgen.py --style faraquet --generative --tempo 140 --bars 8
python drumgen.py --style faraquet --generative --variations 3 --tempo 140 --bars 8

# Layer mode (mix instrument layers from different cells)
python drumgen.py --kick blast_traditional --cymbal shellac_floor_tom_drive --bars 4 --tempo 160

# Mixed meters in arrangement mode
python drumgen.py --style shellac -a "4:verse@7/8 2:verse@4/4 4:verse@7/8" --tempo 130

# Run GUI
./run-drumgen              # auto-activates venv
# or manually: streamlit run app.py

# Test a kit mapping (one hit per instrument)
python drumgen.py --test-mapping ugritone

# Run tests
python -m pytest test_drumgen.py -v
```

### MIDI Import & ALS Extraction

```bash
# Import a MIDI file as a cell
python midi_reader.py input.mid --name my_cell --tags blast,intense --kit ugritone

# Import from Addictive Drums source (note 48 = snare, not tom_high)
python midi_reader.py input.mid --kit addictive_drums --auto-tag

# Import all .mid files in a directory (auto-detect tags from content)
python midi_reader.py input_dir/ --auto-tag --kit addictive_drums

# List imported cells
python midi_reader.py --list

# Maintenance commands
python midi_reader.py --validate --kit addictive_drums
python midi_reader.py --stats
python midi_reader.py --retag
python midi_reader.py --dedup [--confirm]

# Extract MIDI clips from an Ableton .als project
python als_extractor.py project.als -o extracted/ --drums-only --verbose

# Recursively extract from a directory of .als files
python als_extractor.py /path/to/projects/ --recursive --drums-only

# Dry run (list clips without writing)
python als_extractor.py project.als --dry-run

# Full pipeline: extract → import → generate
python als_extractor.py project.als --drums-only -o /tmp/extract/
python midi_reader.py /tmp/extract/ --auto-tag --kit addictive_drums
python drumgen.py --cell my_imported_cell --bars 4 --tempo 120
```

## Architecture

The pipeline flows: **CLI/GUI -> Assembler -> Cell Library + Humanizer -> MIDI Engine -> .mid file**

- `drumgen.py` — CLI entry point (argparse). Parses args, delegates to `assemble()`, `assemble_arrangement()`, or `assemble_layered()`, then `write_midi()`. Supports `--generative`, `--variations`, and layer mode (`--kick/--snare/--cymbal/--toms`).
- `app.py` — Streamlit GUI. Same generation pipeline as CLI. Sidebar organized into visual sections (Output, What, Sound, Feel, Modes, Generate). Output folder displays/accepts Windows paths with "Open folder" button (explorer.exe). Mode-aware controls: Style/Cell disabled when layer mode active, Bars disabled in arrangement mode. Layer Mode expander includes clear button and active summary. Arrangement mode has meter quick-add selector for per-section time signatures. Smart filename auto-updates based on active mode (generative prefix, layer names, cell override). Generate button in both sidebar and main area. Post-generate layout: compact success row with download + open folder, audio preview with optional auto-play, pattern grid in collapsible expander, params/stats collapsed. MIDI import expander at bottom of sidebar.
- `assembler.py` — Core orchestrator. Four main functions:
  - `assemble()` — Single-cell mode: repeats a cell for N bars, inserting fills if requested. Supports `generative=True` to prefer probability grid cells.
  - `assemble_arrangement()` — Multi-section mode: parses arrangement strings like `"4:build 8:drive@7/8 2:blast"`, picks best cell per section via tag scoring. Supports per-section time signatures via `@N/M` suffix and `generative=True`.
  - `assemble_layered()` — Layer mode: mixes instrument layers (kick/snare/cymbal/toms) from different cells into one pattern, with conflict resolution.
  - Also handles: hit normalization (4-tuple to 5-tuple), probability grid realization (`realize_probability_grid`), physical constraint validation, layer extraction/conflict resolution, variation mutations (`vary_hits`), velocity drift per section, per-bar humanize overrides. All three assemble functions apply advanced humanization post-processing (flam, ghost clustering) after bar loop and pass section drift into `_process_bar()`.
- `cell_library.py` — All rhythmic cells defined as Python functions returning dicts. Includes both fixed cells (with `hits`) and probability grid cells (with `type: "probability"` and `grid`). Contains `CELLS` registry, `STYLE_POOLS` (style -> list of cell names), `SECTION_PREFERENCES` (section type -> preferred tags), and lookup functions (`get_cell`, `get_pool`, `get_cell_for_section`). Loads user-imported cells from `user_cells/` via `load_user_cells()` and auto-integrates them into `STYLE_POOLS` via `TAG_TO_POOLS` tag-to-pool mapping.
- `test_drumgen.py` — Comprehensive test suite (pytest, 226 tests). Covers cell integrity, time signatures, style pools, assembler, humanizer, MIDI engine, end-to-end generation, probability grids, layer mode, mixed meters (including note position verification), variations, Phase 3 style palette expansion, and advanced humanization (velocity contour, flam, section drift, ghost clustering, seed reproducibility).
- `midi_reader.py` — Standalone CLI + importable library. Reads `.mid` files via mido, converts to drumgen's native cell format (flat 5-tuple hits), saves as JSON in `user_cells/`. Includes content-based auto-tagging (`auto_tag_cell()`), validation (`validate_cell()`), hit deduplication, trailing bar trim, and content hashing for dedup. Exposes `midi_to_cell()`, `save_cell()`, `auto_tag_cell()`, `validate_cell()` for GUI use.
- `als_extractor.py` — Standalone CLI. Opens `.als` files (gzip-compressed XML), finds MidiClip elements from both Session and Arrangement views, writes each as a `.mid` file to `extracted/`. Filters non-drum tracks via name blacklist (synth, sampler, pad, etc.) when `--drums-only` is used.
- `humanizer.py` — `Humanizer` class with seeded RNG. Per-instrument velocity variance tables, timing tendencies (e.g., snare slightly late, ride slightly early), swing application. Advanced humanization: velocity contour (wrist pattern for cymbals + beat-1 emphasis), section push/pull drift (verse drags, chorus pushes, build gradually pushes), kick-snare flam (kick pulled 5-12ms early on simultaneous hits), and ghost note clustering (ghosts gravitate toward snare accents, style-dependent via `_CLUSTER_TAG_AMOUNTS`). Module-level helpers: `get_cluster_amount(cell)`, `infer_section_type(cell)`.
- `midi_engine.py` — Position-to-tick math and MIDI file writing via `mido`. Constants: PPQ=480, note duration=30 ticks, MIDI channel=9. Includes note overlap prevention (inserts early note_off when humanizer timing causes pitch collisions). Time signature meta messages and note events are interleaved in a single sorted pass (by absolute tick) to avoid mixed-meter delta calculation bugs.
- `preview.py` — Optional FluidSynth-based WAV rendering for the Streamlit GUI.
- `kit_mappings/` — JSON files mapping instrument names to MIDI note numbers. Default: `ugritone.json`. Also `addictive_drums.json` (note 48 = snare) and `general_midi.json`. Kit files support an `aliases` field for additional note-to-instrument mappings.
- `user_cells/` — Directory for imported cell JSON files (gitignored). Loaded automatically by `cell_library.py` at import time.
- `extracted/` — Directory for MIDI files extracted from .als projects (gitignored).
- `styles/drumgen-style-dna.md` — Reference document describing rhythmic vocabulary per genre (blast beats, d-beats, Shellac precision, etc.). Used as build-time guidance, not consumed by code.

## Key Concepts

**Cell format:** Each cell is a dict with `name`, `tags`, `time_sig`, `num_bars`, `humanize`, `role` (groove/fill/transition), and `hits`. Single-bar cells use 4-tuples `(beat, sub, instrument, velocity_level)`. Multi-bar cells use 5-tuples `(bar, beat, sub, instrument, velocity_level)`. Sub values: 0.0=on beat, 0.25=sixteenth, 0.5=eighth, 0.75=dotted eighth.

**Probability grid cells:** Cells with `type: "probability"` use `grid` instead of `hits`. Grid entries are 5-tuples `(beat, sub, instrument, probability, velocity_level)` for single-bar or 6-tuples `(bar, beat, sub, instrument, probability, velocity_level)` for multi-bar. Each entry has a probability (0.0-1.0) that determines whether the hit is included on each realization. Different seeds produce different patterns from the same grid. Physical constraints (limb conflicts) are validated after realization.

**Velocity levels:** `ghost`, `soft`, `normal`, `accent` — mapped to numeric ranges by the Humanizer.

**Style pools vs STYLE_MAP:** `STYLE_POOLS` maps a style name to a list of cell names (used in arrangement mode for section-aware selection). `STYLE_MAP` is a backward-compat shortcut mapping each style to its first cell.

**Arrangement mode:** Parses `"N:section_type"` or `"N:section_type@num/den"` tokens. Section types (intro, build, verse, chorus, drive, blast, breakdown, atmospheric, silence, fill, outro) have preferred tags in `SECTION_PREFERENCES`. The assembler scores pool cells against these preferences to pick the best match. Per-section time signatures are supported via the `@N/M` suffix (e.g. `"4:verse@7/8 2:fill@4/4"`).

**Layer mode:** Mix instrument layers from different cells. Four layer groups: `kick`, `snare` (includes snare_ghost, snare_rim), `cymbal` (hihat, ride, crash, china, splash), `toms`. Conflicts at the same position are resolved by priority (crash > ride > hihat, snare > tom). CLI: `--kick/--snare/--cymbal/--toms cell_name`. GUI: "Layer Mode" expander.

**Generative mode:** When `--generative` / `-g` is used, the assembler prefers probability grid cells from the style pool. Each seed produces a unique pattern. Use `--variations N` to generate N outputs with sequential seeds.

**Advanced humanization:** Four physics-based features that model real drumming, all scaling from the master humanize slider:
- **Velocity contour** — Wrist-pattern shaping for cymbals (ride, hihat). Downbeat sub=0.0 gets +6, weakest sub=0.75 gets -5. Beat 1 gets an extra +4 boost. Deterministic (no RNG).
- **Section push/pull drift** — Timing shifts per bar based on section type. Verses gradually drag (+ms), choruses/blasts constantly push (-ms), builds gradually push, breakdowns drag. Deterministic.
- **Kick-snare flam** — When kick and snare land on the same tick (both velocity > 75), kick is pulled 5-12ms earlier. Only fires at humanize >= 0.2. Uses RNG.
- **Ghost note clustering** — Ghost notes gravitate toward nearby snare accents. Style-dependent intensity via `_CLUSTER_TAG_AMOUNTS` (faraquet=0.7, shellac=0.0). May add paired ghost notes. Uses RNG.

Processing order in `_process_bar()`: position_to_ticks → swing → humanize_timing → section drift → humanize_velocity → velocity_contour → velocity_offset clamp. Flam and ghost clustering are applied as post-processing after all bars are assembled.

At humanize=0, all advanced features early-return with no RNG calls, preserving backward compatibility.

**Output:** MIDI files go to `output/` by default (CLI) or a configurable folder (GUI, defaults to `/mnt/c/Users/.../drumgen_output` for WSL-to-Windows access).

## Adding New Cells

1. Define a function in `cell_library.py` returning a cell dict (follow existing patterns).
2. Add the cell to the `CELLS` registry dict at the bottom of `cell_library.py`.
3. Add the cell name to relevant entries in `STYLE_POOLS`.
4. Use tags that match `SECTION_PREFERENCES` keys so arrangement mode can select the cell appropriately.

**Adding probability grid cells:** Same as above, but the cell dict uses `"type": "probability"` and `"grid"` (list of 5-tuples) instead of `"hits"`. Include `"generative"` in tags. Each grid entry has a probability (0.0-1.0) controlling how often it fires. Near-1.0 = nearly deterministic, lower = more variation between seeds.

## Importing Cells from MIDI

Imported cells are stored as JSON in `user_cells/` and loaded automatically into `CELLS` at import time. They auto-integrate into `STYLE_POOLS` based on tag-to-pool mapping (`TAG_TO_POOLS` in `cell_library.py`). They work seamlessly with `--cell`, `--list-cells`, arrangement mode, and the GUI.

**Import format:** Each JSON file contains a cell dict with `name`, `tags`, `time_sig`, `num_bars`, `humanize`, `role`, `hits` (flat 5-tuple list), `source: "imported"`, `source_file`, `content_hash`, and optionally `source_bpm`.

**Kit selection:** Use `--kit addictive_drums` when importing from Addictive Drums sources (note 48 = snare). Use `--kit ugritone` (default) for Ugritone sources. Kit mappings support an `aliases` field for multi-note-to-instrument mappings.

**Import pipeline features:**
- Content-based auto-tagging (`--auto-tag`): detects blast beats, halftime, backbeat, fills, density, ghost notes, odd meters, etc.
- Content hashing for dedup: identical patterns are skipped automatically
- Hit deduplication: multi-track merges resolved by keeping highest velocity
- Trailing bar trim: single-hit boundary bleed bars are removed
- Validation: rejects non-drum patterns (by name and content), pure-cymbal cells, and cells with <3 notes
- Non-drum track filtering in ALS extractor: synth/sampler/pad tracks blacklisted

**Full pipeline:** Use `als_extractor.py` to get .mid files from Ableton projects, then `midi_reader.py` to convert them to cells. Or upload .mid files directly through the Streamlit GUI's "Import MIDI as Cell" expander (supports preview, auto-tagging, and validation).

## Physical Constraints

Cells must respect real drummer limb constraints (documented in `styles/drumgen-style-dna.md` section 13): no ride+crash simultaneously, no hi-hat+ride simultaneously, no snare+tom simultaneously, no cymbals during fills (except crash at the end).
