# drumgen in Bitwig Studio (Linux) — the golden path

drumgen is a note **generator**: it makes no sound of its own (the stereo output is
a silent dummy some hosts require). It emits MIDI drum notes while the transport
runs, and whatever drum instrument sits *after* it in the chain plays them.

## 1. Wire it up (one track, two devices)

Bitwig passes note events *downstream* through a track's device chain, exactly like
audio. So everything lives on **one instrument track**:

```
[Track: DRUMS]  →  drumgen  →  Ugritone Drums  →  (track audio out)
```

1. Create an Instrument Track.
2. Add **drumgen** — prefer the **CLAP** build (`~/.clap`); the VST3 works too.
3. Add **Ugritone Drums** (the yabridged VST3) *after* drumgen in the same chain.
4. Press **Play**. That's it — drumgen only emits notes while the transport is
   running.

No Note Receiver, no second track, no MIDI routing menus. If you hear nothing:

- Transport must be **playing** (stopped = silence by design).
- Track monitor must be on (the little speaker icon) if the track isn't record-armed.
- Don't debug "drumgen makes no sound" — it never does; the sound is Ugritone's.

Turn knobs / flip styles / hit DICE while playing — the pattern regenerates and
swaps in immediately.

## 2. Capture a loop as an editable MIDI clip

Bitwig has no retrospective MIDI capture, and plugin-generated notes aren't
recorded just by playing — so record them the explicit way:

1. Keep drumgen on Track A (temporarily bypass/remove Ugritone there so you're
   only routing notes).
2. Create Track B (instrument track), and in its **input chooser** (Inspector or
   the small input selector on the track header) pick **Track A** as the note
   source.
3. **Arm** Track B, enable its monitor, press **Record**.
4. Let it run for the bars you want, stop. Track B now holds a normal note clip —
   open the piano roll, edit, quantize, whatever.
5. Drop Ugritone on Track B (or move the clip to your drum track and bypass
   drumgen there) so the clip drives the kit.

Alternatively: **SAVE .MID** in the drumgen GUI writes the current pattern to
`~/drumgen_output/`, and you can drag that file straight from your file manager
onto the Bitwig arranger (or browse it via the Browser's Files tab). Dropping it
on an existing track imports the notes into that track.

## 3. Suggested workflow for loop farming

1. One track: drumgen → Ugritone. Play.
2. Flip STYLE / hit DICE until something grabs you. HUMANIZE ~40% is the sweet
   spot; BARS 4; METER Auto uses the style's native meter.
3. SAVE .MID (or record to Track B) each keeper.
4. Drag keepers into your song, delete/bypass drumgen when arranging.

Same SEED + same settings = the exact same pattern again, forever. DICE is just
SEED+1 — write down a seed if you love it.

## 4. Ugritone via yabridge (already set up on this machine)

`~/.vst3/Ugritone Drums.vst3` is a native yabridge bridge to the Windows VST3 in
`~/.wine/drive_c/Program Files/Common Files/VST3/`. If you ever reinstall or add
Ugritone kits:

```bash
yabridgectl sync        # re-create bridges after (re)installing Windows plugins
```

(`yabridgectl` isn't on PATH on this machine — it lives wherever yabridge was
unpacked, typically `~/.local/share/yabridge/yabridgectl`. Find it with
`find ~ -name yabridgectl -type f 2>/dev/null` if the plain command errors.)

Then rescan plugins in Bitwig (Settings → Locations → rescan). Sample-heavy kits
load slowly the first time under Wine — raise the audio buffer if you get xruns
while it loads.

## 5. Troubleshooting

- **Reinstalled drumgen but nothing changed?** Bitwig keeps the old `.so` in
  memory. Remove the drumgen device and re-add it (or restart Bitwig).
- **Plugin missing from browser?** Settings → Locations: make sure `~/.vst3` and
  `~/.clap` are listed, then rescan.
- **Bridged plugin misbehaving?** Check `~/.BitwigStudio/log/engine.log` — yabridge
  plugin output lands there.
- **Rebuild + reinstall drumgen:** `cd plugin && ./build-linux.sh`
