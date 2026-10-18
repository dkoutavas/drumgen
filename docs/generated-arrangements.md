# Generated arrangements — design note

Status: decided 2026-10-12; steps 1–3 are in (Python reference with CLI
`--form`/`--form-seed`; Rust engine port with a cross-engine fixture; plugin
FORM and FORM SEED params, editor controls, bank). Seen working in a headless
standalone build; not yet heard in Bitwig. PROJECT.md §8 queue item 3 ("the
reason the bank exists … needs a plan session before code"). The decisions
are at the end; the brief for them was "the most creative option that stays
easy to understand".

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

The 9 built-in forms reduce to five shapes, plus one the shipped forms do not
have (Slow Burn, decision 2). Each template is a list of slots; a slot names
the section types it may become, the bar counts it may take, and a chance.
`FORMS` in `arrangement_gen.py` is the source of truth; this table is the
gist:

| shape | from | slots (type: bars) |
|---|---|---|
| **Verse/Chorus** | Verse/Chor (Fugazi, ATDI) | intro 1–2 · verse 4–8 · chorus 2–4 · fill? 1 · verse 4–8 · chorus 4–8 · outro 1–4 |
| **Arc** | Skramz Arc, Post-Rock (Daitro, City of Caterpillar, Envy) | intro 1–4 · build 4–8 · fill? 1 · blast/drive 3–8 · landing (breakdown/atmospheric/outro) 1–4 |
| **Stop/Go** | Stop/Go, Eruption, Ampere (Orchid, pg.99, Ampere) | 3–5 × (burst: blast/chorus 1–3, then silence? 1) · landing breakdown 2–4 |
| **Quiet/Loud** | Quiet/Loud (Saetia) | atmospheric 2–4 · drive/chorus 2–4 · silence? 1 · atmospheric 2–4 · blast 3–6 · outro 2–4 |
| **Maze** | Labyrinth, Blast Fwd (Lord Snow) | intro 2 · 4–7 short sections (verse/blast/build/drive 2–4) with fills between · landing 2 |
| **Slow Burn** | slowcore, post-rock that withholds | intro/atmospheric 4–8 · verse? 4–8 · build 8 · drive? 4 · silence? 1 · comedown 4–8. No blast, no chorus: the peak never comes |

Rules every shape obeys, taken from the hard-won lessons:

- Silence is a spice (lesson 2): at most two silences per form (three in
  Stop/Go, where the gaps are the point, as in the shipped Stop/Go), one bar
  each, never first or last, never two in a row.
- No section follows itself: a slot never picks the type just placed when it
  offers another, and two of a kind left adjacent by a dropped fill or
  silence merge into one section.
- Fills only directly before an intense section (chorus, blast, drive,
  breakdown), so the `into_<next>` fill preference has something to aim at.
- Bar counts favour 2, 4 and 8 (phrase-aligned, and 8 matches the 8-bar build
  cells); odd counts (3, 7) only in Maze and Stop/Go, where the records use
  them.
- Total length 12–40 bars, inside `valid_arrangement`'s 64-bar cap.
- **Meter turns honour the meter promise.** A turn is drawn only from meters
  the chosen style's built-in cells are written in (fugazi can turn to 7/8 or
  6/8; kidcrash, which owns only 4/4 cells, never turns). Only verses and
  drives turn, as in the shipped forms: a turned blast or build would lose its
  section's cells to whatever the style owns in that meter. At most one turn
  in Arc, two in Maze, none elsewhere. Imported cells do not count, because the
  plugin never sees them and both engines must agree on the form.

## Determinism: one form per (shape, form seed), in both engines

Same seed, same notes, forever (§3). For forms that should also mean: the
CLI and the plugin produce the **same form** for the same shape and seed, so a
form found in one can be reproduced in the other and pasted into `songs.txt`.

The engines cannot share an RNG (Mersenne Twister vs ChaCha8), so form choice
should not use either. SplitMix64 (Vigna's reference stream, pinned by a
test) drives every choice: ten lines in each language, integer-only, no engine
RNG. Two streams from the one seed: stream 0 rolls the structure, stream 1 the
meter turns, so "ARC 42" has the same skeleton in every style and only the
turns depend on the style. Every slot draws its chance even when it is 1000,
and the tidy rules (fills, silences, merging, the 12–40 guard) use no dice, so
the draw order is the template's order and nothing else. A cross-engine
fixture (like `golden_vector.json`: `(shape, seed, style) → form string`)
pins the port.

## Where it lives

- **Python first (reference):** `arrangement_gen.py` with the templates as
  data and `generate_form(shape, form_seed, style) -> str`. CLI:
  `python drumgen.py --style fugazi --form arc --form-seed 42` prints the form
  and renders it. Tested (`TestGeneratedForms`): every shape × 2,000 seeds ×
  six styles passes a transcription of the plugin's `valid_arrangement`,
  respects the rules above, and is deterministic.
- **Rust mirror:** `plugin/src/engine/arrangement_gen.rs`, the same templates
  exported through `builtin.json` (a `forms` key, written by
  `export_cells.py`) so the data has one source, plus the cross-engine fixture.
- **Plugin params:** a new **FORM** stepper (Off, Verse/Chor, Arc, Stop/Go,
  Quiet/Loud, Maze, Slow Burn) and a **FORM SEED** with its own dice. FORM other than Off
  overrides SONG. A separate param, rather than new SONG entries, because the
  SONG param is still an index: inserting entries would re-map saved projects.
  New params are appended, so saved projects keep their state.
- **Telegraph and horizon:** unchanged; they already read `Pattern.sections`.
  The status line names the form, e.g. `ARC 42`.
- **Bank:** a pad stores the form as its own fields: `form_name` (the shape
  KEY), `form_seed`, and the index `form` (re-resolved from the key on
  restore; an unknown key is Off). As built this replaced the `gen:<shape>:<seed>`
  song-name encoding first proposed here: FORM and SONG are separate params,
  so one field holding either would have hidden which one a pad meant. All
  three are `serde(default)`, so banks saved before forms still load.

## What it must not do

- No notes beyond the existing cells; no melody, harmony or "song" beyond a
  skeleton.
- No hidden re-rolls: the form changes only when the author changes FORM or
  rolls FORM SEED. The note DICE re-rolls notes inside the same form.
- No meter the style cannot play (see above).

## Decisions (2026-10-12)

Brief from the author: the most creative option that stays easy to
understand.

1. **Separate FORM SEED, with its own dice.** Two dice, two questions: FORM
   DICE asks "what song?", the note DICE asks "how is it played?". A form you
   like survives any number of note rolls. Form seeds run 0–9999 so a form has
   a name you can remember and say: `ARC 42`.
2. **Six shapes:** the five above plus **Slow Burn**, the slowcore and
   withholding post-rock form: a long rise, a held breath where the climax
   should land, and a comedown instead of the payoff. Labels fit the stepper
   (10 characters): Verse/Chor, Arc, Stop/Go, Quiet/Loud, Maze, Slow Burn.
3. **No LENGTH control.** The shape decides (12–40 bars) and the length
   varies with the seed; the status line shows the total. One knob fewer, and
   a short Arc and a long Arc are both Arcs.
4. **METER Auto lets the form wander; a forced METER holds it.** On Auto, Arc
   and Maze may turn verses and drives into the style's other meters. Any
   forced meter is the whole form's meter, no turns. In the CLI, leaving out
   `--time-sig` is Auto.
5. **No keep gesture: the name is the form.** Shape plus seed regenerates it
   exactly, in both engines, forever. The CLI prints a ready `songs.txt` line
   (`ARC 42 | 2:intro 8:build …`) for when you want to edit a form by hand,
   the plugin's session log records the string with every generation, and a
   bank pad stores the shape and seed.
6. **Fills and silences rolled per slot**, inside the rules above, so two
   seeds of one shape can breathe differently. Stop/Go is the one shape whose
   silences are allowed a third gap.

## Proposed order (evening-sized)

1. Python: `arrangement_gen.py`, templates, CLI flags, tests; render a batch of
   forms with `scripts/audition_forms.py` so the shapes are judged by ear
   before any plugin work. **Done 2026-10-12, unheard.**
2. Rust: the port, templates via `builtin.json`, the cross-engine fixture.
   **Done 2026-10-12:** 216 fixture forms match exactly.
3. Plugin: FORM and FORM SEED params, editor stepper and dice, status line,
   bank fields. **Done 2026-10-12**, seen in a headless standalone build.
   Then the listen in Bitwig.
4. ~~The keep-a-form gesture~~: not needed (decision 5).
