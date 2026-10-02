# drumgen in Bitwig Studio (Linux), the golden path

drumgen is a note generator: it makes no sound of its own (the stereo output is
a silent dummy some hosts require). It emits MIDI drum notes while the transport
runs, and whatever drum instrument sits *after* it in the chain plays them.

## 1. Wire it up (one track, two devices)

Bitwig passes note events *downstream* through a track's device chain, exactly like
audio. So everything lives on one instrument track:

```
[Track: DRUMS]  →  drumgen  →  Ugritone Drums  →  (track audio out)
```

1. Create an Instrument Track.
2. Add drumgen, prefer the CLAP build (`~/.clap`); the VST3 works too.
3. Add Ugritone Drums (the yabridged VST3) *after* drumgen in the same chain.
4. Press Play. That's it, drumgen only emits notes while the transport is
   running.

No Note Receiver, no second track, no MIDI routing menus. If you hear nothing:

- Transport must be playing (stopped = silence by design).
- Track monitor must be on (the little speaker icon) if the track isn't record-armed.
- Don't debug "drumgen makes no sound": it never does; the sound is Ugritone's.

Turn knobs / flip styles / hit DICE while playing, the pattern regenerates and
swaps in immediately.

## 2. Capture a loop as an editable MIDI clip

Bitwig has no retrospective MIDI capture, and plugin-generated notes aren't
recorded just by playing — so record them the explicit way:

1. Keep drumgen on Track A (temporarily bypass/remove Ugritone there so you're
   only routing notes).
2. Create Track B (instrument track), and in its input chooser (Inspector or
   the small input selector on the track header) pick Track A as the note
   source.
3. Arm Track B, enable its monitor, press Record.
4. Let it run for the bars you want, stop. Track B now holds a normal note clip, open the piano roll, edit, quantize, whatever.
5. Drop Ugritone on Track B (or move the clip to your drum track and bypass
   drumgen there) so the clip drives the kit.

Alternatively, and this is the laziest, most reliable path: SAVE .MID in
the drumgen GUI writes the current pattern to `~/drumgen_output/`, and you can
drag that file straight from your file manager onto the Bitwig arranger (or
browse it via the Browser's Files tab). Drop it on the drumgen/Ugritone track
itself: the clip now drives Ugritone directly, so you can bypass/remove drumgen
there and the part is locked in as a normal editable note clip.

## 2b. Record the chain as AUDIO

1. Create an Audio track.
2. Open its input chooser (where a mic input would normally be picked) and
   select Ugritone Drums, any track's output can be another track's input
   in Bitwig.
3. Arm the audio track, press Record, let the loop play. Done.

Or skip live capture entirely: once a SAVE .MID clip is on the track (see
above), right-click the clip → Bounce. Bouncing works on real clips —
it does NOT work on live generator output, which is why recording the raw
drumgen track directly bounces to silence.

## 3. Suggested workflow for loop farming

1. One track: drumgen → Ugritone. Play.
2. Flip STYLE / hit DICE until something grabs you. HUMANIZE ~40% is the sweet
   spot; BARS 4; METER Auto follows the project's time signature (below).
3. SAVE .MID (or record to Track B) each keeper.
4. Drag keepers into your song, delete/bypass drumgen when arranging.

Same SEED + same settings = the exact same pattern again, forever. DICE jumps
the seed by a prime stride so every press lands on a different groove; dragging
the SEED readout scrubs it one at a time. Write down a seed if you love it.

**METER Auto** follows Bitwig's time signature live. It plays what the plugin
has cells for: /4 or /8 with up to 15 beats (2/2 plays as 4/4). Anything else
the host reports plays the style's native meter. If the style has no cell in
the meter you asked for, its closest cell is adapted to the bar: the pulse is
kept (a quarter stays a quarter) and the figure is repeated to fill, or cut at
the barline.

**FILL and loop length.** FILL "Every N" puts a fill on every Nth bar, so the
loop is stretched to hold the whole cycle: the least common multiple of BARS
and N. BARS 4 with Every 8 loops 8 bars (the figure twice, a fill closing bar
8); BARS 15 with Every 8 is 120 bars. The Python CLI keeps BARS as the exact
file length instead.

**SONG** (anything but Off) plays a whole arrangement and ignores BARS, METER
and FILL. Your own forms live in `~/.config/drumgen/songs.txt` (see the README);
restart the DAW after editing it.

## 3b. The pattern bank: switching grooves while it plays

Sixteen pads sit in the bank row under the knobs. A pad is a snapshot of the
sound: style, humanize, bars, seed, swing, meter, fill and song. Tempo is not
stored, a pad regenerates at whatever the transport says.

- **Store:** click STORE (it lights up), then click a pad. STORE copies what is
  playing: the pad that is playing if one is, otherwise the knobs. So you can
  also copy a pad to another slot while jamming.
- **Play:** click a pad, or send the track MIDI note 36 to 51 (C1 to D#2 in
  Bitwig's note names, where middle C is C3). The switch lands on the next
  barline of what is playing: the pad blinks while it waits, and the telegraph
  line reads `▸ SLOT n NEXT BAR`. A pad that is still generating waits, the press
  is not lost.
- **Clear:** right-click a pad.
- **Leaving the bank:** touching a knob or a stepper hands playback back to the
  params. A tempo change or a time-signature flip from the host does not: the
  playing pad stays. Pads re-bake to the new tempo, and pads set to METER Auto
  also re-bake to the new meter (a pad with a forced meter keeps it).
- **Saving:** pads are stored with the project and regenerate from their
  settings. A pad that stored a song remembers it by NAME, so reordering lines in
  `songs.txt` cannot turn it into a different form.
- **Pad colours:** dark is empty, faint is stored and still generating, solid is
  ready, lime with a border is playing, blinking is queued.

drumgen consumes the notes arriving on its track as pad triggers and does not
pass them on to the drum instrument after it. To play the kit by hand, do it on
a separate track.

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
load slowly the first time under Wine, raise the audio buffer if you get xruns
while it loads.

## 5. Troubleshooting

- Something odd mid-jam? The plugin writes one line per decision (transport,
  host meter flips, pad presses and why one was ignored, switches, bank exits,
  every generation, a failed build) to `~/drumgen_output/drumgen.log`. Read it
  with `tail -f` during a session, and attach it to a bug report.

- Reinstalled drumgen but nothing changed? Bitwig keeps the old `.so` in
  memory. Remove the drumgen device and re-add it (or restart Bitwig).
- Plugin missing from browser? Settings → Locations: make sure `~/.vst3` and
  `~/.clap` are listed, then rescan.
- Bridged plugin misbehaving? Check `~/.BitwigStudio/log/engine.log`: yabridge
  plugin output lands there.
- Rebuild + reinstall drumgen: `cd plugin && ./build-linux.sh`
