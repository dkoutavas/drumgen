# Generated arrangements — design note

Status: proposal, 2026-10-12. No code yet. PROJECT.md §8 queue item 3 ("the
reason the bank exists … needs a plan session before code"). This is that
plan. The open questions at the end are the author's to answer before the
first evening of code.

## What it is

A generated arrangement is a **song form**: a sequence of sections with bar
counts, optional fills and silences, and occasional meter turns. It is the
same string the 9 built-in forms and `songs.txt` already use:

```
2:intro 6:build 1:fill 4:blast@7/8 2:breakdown
```

Using that grammar is the whole trick. Every existing path already consumes
it: the strict `valid_arrangement` validator, `assemble_arrangement` in both
engines, the telegraph and horizon strip, SAVE .MID (time signatures
included), the notation chain, and the bank. A generated form is just a form
nobody typed.

It never generates notes. Cells still come from the style's authored pool,
chosen by section intent exactly as in Song Mode today.

## Why it fits the project's line (PROJECT.md §3)

The composer stays the author. The generator proposes a skeleton from
hand-authored **templates** (data, like cells: "taste is data"), the author
picks the shape, rolls until a form suits the riff, keeps or discards it. No
model, no inference; seeded choices over authored options, the same kind of
shaped randomness as probability grids.

## Templates: shapes abstracted from the shipped forms

The 9 built-in forms reduce to five shapes. Each template is a list of slots;
a slot names the section types it may become, a bar range, and whether it is
optional. Proposed set, with the records the forms cite:

| shape | from | slots (type: bars) |
|---|---|---|
| **Verse/Chorus** | Verse/Chor (Fugazi, ATDI) | intro 1–2 · verse 4–8 · chorus 2–4 · fill? 1 · verse 4–8 · chorus 4–8 · outro 1–4 |
| **Arc** | Skramz Arc, Post-Rock (Daitro, City of Caterpillar, Envy) | intro 1–4 · build 4–8 · fill? 1 · blast/drive 3–8 · landing (breakdown/atmospheric/outro) 1–4 |
| **Stop/Go** | Stop/Go, Eruption, Ampere (Orchid, pg.99, Ampere) | 3–5 × (burst: blast/chorus 1–3, then silence? 1) · landing breakdown 2–4 |
| **Quiet/Loud** | Quiet/Loud (Saetia) | atmospheric 2–4 · drive/chorus 2–4 · silence? 1 · atmospheric 2–4 · blast 3–6 · outro 2–4 |
| **Maze** | Labyrinth, Blast Fwd (Lord Snow) | intro 1–2 · 4–7 short sections (verse/blast/build/drive 1–4) with fills between · landing 2 |

Rules every shape obeys, taken from the hard-won lessons:

- Silence is a spice (lesson 2): at most two silences per form, one bar each,
  never first or last.
- Fills only directly before an intense section (chorus, blast, drive,
  breakdown), so the `into_<next>` fill preference has something to aim at.
- Bar counts favour 2, 4 and 8 (phrase-aligned, and 8 matches the 8-bar build
  cells); odd counts (3, 7) only in Maze and Stop/Go, where the records use
  them.
- Total length 12–40 bars, inside `valid_arrangement`'s 64-bar cap.
- **Meter turns honour the meter promise.** A turn is drawn only from meters
  the chosen style has cells in (fugazi can turn to 7/8 or 6/8; kidcrash, which
  owns only 4/4 cells, never turns). At most two turns per form, and only in
  Maze and Arc. This avoids leaning on the meter adapter for whole sections.

## Determinism: one form per (shape, form seed), in both engines

Same seed, same notes, forever (§3). For forms that should also mean: the
CLI and the plugin produce the **same form** for the same shape and seed, so a
form found in one can be reproduced in the other and pasted into `songs.txt`.

The engines cannot share an RNG (Mersenne Twister vs ChaCha8), so form choice
should not use either. Proposal: a tiny integer hash (SplitMix64) of
`(form_seed, slot_index, decision_index)` drives every choice. It is ten lines
in each language, consumes no engine RNG, and a cross-engine fixture
(like `golden_vector.json`: a list of `(shape, seed) → form string`) pins it.

## Where it lives

- **Python first (reference):** `arrangement_gen.py` with the templates as
  data and `generate_form(shape, form_seed, style) -> str`. CLI:
  `python drumgen.py --style fugazi --form arc --form-seed 42` prints the form
  and renders it. Tested: every shape × 10,000 seeds passes
  `valid_arrangement`, respects the rules above, and is deterministic.
- **Rust mirror:** `plugin/src/engine/arrangement_gen.rs`, the same templates
  exported through `builtin.json` (a `forms` key, written by
  `export_cells.py`) so the data has one source, plus the cross-engine fixture.
- **Plugin params:** a new **FORM** stepper (Off, Verse/Chorus, Arc, Stop/Go,
  Quiet/Loud, Maze) and a **FORM SEED** with its own dice. FORM other than Off
  overrides SONG. A separate param, rather than new SONG entries, because the
  SONG param is still an index: inserting entries would re-map saved projects.
  New params are appended, so saved projects keep their state.
- **Telegraph and horizon:** unchanged; they already read `Pattern.sections`.
  The status line names the form, e.g. `ARC 42`.
- **Bank:** a pad already stores the song by name (`SlotSnapshot.song_name`).
  A generated form is stored as the name `gen:<shape>:<seed>`, and
  `resolve_song` gains one branch: a `gen:` name is always resolvable, because
  it is regenerated, not looked up. No schema change, old banks unaffected.

## What it must not do

- No notes beyond the existing cells; no melody, harmony or "song" beyond a
  skeleton.
- No hidden re-rolls: the form changes only when the author changes FORM or
  rolls FORM SEED. The note DICE re-rolls notes inside the same form.
- No meter the style cannot play (see above).

## Open questions for the author

1. **Separate FORM SEED, or one seed for form and notes?** Proposed separate:
   structure and performance are different decisions, and you will want to
   keep a form while rolling its notes.
2. **Which shapes?** Five proposed. Too many? Missing one (a slowcore
   "long build, no payoff")?
3. **Length:** let the template decide (12–40 bars), or add a LENGTH control
   (short / medium / long)?
4. **Meter turns:** drawn from the style's meters automatically, or only when
   METER is not Auto, or never?
5. **Keep a form:** is copying the realised string to the clipboard / a
   `songs.txt` line worth a gesture, or is "write down shape and seed" enough?
6. **Fills and silences:** rolled per slot as above, or fixed per shape?

## Proposed order (evening-sized)

1. Python: `arrangement_gen.py`, templates, CLI flags, tests; render a batch of
   forms with `scripts/audition_cells.py`-style tooling so the shapes are judged
   by ear before any plugin work.
2. Rust: the port, templates via `builtin.json`, the cross-engine fixture.
3. Plugin: FORM and FORM SEED params, editor stepper and dice, status line,
   bank name encoding. Then the listen in Bitwig.
4. Only if question 5 says so: the keep-a-form gesture.
