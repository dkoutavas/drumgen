# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**Read `PROJECT.md` first.** It carries the project's purpose, aesthetics, core
beliefs (honesty, anti-generative-AI-music, DIY/open-source), current state,
hard-won lessons, and open threads. This file is the mechanical reference:
commands, architecture, conventions.

## Project Overview

drumgen is a pure-algorithmic drum MIDI generator — **no AI, no model, no
inference**. Hand-authored rhythmic "cells" are assembled by seeded algorithms,
humanized, and either written to `.mid` or emitted live from a VST3/CLAP plugin.
Primary host: **Bitwig Studio on native Linux**, driving Ugritone drums (via
yabridge). Targets screamo/emoviolence, post-hardcore, math rock, noise rock,
post-rock, atmospheric black metal, and jazz-on-hardcore ("zona").

Two frontends share one hand-duplicated engine: Python (authoring, CLI, GUI,
tools) and Rust (`plugin/`, the daily driver). Cells are data, exported from
Python into the plugin.

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

# Drummer notation: .mid -> MusicXML (open in MuseScore 4 -> PDF)
python notation.py ~/drumgen_output/take.mid
python drumgen.py --style zona --tempo 140 --bars 8 --musicxml   # generate + notate
./scripts/install-notation-hook.sh       # SAVE .MID in the plugin auto-notates

# Run tests (289 Python)
python -m pytest test_drumgen.py -v

# Validate the MIDI pipeline across many configurations
python validate_midi.py                  # quick mode (8 styles)
python validate_midi.py --full           # exhaustive matrix
python validate_midi.py --style shellac  # single style
```

### VST3/CLAP Plugin (Rust / nih-plug)

The plugin builds VST3 + CLAP. **Native Linux + Bitwig is the target** (Bitwig
loads either; prefer CLAP). Ableton Live is Windows/macOS only and does not
support CLAP — only the Windows VST3 artifact could ever load there.

**After installing, restart the DAW** — a loaded `.so` stays in memory, so
re-adding the device is not always enough to pick up a new build.

```bash
# Regenerate the embedded cell library after editing cell_library.py
python export_cells.py                   # writes plugin/cells/builtin.json

# Native Linux build + install (-> ~/.vst3 and ~/.clap)
cd plugin && ./build-linux.sh            # --check to build without installing

# Cross-compile a Windows VST3/CLAP from Linux (mingw) -> plugin/dist/windows/
# (needs: rustup target add x86_64-pc-windows-gnu
#         openSUSE: sudo zypper in mingw64-cross-gcc)
cd plugin && ./build-windows.sh          # --install DIR to also copy the bundle

# Run Rust tests (73)
cd plugin && cargo test

# CI (.github/workflows/build-plugin.yml) builds Linux + Windows (native MSVC) +
# macOS bundles on push; a v* tag cuts a release. Prefer the CI MSVC artifact
# over the mingw cross-build for anything you actually install on Windows.
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

A second frontend exists as a Rust VST3/CLAP plugin (`plugin/`) that ports the same engine and emits MIDI in real time inside a DAW instead of writing files (see "VST3 Plugin" below).

- `drumgen.py` — CLI entry point (argparse). Parses args, delegates to `assemble()`, `assemble_arrangement()`, or `assemble_layered()`, then `write_midi()`. Supports `--generative`, `--variations`, and layer mode (`--kick/--snare/--cymbal/--toms`). `OUTPUT_DIR` auto-detects WSL and defaults to `C:\Users\%USERNAME%\Documents\drumgen_output`. `_ts_suffix()` appends time signature to filenames for non-4/4 meters.
- `app.py` — Streamlit GUI. Same generation pipeline as CLI. Sidebar organized into visual sections (Output, What, Sound, Feel, Modes, Generate). Output folder defaults to Windows Documents on WSL (auto-detects `%USERNAME%`). "Open folder" button uses `wslpath -w` for reliable WSL→Windows path conversion (handles both `/mnt/` and native WSL paths). WSL caption shows the Windows-equivalent path. Mode-aware controls: Style/Cell disabled when layer mode active, Bars disabled in arrangement mode. Layer Mode expander includes clear button and active summary. Arrangement mode has meter quick-add selector for per-section time signatures. Smart filename auto-updates based on active mode (generative prefix, layer names, cell override, time signature for non-4/4). Generate button in both sidebar and main area. Post-generate layout: compact success row with download + open folder, audio preview with optional auto-play, pattern grid in collapsible expander, params/stats collapsed. MIDI import expander at bottom of sidebar.
- `assembler.py` — Core orchestrator. Four main functions:
  - `assemble()` — Single-cell mode: repeats a cell for N bars, inserting fills if requested. Supports `generative=True` to prefer probability grid cells.
  - `assemble_arrangement()` — Multi-section mode: parses arrangement strings like `"4:build 8:drive@7/8 2:blast"`, picks best cell per section via tag scoring. Supports per-section time signatures via `@N/M` suffix and `generative=True`.
  - `assemble_layered()` — Layer mode: mixes instrument layers (kick/snare/cymbal/toms) from different cells into one pattern, with conflict resolution.
  - Also handles: hit normalization (4-tuple to 5-tuple), probability grid realization (`realize_probability_grid`), physical constraint validation, layer extraction/conflict resolution, variation mutations (`vary_hits`), velocity drift per section, per-bar humanize overrides. All three assemble functions apply advanced humanization post-processing (flam, ghost clustering) after bar loop and pass section drift into `_process_bar()`.
- `cell_library.py` — All rhythmic cells defined as Python functions returning dicts. Includes both fixed cells (with `hits`) and probability grid cells (with `type: "probability"` and `grid`). Contains `CELLS` registry, `STYLE_POOLS` (style -> list of cell names), `SECTION_PREFERENCES` (section type -> preferred tags), and lookup functions (`get_cell`, `get_pool`, `get_cell_for_section`). Loads user-imported cells from `user_cells/` via `load_user_cells()` and auto-integrates them into `STYLE_POOLS` via `TAG_TO_POOLS` tag-to-pool mapping.
- `test_drumgen.py` — Comprehensive test suite (pytest, 289 tests). Covers cell integrity, time signatures, style pools, assembler, humanizer, MIDI engine, end-to-end generation, probability grids, layer mode, mixed meters (including note position verification), variations, Phase 3 style palette expansion, advanced humanization (velocity contour, flam, section drift, ghost clustering, seed reproducibility), MIDI duration overshoot verification for odd meters, Stage-1 shaped randomness (trig conditions, Euclidean realization, steering determinism), section dynamics, into-aware fill sections, the zona pool's format discipline, and the notation round-trip.
- `midi_reader.py` — Standalone CLI + importable library. Reads `.mid` files via mido, converts to drumgen's native cell format (flat 5-tuple hits), saves as JSON in `user_cells/`. Includes content-based auto-tagging (`auto_tag_cell()`), validation (`validate_cell()`), hit deduplication, trailing bar trim, and content hashing for dedup. Auto-generated cell names include `_{bpm}bpm` suffix when BPM metadata is available (explicit `--name` is not affected). Exposes `midi_to_cell()`, `save_cell()`, `auto_tag_cell()`, `validate_cell()` for GUI use.
- `als_extractor.py` — Standalone CLI. Opens `.als` files (gzip-compressed XML), finds MidiClip elements from both Session and Arrangement views, writes each as a `.mid` file to `extracted/`. Filters non-drum tracks via name blacklist (synth, sampler, pad, etc.) when `--drums-only` is used.
- `humanizer.py` — `Humanizer` class with seeded RNG. Per-instrument velocity variance tables (25 instruments), timing tendencies (e.g., snare slightly late, ride slightly early), swing application. Advanced humanization: velocity contour (wrist pattern for cymbals including hihat_wide_open + beat-1 emphasis), section push/pull drift (verse drags, chorus pushes, build gradually pushes), kick-snare flam (kick pulled 5-12ms early on simultaneous hits), and ghost note clustering (ghosts gravitate toward snare accents, style-dependent via `_CLUSTER_TAG_AMOUNTS`). Module-level helpers: `get_cluster_amount(cell)`, `infer_section_type(cell)`.
- `midi_engine.py` — Position-to-tick math and MIDI file writing via `mido`. Constants: PPQ=480, note duration=30 ticks, MIDI channel=9. Includes note overlap prevention (inserts early note_off when humanizer timing causes pitch collisions). Note_off events are clamped to the expected bar boundary so MIDI clips don't overshoot the grid in DAWs (especially important for odd meters). Time signature meta messages and note events are interleaved in a single sorted pass (by absolute tick) to avoid mixed-meter delta calculation bugs. Resolves kit aliases so aliased instrument names map to the correct MIDI note. `unique_filepath()` utility auto-increments filename suffix (`_1`, `_2`, etc.) to prevent overwriting existing files — used by both CLI and GUI.
- `notation.py` — `.mid` → drum-staff **MusicXML** for a human drummer. Snaps ticks to the sixteenth grid (lossless: the whole vocabulary is 16th-based), recovers velocity levels via `midi_reader._classify_velocity`, emits two voices (hands stems-up / feet stems-down with `<backup>`), x noteheads for cymbals, circle-x for open hats, **parenthesized ghost snares**, accents, and a fresh `<time>` at every meter change. `<midi-unpitched>` is 1-based (classic silent off-by-one). Open the result in MuseScore 4 → PDF. See `NOTATION.md`.
- `scripts/install-notation-hook.sh` — Writes `~/.config/drumgen/on_save`, the executable the plugin spawns after SAVE .MID (detached, fire-and-forget). Installing it makes every plugin save also render a score.
- `validate_midi.py` — Standalone validation script (also runnable via pytest). Generates MIDI across many style/cell/meter/tempo configurations and checks pipeline correctness (bar alignment, note bounds, round-trip via `midi_to_cell`).
- `run-drumgen` — Bash launcher script. Activates venv and runs `streamlit run app.py`. On WSL, sets `BROWSER=explorer.exe` so Streamlit auto-opens in the Windows default browser.
- `preview.py` — Optional FluidSynth-based WAV rendering for the Streamlit GUI.
- `kit_mappings/` — JSON files mapping instrument names to MIDI note numbers. Default: `ugritone.json` (25 instruments including chokes, fx cymbals, ride_crash, china_2, hihat_wide_open, tom_mid_high). Also `addictive_drums.json` (note 48 = snare) and `general_midi.json`. Kit files support an `aliases` field for additional note-to-instrument mappings (e.g., `floor_tom` → `tom_floor`).
- `user_cells/` — Directory for imported cell JSON files (gitignored). Loaded automatically by `cell_library.py` at import time.
- `extracted/` — Directory for MIDI files extracted from .als projects (gitignored).
- `styles/drumgen-style-dna.md` — Reference document describing rhythmic vocabulary per genre (blast beats, d-beats, Shellac precision, etc.). Used as build-time guidance, not consumed by code.

### VST3 Plugin (`plugin/`)

A Rust port of the core engine as a VST3/CLAP plugin (nih-plug). Pure MIDI
generator, no audio processing: it reads the DAW transport and emits notes that
drive a drum sampler **after it in the same Bitwig device chain** (see
`BITWIG.md`). A dummy stereo output exists only because some DAWs refuse to load
a plugin without one — **never remove it**.

- `plugin/src/engine/` — Hand-duplicated port of the Python engine: `midi_math.rs` (PPQ=480, position→tick), `cell.rs` (instruments, `TrigCond`, `Limb`), `humanizer.rs`, `assembler.rs`, `cell_library.rs`. **When changing engine logic in Python, mirror it here** (and vice versa).
- `plugin/src/lib.rs` — nih-plug `Plugin` impl. Change detection (discrete params regenerate instantly; continuous ones use a 150ms settle), immediate pattern swap with note flush, `u128` active-note mask, host tempo + time signature reading, and **one relaxed `AtomicI64` store per buffer** publishing the playhead tick for the GUI. That store is the audio thread's only extra work — keep it that way.
- `plugin/src/params.rs` — DAW-automatable params: `style`, `humanize`, `bars`, `seed`, `swing`, `meter`, `fill`, `song`. Also owns `METERS`/`FILLS`/`SONGS` tables, the `songs.txt` loader (`OnceLock`, strict validator, starter-file planting), and the editor size + persisted `editor-state-v2` key.
- `plugin/src/worker.rs` — `GenWorker`: the generation thread. `GenRequest` in, `Arc<Pattern>` out, crossbeam channels with latest-wins coalescing. `request()` returns whether the send was accepted — the caller must not record a dropped request as sent.
- `plugin/src/pattern.rs` — Immutable baked `Pattern`: sorted events, `bar_starts`, `time_signatures`, `sections`, tempo, and the **raw display seed** (not the salted one).
- `plugin/src/playback.rs` — Stateless, allocation-free scan of a tick window into a reused scratch `Vec`; pattern-relative offsets (an absolute-position bug once collapsed all humanization after loop 1).
- `plugin/src/generation.rs` — `GenerationManager`: owns the `CellLibrary`; `generate()` (loop mode) and `generate_arrangement()` (Song Mode). Applies the two plugin-boundary policies: **FNV-1a style-name seed salt** and the **vary floor** when no probability cell is reachable.
- `plugin/src/editor.rs` — The 8-bit GUI (see Key Concepts). 720×440, resizable grip, telegraph, horizon strip with playhead cursor, step grid, DICE/SEED/SAVE.
- `plugin/src/export.rs` — Hand-rolled SMF (format 0) writer for SAVE .MID, plus the `on_save` hook spawn.
- `plugin/cells/builtin.json` — Built-in cells + style pools + section preferences embedded into the plugin. **Generated** by `export_cells.py` from `cell_library.py` (imported user cells are excluded) — rerun the export after adding or editing built-in cells.
- `plugin/build-windows.sh` — Cross-compiles from WSL2 with `cargo xtask bundle` (mingw-w64 target) and installs to `C:\Program Files\Common Files\VST3\`.
- `export_cells.py` — Serializes `CELLS`/`STYLE_POOLS`/`SECTION_PREFERENCES` to `plugin/cells/builtin.json`.
- `live_player.py` — Real-time MIDI player (python-rtmidi) that streams drumgen patterns into a virtual MIDI port (loopMIDI, port named "drumgen") for Ableton. Must run on Windows Python — WSL2 cannot access Windows MIDI devices.

## Key Concepts

**Cell format:** Each cell is a dict with `name`, `tags`, `time_sig`, `num_bars`, `humanize`, `role` (groove/fill/transition), and `hits`. Single-bar cells use 4-tuples `(beat, sub, instrument, velocity_level)`. Multi-bar cells use 5-tuples `(bar, beat, sub, instrument, velocity_level)`. Sub values: 0.0=on beat, 0.25=sixteenth, 0.5=eighth, 0.75=dotted eighth.

**Probability grid cells:** Cells with `type: "probability"` use `grid` instead of `hits`. Grid entries are 5-tuples `(beat, sub, instrument, probability, velocity_level)` for single-bar or 6-tuples `(bar, beat, sub, instrument, probability, velocity_level)` for multi-bar. Each entry has a probability (0.0-1.0) that determines whether the hit is included on each realization. Different seeds produce different patterns from the same grid. Physical constraints (limb conflicts) are validated after realization.

**Euclidean cells:** `type: "euclidean"` with `limbs`, each `{instrument, pulses, steps, rotation, velocity, dice_rotate}`. Each limb tiles its own `steps`-slot cycle across the pattern with **no bar reset** (polymeter), so co-prime limbs phase for many bars. `dice_rotate: true` limbs take a seed-derived rotation (the dice re-voices them); anchors hold. Slots are sixteenths: 4 per beat in /4 meters, 2 per beat in /8. Deterministic — consumes no RNG.

**Trig conditions:** an optional trailing string on a probability-grid entry, evaluated **before** the roll (so a failed condition consumes no RNG): `"A:B"` fires on pass A of every B cell cycles, `"1st"`/`"last"` gate by pattern position, `"pre"`/`"!pre"` react to whether the previous entry in that bar fired. This is how cells get memory and phrase logic. Malformed conditions fail open in both engines.

**Shaped randomness:** `realize_probability_grid` applies a **syncopation guard** (a bar whose kick/snare offbeat ratio leaves `[0.10, 0.60]` is re-rolled once — Witek et al.'s inverted-U) and a **tension envelope** ramping mid-band probabilities across the pattern, scaled per section. Tuning knobs: `SYNC_LO/SYNC_HI`, `TENSION_START/TENSION_END` at the top of the Stage-1 block in `assembler.py` and `assembler.rs` — **keep them in sync**.

**Section dynamics:** `SECTION_DYNAMICS` (assembler.py) / `section_dynamics()` (assembler.rs) maps each section type to `(vel_base, vel_slope_per_bar, tension_mult)`. A build starts −12 velocity and climbs ~15 over 8 bars while running denser; atmospheric sits −14 and thin; blast lands +7 and thick. Supersedes the old up/down drift.

**Song Mode:** the `song` param picks an arrangement string from `SONGS` (params.rs) — 10 built-in forms grounded in real records — or from the user's `~/.config/drumgen/songs.txt` (`Name | 3:verse@7/8 2:blast …`, loaded once at instantiation, strict validator, whole-line reject with `nih_log`, starter file planted when missing). Song Mode drives the same `assemble_arrangement` the Python CLI uses; BARS/METER/FILL are ignored while a song is active.

**Fill sections:** a `fill` section resolves via `get_cell_for_section`'s special case — `role == "fill"` cells library-wide (fills deliberately live outside style pools), meter-filtered, preferring cells tagged `into_<next_section>`. In loop mode the FILL param swaps a meter-matched fill in every N bars, drawn **per fill bar** so consecutive fills differ.

**Velocity levels:** `ghost`, `soft`, `normal`, `accent` — mapped to numeric ranges by the Humanizer.

**Style pools vs STYLE_MAP:** `STYLE_POOLS` maps a style name to a list of cell names (used in arrangement mode for section-aware selection). `STYLE_MAP` is a backward-compat shortcut mapping each style to its first cell.

**Arrangement mode:** Parses `"N:section_type"` or `"N:section_type@num/den"` tokens. Section types (intro, build, verse, chorus, drive, blast, breakdown, atmospheric, silence, fill, outro) have preferred tags in `SECTION_PREFERENCES`. The assembler scores pool cells against these preferences to pick the best match. Per-section time signatures are supported via the `@N/M` suffix (e.g. `"4:verse@7/8 2:fill@4/4"`).

**Layer mode:** Mix instrument layers from different cells. Four layer groups: `kick`, `snare` (includes snare_ghost, snare_rim), `cymbal` (hihat, hihat_wide_open, ride, ride_crash, crash, crash chokes, china, china_2, splash, fx_cymbal_1/2), `toms` (tom_high, tom_mid_high, tom_mid, tom_low, tom_floor). Conflicts at the same position are resolved by priority (crash/china > ride_crash/fx/splash > ride > hihat, snare > tom). CLI: `--kick/--snare/--cymbal/--toms cell_name`. GUI: "Layer Mode" expander.

**Generative mode:** When `--generative` / `-g` is used, the assembler prefers probability grid cells from the style pool. Each seed produces a unique pattern. Use `--variations N` to generate N outputs with sequential seeds.

**Advanced humanization:** Four physics-based features that model real drumming, all scaling from the master humanize slider:
- **Velocity contour** — Wrist-pattern shaping for cymbals (ride, hihat, hihat_wide_open). Downbeat sub=0.0 gets +6, weakest sub=0.75 gets -5. Beat 1 gets an extra +4 boost. Deterministic (no RNG).
- **Section push/pull drift** — Timing shifts per bar based on section type. Verses gradually drag (+ms), choruses/blasts constantly push (-ms), builds gradually push, breakdowns drag. Deterministic.
- **Kick-snare flam** — When kick and snare land on the same tick (both velocity > 75), kick is pulled 5-12ms earlier. Only fires at humanize >= 0.2. Uses RNG.
- **Ghost note clustering** — Ghost notes gravitate toward nearby snare accents. Style-dependent intensity via `_CLUSTER_TAG_AMOUNTS` (faraquet=0.7, shellac=0.0). May add paired ghost notes. Uses RNG.

Processing order in `_process_bar()`: position_to_ticks → swing → humanize_timing → section drift → humanize_velocity → velocity_contour → velocity_offset clamp. Flam and ghost clustering are applied as post-processing after all bars are assembled.

At humanize=0, all advanced features early-return with no RNG calls, preserving backward compatibility.

**Plugin GUI (8-bit):** Sweetie-16 palette, Press Start 2P at 8/16px, feathering off, `CornerRadius::ZERO`. 720×440 with a plugin-side resize grip (the DAW's own border cannot resize it — nih-plug limitation); the detail grid absorbs any dragged space. Elements: style picker, HUMANIZE/SWING pixel knobs, BARS/METER/FILL/SONG steppers, DICE (seed+1) + drag-scrub SEED + SAVE .MID, a **telegraph** countdown line (`CHORUS ▸ BLAST IN 2` → `▸ BLAST NOW`), a **horizon strip** (current bar + next three in miniature with a sweeping playhead cursor), and the step grid (6 lanes × up to 24 sixteenths, velocity-shaded, pageable, BAR/NOW/NEXT follow modes). Repaint is 16ms while playing, 100ms stopped. **If you change the window size, bump the `#[persist = "editor-state-vN"]` key** or saved projects will restore the old size forever.

**Output:** The plugin writes to `~/drumgen_output/` (auto-incrementing filenames, time signature in the name for non-4/4) and then spawns `~/.config/drumgen/on_save` if present. The Python CLI/GUI default to `output/` on Linux (legacy WSL/Windows detection remains in `drumgen.py`/`app.py`); the GUI folder is configurable and persisted in `.drumgen_config.json`.

## Adding New Cells

1. Define a function in `cell_library.py` returning a cell dict (follow existing patterns).
2. Add the cell to the `CELLS` registry dict at the bottom of `cell_library.py`.
3. Add the cell name to relevant entries in `STYLE_POOLS`.
4. Use tags that match `SECTION_PREFERENCES` keys so arrangement mode can select the cell appropriately.

**Adding probability grid cells:** Same as above, but the cell dict uses `"type": "probability"` and `"grid"` instead of `"hits"`. Include `"generative"` in tags. Entries are 5-tuples `(beat, sub, inst, prob, vel)`, 6-tuples (either `(bar, …)` or `(…, cond)` — disambiguated by whether position 2 is a string), or 7-tuples with both.

**Authoring lesson (important):** probability where *structure* belongs reads as noise. Give a cell a dependable spine at 0.85–0.95 and let the dice pick the *variation*, not whether the groove exists. Two-bar phrase logic via trig conditions (`1:2` states a figure, `2:2` answers it) is what makes a cell sound like a drummer with intention — see the `zona` pool for the reference implementation.

**Adding Euclidean cells:** `"type": "euclidean"` with a `limbs` list. Keep one or two limbs as anchors (`dice_rotate: false`) so the groove has ground, and give the rest co-prime `steps` so they phase.

**After ANY `cell_library.py` edit:** run `python export_cells.py` to regenerate `plugin/cells/builtin.json`, then rebuild the plugin. Alias styles (`math`→`faraquet`, `blood_brothers`→`atdi`, `dry_cleaning`→`preoccupations`) are skipped at export while their pools stay byte-identical — if you add a cell to one, add it to its twin.

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

Cells must respect real drummer limb constraints (documented in `styles/drumgen-style-dna.md` section 13): no ride+crash simultaneously, no hi-hat+ride simultaneously, no snare+tom simultaneously, no cymbals during fills (except crash at the end). Hand+foot combinations are legal (`hihat_pedal` counts as a foot and coexists with cymbals). The assembler enforces these after realization via `_validate_physical_constraints` / `validate_physical_constraints`.

## Working agreements

- **Nothing is "done" until it has been heard in the DAW.** Tests green + build installed = ready to verify, not verified. Say so in commit messages.
- **Mirror engine changes** across `*.py` and `plugin/src/engine/*.rs`. Cross-engine RNG bit-parity is impossible (Mersenne Twister vs ChaCha8) and not a goal; structural parity is.
- **Determinism is a feature**: same seed + same params = identical notes. Never introduce unseeded randomness or `HashMap` iteration into an RNG-consuming path (use `BTreeMap`).
- **The audio thread** does no allocation, no blocking lock, and no generation. Publishing state to the GUI = one relaxed atomic store.
- **Deliberate shortcuts** get a `ponytail:` comment naming the ceiling and the upgrade path.
- Run `python -m pytest test_drumgen.py -q` **and** `cd plugin && cargo test` before committing anything that touches shared behavior.
