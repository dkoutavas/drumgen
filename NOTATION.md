# Handing drum parts to a human drummer

drumgen patterns become standard notation in two steps: a `.mid` you already
have, and one command. The score is real drum-staff notation — ghost notes in
parentheses, cymbals as x noteheads, accents marked, meter changes explicit.

## The pipeline

```
plugin SAVE .MID  (or  python drumgen.py ... --musicxml)
        │
        ▼
python notation.py ~/drumgen_output/zona_0003_4bars.mid
        │                       (writes zona_0003_4bars.musicxml next to it)
        ▼
MuseScore 4  →  File → Export → PDF     (or headless, see below)
```

That's it. The engine's entire vocabulary lives on a sixteenth grid, so the
converter snaps the humanized timing back to the grid **losslessly** — the
jitter is always well under half a sixteenth. No tuplet soup, nothing to
clean up by hand.

## MuseScore 4 (the recommended reader)

- Free, native Linux (AppImage), renders proper percussion staves.
- Open the `.musicxml`, done. Batch/headless PDF:
  `MuseScore4 --export-to out.pdf in.musicxml`
- **Do NOT import the .mid directly into MuseScore 4** — MS4 has no MIDI
  import panel (an MS3 feature not yet ported), and raw humanized MIDI
  becomes unreadable 32nd-note soup in any notation app. The `.musicxml` is
  the clean door.

## What the drummer sees

- Two voices, drummer-standard: hands stems-up, kick + pedal hat stems-down.
- Ghost snares in parentheses (the Daitro readability win), accents marked,
  x noteheads for cymbals, circle-x for open hats.
- Fresh time signature at every meter change (7/8 → 6/8 mid-song just works).
- Tempo marking from the file.

## About that Guitar Pro 5 license

Honest verdict: **GP5 is a file format now, not an application.** GP under
Wine is unsupported by Arobas and the community reports (sound engine breaks,
freezes) haven't improved in a decade. If your drummer specifically wants a
`.gp5` file: open the `.musicxml` in **TuxGuitar** (free, native Linux,
actively maintained) and export `.gp5` from there. Expect the notation to
render less beautifully than MuseScore — TuxGuitar is a tab editor, not an
engraver. For a chart a working drummer reads cold, send the MuseScore PDF.

## Zero-command mode: the on-save hook

Run once:

```bash
./scripts/install-notation-hook.sh
```

From then on, every **SAVE .MID** in the plugin also renders the `.musicxml`
score next to it automatically (the save message shows `▸ score`). The hook
is just `~/.config/drumgen/on_save` — an executable that receives the saved
path; edit or delete it freely.

## CLI one-shot

```bash
# Generate + notate in one go:
python drumgen.py --style zona --tempo 140 --bars 8 --seed 3 --musicxml

# Convert an existing SAVE .MID:
python notation.py ~/drumgen_output/skramz_arc_take.mid
```
