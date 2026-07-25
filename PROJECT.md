# drumgen — project primer

> Read this first. It is the whole context: what this tool is, who it's for,
> what we believe, where the code stands, and what comes next. Written to be
> handed to a fresh collaborator (human or agent) so they arrive with the
> *why*, not just the files.

---

## 1. What this is

A **drum-part generator for labyrinthine heavy music**, built as:

- a **native Linux VST3/CLAP plugin** (Rust, nih-plug) — the daily driver,
  used inside **Bitwig Studio** to write demos and jam guitar over;
- a **Python engine + CLI + Streamlit GUI** — the authoring workshop where
  rhythmic material and tooling get built and tested;
- a **notation pipeline** — generated parts become standard drum charts a
  human drummer reads cold.

It generates MIDI in real time from hand-authored rhythmic "cells" assembled
by seeded algorithms. **No machine learning. No model weights. No inference.**
Every note traces back to a musical decision someone made on purpose.

Target genres: screamo / emoviolence / skramz, post-hardcore, math rock, noise
rock, post-rock, atmospheric black metal, and the jazz-on-hardcore corner
(Monster Machismo, Zona Mexicana). Reference shelf: Orchid, Saetia, pg.99,
Daitro, Raein, La Quiete, Mihai Edrisch, Lord Snow, Ampere, Louise Cyphre,
Hot Cross, Sweep the Leg Johnny, City of Caterpillar, Envy, Fugazi, Faraquet,
Drive Like Jehu, At the Drive-In, Shellac, Slint, Unwound, Deafheaven, Liturgy.

## 2. Who it's for

**The author**: a 33-year-old guitarist with a full-time job, writing dense
mathy riffs in an office, in a band with an excellent improvising drummer. Not
a drummer himself. Has decades of listening taste and limited hours.

The tool exists so that **writing a demo does not require programming drums
by hand for an evening**. It replaces the session drummer he doesn't have at
his desk — not the composer, and not the drummer in the room.

Also for: the gen-Z bedroom skramz kids resurging in the 2020s, and the elder
emos raised on the classics. A niche tool for a niche scene, made properly.

## 3. What we believe (non-negotiable)

**Honesty.** Never claim something works because tests pass. Nothing is
"done" until it has been *heard* in the DAW. Commit messages record what is
unverified. When a diagnosis is wrong, say so and find the real cause. Every
big feature in this project's history was gated on the author's ears, and
several confident fixes turned out to be wrong — that record stays visible.

**Anti-slop, pro-tool.** Suno-style generative music replaces the composer;
we refuse that. This replaces the *session player*. The author still chooses
the form, the seed, the keepers, where the stops land, and what gets deleted.
The creative decisions stay human. That distinction is the project's spine —
protect it in every design choice.

**Creativity over correctness theatre.** Musical payoff per session beats
architectural elegance. When output feels dead, the fix is usually musical
(phrasing, dynamics, structure), not more infrastructure.

**Practicality / DIY.** Laziest thing that works ships first (ponytail
principle: YAGNI, stdlib before deps, one line before fifty). Deliberate
shortcuts get a `ponytail:` comment naming the ceiling and the upgrade path.
No dependency gets added for what a hundred lines can do (the SMF writer and
the MusicXML writer are both hand-rolled for this reason).

**Open source, self-hosted, no cloud.** Everything runs on the author's Linux
box. Public repo, free CI, no accounts, no telemetry, no services.

**Determinism as a feature.** Same seed + same settings = the exact same
notes, forever. A groove you loved is recoverable by writing down a number.

**Taste is data.** The cell library is a curated vocabulary, not a dataset.
The author's ear is the training signal, applied at authoring time.

## 4. Where it stands (2026-07-25)

`main` is tagged **v0.1.0** — the first ear-verified release (CI builds Linux
/ Windows / macOS artifacts). Active work lands on **`plugin-hardening`**
(currently ~7 commits ahead), merged to main only after the author confirms
by listening.

**Verified in Bitwig by ear**: styles are distinct, Song Mode arcs work
("skramz arc build up actually works"), zona grooves after a phrasing rework,
telegraph "alright", audio takes recorded successfully, and a generated
`atdi` take rendered to a correct PDF drum chart (3/4 → 7/8 → 3/4, ghosts in
parentheses, x-notehead cymbals, accents) — the notation chain is proven
end-to-end.

**Scale**: 106 cells (27 probability grids, 5 Euclidean, 26 fills) across 29
plugin style pools (32 Python pools incl. CLI-only aliases), 10 built-in song
forms plus unlimited user forms, ~5,500 lines of Rust plugin, ~5,200 lines of
Python engine/tools, **286 Python + 71 Rust tests green**.

### Feature inventory

| Area | State |
|---|---|
| Style pools | 29 in plugin; every style produces distinct MIDI (regression-tested) |
| Cell types | fixed hits, probability grids (per-seed realization), Euclidean limbs (polymeter) |
| Trig conditions | `A:B` pass ratios, `1st`, `last`, `pre`, `!pre` — memory across the pattern |
| Shaped randomness | syncopation guard (Witek inverted-U band), tension envelope per pattern |
| Song Mode | 10 forms (Verse/Chor, Skramz Arc, Stop/Go, Quiet/Loud, Eruption, Post-Rock, Blast Fwd, Labyrinth, Ampere) + `~/.config/drumgen/songs.txt` |
| Section dynamics | per-section velocity base/slope + tension multiplier (builds rise and thicken) |
| Fills | 26 cells, meter-matched, chosen by `into_<next_section>` intent, alternating per fill bar |
| Humanization | velocity variance, timing tendencies, swing, wrist contour, section drift, kick-snare flam, ghost clustering |
| Meter | host time signature followed live in Auto; forced 3/4 · 4/4 · 5/4 · 6/4 · 6/8 · 7/8; per-section `@N/M` |
| GUI | 8-bit (Sweetie-16 + Press Start 2P), 720×440 resizable, horizon strip w/ sweeping playhead cursor, telegraph countdown, step grid, pageable bars |
| Export | SAVE .MID (hand-rolled SMF) + `on_save` hook → auto MusicXML score |
| Notation | `notation.py` → MusicXML → MuseScore 4 (installed via Flatpak) → PDF |
| RT safety | generation on a worker thread; audio thread does one relaxed atomic store, no allocation, no locks that block |

## 5. Architecture

```
       Python (authoring + tools)              Rust plugin (performance)
  ┌───────────────────────────────┐      ┌────────────────────────────────┐
  │ cell_library.py  (vocabulary) │      │ engine/cell_library.rs         │
  │ assembler.py     (assembly)   │─────▶│ engine/assembler.rs            │
  │ humanizer.py     (feel)       │ port │ engine/humanizer.rs            │
  │ midi_engine.py   (ticks/SMF)  │      │ engine/midi_math.rs            │
  └───────────────────────────────┘      ├────────────────────────────────┤
        │              │                  │ worker.rs   (gen thread)      │
        │              └── export_cells.py│ generation.rs (params→pattern)│
        │                    ↓ builtin.json│ pattern.rs  (immutable bake)  │
        │              (cells embedded)   │ playback.rs (tick→sample scan)│
        ▼                                 │ lib.rs      (nih-plug host)   │
  drumgen.py / app.py                     │ editor.rs   (8-bit GUI)       │
  notation.py / midi_reader.py            │ export.rs   (SMF + hook)      │
  als_extractor.py                        └────────────────────────────────┘
```

**The load-bearing rule**: the engine is *hand-duplicated* across languages.
Any change to assembly, humanization, or tick math must land in **both**
`*.py` and `plugin/src/engine/*.rs`, or the plugin and the CLI drift. Cells
are data — edit `cell_library.py`, run `python export_cells.py`, done.

**Cross-engine bit-parity is impossible** (Mersenne Twister vs ChaCha8) and
that's accepted; parity is asserted on structure and behavior, not bytes.

**Plugin-boundary policy**: seeds are salted with the style name in
`generation.rs` (so two styles never share an RNG stream), and a "vary floor"
engages when no probability cell is reachable. These live at the boundary, not
in the engine, so the engine stays a faithful port.

## 6. How to work on it

```bash
# Python side
source .venv/bin/activate
python -m pytest test_drumgen.py -q          # 286 tests
python validate_midi.py                      # pipeline sanity
python export_cells.py                       # after ANY cell_library.py edit

# Rust side
cd plugin && cargo test                      # 71 tests
./build-linux.sh                             # → ~/.vst3 + ~/.clap

# Then: restart Bitwig (a loaded .so stays in memory), re-add the device.
```

**The loop that works**: implement → both suites green → build/install → the
author listens in Bitwig → fix what the ears reject → commit with an honest
message → push. Merges to `main` wait for ear confirmation.

**Multi-agent orchestration** (the `ultracode` keyword) has been used
repeatedly and productively: parallel research crews for design questions,
and an adversarial verification workflow that has caught genuine bugs
(silent Euclidean phasing loss in arrangement mode, request stranding on a
full worker queue, a dead vary floor under forced meters, a seed-label race).
Use it for anything ambiguous or risky; it earns its cost.

## 7. Hard-won lessons (do not relearn these)

1. **Probability where structure belongs is noise.** A drummer repeats a
   motif and varies the *answer*. Comping cells use two-bar phrase logic:
   odd passes state a dependable figure (0.85–0.95), even passes answer it.
   The dice picks *which* answer, never *whether* the groove exists. This
   single change turned zona from "random" into "grooving".
2. **Silence is a spice, not a texture.** Cutting silence sections out of
   Labyrinth and trimming Eruption/Quiet-Loud made the forms move.
3. **The plugin holds the `.so` in memory** — reinstalling isn't enough;
   restart the DAW when verifying a build.
4. **`EguiState` persistence stomps new window defaults.** Bump the
   `#[persist = "editor-state-vN"]` key in the same commit as any size change.
5. **Realized cell hits (probability *and* Euclidean) are keyed by output
   bar** — arrangement/layered paths must key on `i+1`, never the fixed-cell
   modulo, or phasing silently dies.
6. **Fills must match the bar's meter**, or a 4/4 fill overflows a 3/4 bar
   into hung notes.
7. **Never feed raw humanized MIDI to a notation app** — it becomes 32nd
   soup. Snap to the sixteenth grid (lossless here: the whole vocabulary is
   16th-based, jitter is under half a sixteenth).
8. **MuseScore 4 has no MIDI import panel.** MusicXML is the only clean door.
9. **Guitar Pro 5 is a file format, not an application** (Wine is a dead end;
   TuxGuitar converts MusicXML → .gp5 if a drummer insists).
10. **The audio thread gets exactly one relaxed atomic store** for the GUI
    playhead. Everything else derives GUI-side.
11. **Features that exist only as files do not exist.** songs.txt and the
    notation pipeline both had to be surfaced (starter file planted on first
    run; save hook; hover texts) before the author found them.

## 8. Open threads

**Awaiting the author's ears**: the grid-grows-with-window change; Labyrinth's
full verdict; whether zona's 10-seed gate passes; merge `plugin-hardening` →
`main` (+ a v0.2.0 tag) once it does.

**Queued, gated on need**:
- Phrase-logic retrofit for older chatty grids (euro_screamo, posthardcore).
- Corpus stage: mass-import the author's years of Ableton projects
  (`als_extractor.py` → `midi_reader.py`), then the missing piece — a
  **cells→probability-grid synthesizer** that turns clusters of imported
  patterns into generative grids in his own voice. Blocked on nothing but
  need; the trigger is "output sounds generic-drumgen rather than like me".
- ML sits behind an explicit, possibly-never trigger: only if phrase-level
  development is still missing after real corpus use, and then only as an
  **offline** cell breeder in Python (never runtime inference), so the
  no-AI-at-runtime ethos survives.

**Naming**: "drumgen" is a placeholder. Greek direction chosen. Shortlist:
**Áno Teleía** (άνω τελεία — the raised dot: a pause that isn't a full stop;
the author's own phrase, and exactly how this music breathes), **Palmós**
(pulse), **Chásma** (the gap), **Anása** (breath), **Diastolí** (the heart's
expansion / the musical pause mark), **Ripí** (burst). Rename touches the
display name and docs only — `VST3_CLASS_ID` and `CLAP_ID` stay frozen so
saved projects never break.

## 9. Working style that fits this author

Terse, technical, no cheerleading. He is burnt out and has taste — respect
both. Lead with the outcome. When he reports something feels wrong, believe
him and find the mechanism; his instincts have been right every time so far
(the fills *were* mechanically static; the styles *were* bit-identical; the
grid *was* clipped). Ask before assuming a musical judgment. Ship in evening-
sized pieces with a listen at the end of each. Two behavioral modes are
usually active in his sessions: **caveman** (terse prose; normal prose in
code, commits, and security notes) and **ponytail** (laziest working
solution). Both are hook-enforced — honor them.

His guitar context, for pairing suggestions: tuning **B–G♯–C♯–F♯–B–D♯**,
mathy dissonant riffing, labyrinthine forms, currently in a fertile writing
week and using the tool to write for a real band.

---

*Documentation map: `README.md` (install + usage), `CLAUDE.md` (repo
conventions, commands, architecture detail), `BITWIG.md` (wiring, recording,
yabridge), `NOTATION.md` (drummer handoff), `styles/drumgen-style-dna.md`
(genre rhythmic vocabulary reference).*
