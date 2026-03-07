# DRUMGEN Style DNA Reference Library

## How To Use This File

Place this file in your `drumgen/styles/` directory. When asking Claude Code to generate a pattern, reference it:

"Read the style DNA file in styles/ and generate an 8-bar pattern using the LITURGY_BURST_BEAT vocabulary at 180bpm in 4/4"

"Check the FARAQUET_ANGULAR style DNA. Generate a 7/8 verse grouped 2+2+3, 8 bars at 140bpm. Use the DISPLACED_BACKBEAT and SYNCOPATED_RIDE primitives."

"Combine SHELLAC_FLOOR_TOM_DRIVE for the verse with DAITRO_BLAST_BUILD for the chorus transition. 4 bars each."

Claude Code should read the specific style section, internalize the EXACT rhythmic cells and rules described, and use them as building blocks when constructing the pattern JSON. Do NOT improvise away from these definitions — they are the style. Variations should be recombinations and developments of these cells, not departures from them.

---

## Table of Contents

1. BLAST BEAT VOCABULARY (Traditional, Liturgy Burst, Skank, Bomb)
2. D-BEAT VOCABULARY
3. SHELLAC / PRECISION NOISE ROCK
4. FARAQUET / ANGULAR MATH POST-HARDCORE
5. FUGAZI / DC POST-HARDCORE
6. POLVO / ART-DAMAGE NOISE ROCK
7. SCREAMO / EMOVIOLENCE (Jeromes Dream, Orchid, Pg.99)
8. EUROPEAN SCREAMO (Daitro, Raein, La Quiete)
9. EXPERIMENTAL BLACK METAL (Krallice, Yellow Eyes, Deafheaven)
10. FILL VOCABULARY
11. TRANSITION VOCABULARY
12. SECTION DYNAMICS MAP

---

## 1. BLAST BEAT VOCABULARY

### 1A. TRADITIONAL BLAST BEAT

The foundational blast. Kick and snare alternate on every sixteenth note. Cymbal rides on eighths or sixteenths.

**Core cell (one beat in 4/4, sixteenth note grid):**
```
Position:  1e+a
Kick:      X . X .
Snare:     . X . X
Ride:      X . X .    (option A: with kick — "European" style)
-- OR --
Ride:      X X X X    (option B: every sixteenth — "full" style)
```

**Velocity rules:**
- Kick: ALL accent (110-127). No dynamics. Blast kicks are committed.
- Snare: Accent (110-120). Slight random variance ±5, never below 105.
- Ride/cymbal: Normal to accent (90-120). If on every 16th, alternate normal/accent to avoid machine gun.

**Timing rules:**
- Kick and snare should NOT be perfectly aligned to grid. Snare flamms 3-8ms AFTER the grid position (real drummers' left hand is slightly late in blasts).
- Progressive rush: over 4+ bars of blasting, the entire pattern should drift 1-3ms ahead per bar. Blast beats naturally accelerate.
- Cymbal can sit slightly ahead of kick (1-3ms) — the cymbal hand leads.

**Duration:** Usually 2-8 bars. Rarely longer without a fill or cymbal break.

**Common variations:**
- **Half-blast**: Kick on every sixteenth, snare on every eighth (instead of every sixteenth). Less intense, more sustainable.
- **Gravity blast / one-handed roll**: Snare on every 32nd note bursts (2-4 beats max). Extremely fast, used as accent moments, not sustained.

---

### 1B. LITURGY BURST BEAT (Greg Fox style)

NOT a standard blast. The burst beat is a specific Greg Fox innovation. The key distinction: kick and snare hit at ALMOST the same time with a tiny flam offset, creating a dense "burst" texture rather than clean alternation.

**Core cell (one beat in 4/4):**
```
Position:     1     e     +     a
Kick:         X     X     X     X     (every sixteenth)
Snare:        .x    .x    .x    .x    (every sixteenth, flammed ~1 sixteenth AFTER kick)
```

Wait — more precisely, the burst beat places kick and snare nearly simultaneously on subdivisions, but with micro-offset:

**Actual burst beat cell (showing timing in sub-sixteenth resolution):**
```
Grid:    |1      e      +      a     |
Kick:    X      X      X      X
Snare:     x      x      x      x    (5-15ms after each kick)
HiHat:   O             O             (open, on 1 and +, quarter note pulse)
```

**The critical characteristics:**
1. Kick and snare are near-simultaneous, NOT alternating. This is what makes it a burst beat, not a blast beat.
2. The flam offset between kick and snare is 5-15ms (not a full sixteenth, just a micro-timing offset). Use `timing_offset_ms: 8` to `12` on the snare hits.
3. Hi-hat or ride plays a SLOWER pulse over top — usually quarter notes or half notes. This creates polyrhythmic tension (fast burst underneath, slower pulse on top).
4. Greg Fox accents in groupings that DON'T align with the bar. Common: accenting every 3rd sixteenth while playing in 4/4, creating a 3-over-4 polyrhythm. This is essential to the Liturgy feel.

**Accent pattern (the polyrhythmic layer):**
```
16th positions:  1  2  3  4  5  6  7  8  9 10 11 12 13 14 15 16
Kick:            X  X  X  X  X  X  X  X  X  X  X  X  X  X  X  X
Snare (flam):    x  x  x  x  x  x  x  x  x  x  x  x  x  x  x  x
ACCENT on snare: A        A        A        A        A        A
                 ^--3--^--3--^--3--^--3--^--3--^--3--
```
Accented snare hits (velocity 120-127) every 3rd sixteenth. Non-accented snares at velocity 80-95. This 3-over-4 accent cycle is the Liturgy DNA.

**Velocity rules:**
- Kick: All strong (100-120). Less variance than traditional blast.
- Snare (non-accent): 80-95. Background texture.
- Snare (accent, every 3rd): 120-127. These are the rhythmic landmarks.
- Hi-hat: 90-110. Steady.

**Timing rules:**
- Snare flamms after kick: 5-15ms, use `timing_offset_ms` of 8-12 on all snare hits.
- Hi-hat slightly ahead of grid (leads the pulse): timing_offset_ms of -3 to -5.
- The polyrhythmic accents should be metronomically precise — Greg Fox's accent pattern is deliberate, not sloppy.

**Common contexts:** Tremolo guitar sections. Build sections. Usually enters after a quieter passage for maximum impact. Tempo range: 160-200 BPM.

---

### 1C. SKANK BEAT

Punk/hardcore staple. NOT a blast beat. Faster than a regular beat, simpler than a blast.

**Core cell (one beat in 4/4):**
```
Position:  1  +
Kick:      X  .
Snare:     .  X
HiHat:     X  X   (eighth notes throughout)
```

That's it. Kick on downbeats, snare on upbeats (every eighth note), hi-hat on every eighth. The simplicity IS the point.

**Velocity rules:**
- Everything at accent level (105-120). Skank beats are played hard.
- Slight emphasis on beat 1 kick and beat 3 snare (the "main" beats).
- Hi-hat: mechanical consistency, minor variance ±5.

**Timing rules:**
- Extremely tight to grid. Skank beats should feel metronomic.
- Minimal humanization — maybe 0.3 on the humanize scale. The punk precision is intentional.

**Variations:**
- **Cymbal swap**: Ride instead of hi-hat for more intensity.
- **Open hat accent**: Open hi-hat on beat 4's "and" leading into the next bar's downbeat. Classic punk tension-release.

---

### 1D. BOMB BLAST

Used in grindcore and powerviolence. Kick, snare, AND cymbal ALL hit simultaneously on every note.

**Core cell:**
```
Position:  every sixteenth note
Kick:      X X X X X X X X X X X X X X X X
Snare:     X X X X X X X X X X X X X X X X
Crash:     X X X X X X X X X X X X X X X X
```

**No offsets, no flam.** Everything lands together. This is deliberately ugly and overwhelming. Wall of sound.

**Velocity:** Everything 115-127. Maximum aggression.
**Timing:** Can be slightly sloppy (humanize 0.8+). This isn't precise, it's violent.
**Duration:** Usually 1-4 bars maximum. It's unsustainable and that's the point.

---

## 2. D-BEAT VOCABULARY

### 2A. STANDARD D-BEAT

Named after Discharge. The defining rhythm of crust punk and a building block for many post-hardcore patterns.

**Core cell (2 beats in 4/4, eighth note grid):**
```
Position:  1  +  2  +
Kick:      X  .  X  X
Snare:     .  X  .  .
HiHat:     X  X  X  X
```

Expanded to full bar:
```
Position:  1  +  2  +  3  +  4  +
Kick:      X  .  X  X  X  .  X  X
Snare:     .  X  .  .  .  X  .  .
HiHat:     X  X  X  X  X  X  X  X
```

**The signature:** Kick pattern is X.XX — that syncopated double kick before the snare is the d-beat DNA. Snare on 2 and 4 (backbeat).

**Velocity rules:**
- Kick: accent on beat 1 and 3 (110-120), normal on the doubles (95-105).
- Snare: all accent (110-125). Strong backbeat.
- Hi-hat: consistent (90-100), slight accent on downbeats.

**Timing rules:**
- The double kick (beats "2+" and "3" or "4+" and "1") should be tight but can flamm very slightly (2-4ms between the two kicks). This gives it human energy.
- Snare lands slightly behind for groove: timing_offset_ms 2-4.

### 2B. DOUBLE-TIME D-BEAT

Same pattern at double speed — now on sixteenths:
```
Position:  1 e + a 2 e + a 3 e + a 4 e + a
Kick:      X . X X X . X X X . X X X . X X
Snare:     . X . . . X . . . X . . . X . .
HiHat:     X X X X X X X X X X X X X X X X
```

This is where d-beat meets blast beat territory.

---

## 3. SHELLAC / PRECISION NOISE ROCK (Todd Trainer)

### Philosophy
Todd Trainer's drumming is about SPACE and COMMITMENT. He plays simple patterns with absolute conviction. Every note is deliberate. There are NO ghost notes. If he hits something, he hits it hard. The power comes from what he DOESN'T play.

### 3A. SHELLAC FLOOR TOM DRIVE

The signature Shellac pattern. Floor tom replaces or supplements kick drum. Sparse. Powerful.

**Core cell (one bar of 4/4):**
```
Position:     1     +     2     +     3     +     4     +
Kick:         X                             X
Floor Tom:                X                             X
Snare:                          X                 X
HiHat:        X     X     X     X     X     X     X     X
```

But that's too busy for Shellac. More typically:
```
Position:     1     +     2     +     3     +     4     +
Floor Tom:    X                       X
Snare:                    X                       X
Ride:         X           X           X           X
```

**That's it.** Floor tom on 1 and 3. Snare on 2 and 4. Ride on quarters. No fills. No ghost notes. No variation for 16 bars straight.

**Velocity rules:**
- Floor tom: ACCENT (115-127). He hits the floor tom like it owes him money.
- Snare: ACCENT (115-125). Equally committed.
- Ride: Normal-accent (95-115). Steady.
- **NO ghost notes. Ever.** Todd Trainer does not do ghost notes.
- **NO velocity swells or dynamics within a pattern.** Consistency IS the style.

**Timing rules:**
- Near-metronomic. Humanize at 0.2-0.3 maximum. The machine-like precision is intentional.
- Ride can sit 1-2ms ahead (barely perceptible leading feel).
- Zero swing.

### 3B. SHELLAC KICK-SNARE LOCK

When Shellac uses kick drum (instead of floor tom):
```
Position:     1     +     2     +     3     +     4     +
Kick:         X                       X
Snare:                    X                       X
Ride:         X     X     X     X     X     X     X     X
```

Basic rock beat. The Shellac-ness comes from the velocity (everything HARD), the precision (metronomic), and the stubborn repetition (no fills for entire sections).

### 3C. SHELLAC ODD-METER LOCK

Shellac frequently uses odd meters but makes them feel natural through repetition:
```
7/8 (grouped 3+4):
Position:     1     2     3     1     2     3     4
Floor Tom:    X                 X
Snare:                    X                       X
Ride:         X     X     X     X     X     X     X
```

Same philosophy — simple pattern, extreme commitment, zero decoration.

---

## 4. FARAQUET / ANGULAR MATH POST-HARDCORE

### Philosophy
Chad Molter makes complex meters groove. The drumming is busy but every hit serves the song. Ghost notes are everywhere. The ride hand is constantly active. Kick drum has a melodic, syncopated relationship with the bass guitar. Odd meters don't sound "math" — they sound like the only natural way the song could feel.

### 4A. DISPLACED BACKBEAT

The snare doesn't land on 2 and 4. It's shifted by an eighth note or sixteenth:
```
4/4 standard:         Snare on 2 and 4
Faraquet displaced:   Snare on 2+ and 4 (or 2 and 3+)
```

**Example (4/4, eighth note grid):**
```
Position:  1  +  2  +  3  +  4  +
Kick:      X        X     X
Snare:           x     X        X     (lowercase x = ghost, uppercase X = accent)
Ghost Sn:     x              x        (ghost notes filling gaps)
Ride:      X  X  X  X  X  X  X  X
```

The accented snare on the "and" of 2 is the displacement. It creates a lurching, angular feel. Ghost snares on either side of the accent smooth it out so it grooves rather than stumbles.

### 4B. SYNCOPATED KICK PATTERN

Faraquet's kick drum doesn't just keep time — it interacts with the bass guitar as a rhythmic counterpoint.

**Example (7/8, grouped 2+2+3, eighth note grid):**
```
Position:  1  2  |  3  4  |  5  6  7
Kick:      X     |     X  |  X     X
Snare:         X |  X     |     X
Ghost Sn:   x   |       x|        x
Ride:      X  X |  X  X  |  X  X  X
```

The kick on positions 1, 4, 5, 7 creates a pattern that interlocks with (not doubles) the bass guitar. The space between kick hits is where the bass breathes.

### 4C. RIDE-HAT INTERCHANGE

Within a single section, the right hand moves between ride and hi-hat to create textural shifts:
```
Bars 1-2: Ride (busier feel, more overtone)
Bars 3-4: Closed hi-hat (tighter, more controlled)
Bar 5: Open hi-hat on beat 1, then ride (tension release)
```

**Velocity rules:**
- Ride: Normal (85-100) with accent on downbeat of each group (110-115). Dynamic right hand.
- Snare accent: 105-120.
- Snare ghost: 25-45. Very quiet. These are felt, not heard distinctly.
- Kick: Normal to accent (90-115). Not all kicks are equal — syncopated kicks are slightly softer (90-100) than downbeat kicks (105-115).
- Hi-hat: Normal (80-95) with slight accent on group downbeats.

**Timing rules:**
- Ghost notes: timing_offset_ms 3-6 (slightly behind).
- Syncopated kicks: timing_offset_ms 0-2 (pretty tight, these need to lock with the bass).
- Ride: timing_offset_ms -2 to 0 (slightly ahead or on grid, the ride leads).
- Humanize: 0.6-0.7. Enough to breathe, not enough to sound sloppy.

### 4D. FARAQUET 7/8 TEMPLATE

Full 2-bar cell in 7/8 (grouped 2+2+3):
```
BAR 1:
Beat:      1     2     3     4     5     6     7
Ride:      X     X     X     X     X     X     X
Kick:      X                 X           X
Snare:                 X                       X
Ghost:           x                 x
HH Ped:                                  x

BAR 2 (variation):
Beat:      1     2     3     4     5     6     7
Ride:      X     X     X     X     X     X     X
Kick:            X           X     X
Snare:     X                             X
Ghost:                 x                       x
HH Ped:                            x
```

Bar 2 displaces the pattern — kick and snare shift position, creating forward motion across the 2-bar phrase. This "cell rotation" is key to Faraquet's approach.

---

## 5. FUGAZI / DC POST-HARDCORE (Brendan Canty)

### Philosophy
Musical restraint with explosive release. The drums SERVE the dynamics of the song. Quiet sections are genuinely quiet. Loud sections are genuinely loud. Floor tom is an expressive voice, not just a fill instrument. Canty plays behind the beat for groove and ahead of the beat for urgency.

### 5A. FUGAZI QUIET VERSE

Minimal. Often just kick and snare with cross-stick or rim click:
```
Position:  1     +     2     +     3     +     4     +
Kick:      X                             X
Rim:                         X                       X
HH Ped:    x           x           x           x
```

**Velocity:** Everything soft (55-75). The hi-hat pedal is barely audible — it's felt more than heard.
**Timing:** Relaxed, behind the beat. timing_offset_ms 3-6 on everything. This is NOT tight.

### 5B. FUGAZI DRIVING CHORUS

When Fugazi explodes, the drums are driving but never overly busy:
```
Position:  1     +     2     +     3     +     4     +
Kick:      X                 X     X
Snare:                 X                       X
Ride:      X     X     X     X     X     X     X     X
Crash:     X                                         (only on first bar of section)
```

**Velocity:** Kick and snare at accent (110-120). Ride at normal (90-100). The crash on beat 1 of the section is 127. Full commitment.
**Timing:** Pushing forward slightly. timing_offset_ms -2 to -4 on kick and snare. The urgency comes from leaning into the beat.

### 5C. FUGAZI FLOOR TOM ACCENT

Canty uses floor tom as a melodic/emotional accent — not as part of a fill but as a standalone voice replacing the kick or snare:
```
Position:  1     +     2     +     3     +     4     +
Floor Tom: X                             X
Snare:                 X                       X
Ride:      X     X     X     X     X     X     X     X
```

Floor tom on 1 and 3 instead of kick. Lower, warmer, more musical than kick drum. Common in Fugazi's more emotional passages.

**Velocity:** Floor tom at 100-115 (strong but not violent — this isn't Shellac).

---

## 6. POLVO / ART-DAMAGE NOISE ROCK

### Philosophy
Unpredictable. The drums follow the guitar's angular movements rather than keeping steady time. Feels like the pattern is constantly about to fall apart but never does. Time signatures shift mid-phrase. The drummer is REACTING to the guitars, not laying down a foundation.

### 6A. POLVO STUMBLE BEAT

A pattern that deliberately trips over itself:
```
Bar 1 (4/4):
Position:  1     +     2     +     3     +     4     +
Kick:      X                 X           X
Snare:            X                X                 X
HiHat:     X     X     X     X     X     X     X     X

Bar 2 (effectively 7/8 crammed into 4/4 space — one beat dropped):
Position:  1     +     2     +     3     +     4
Kick:      X                 X           X
Snare:            X                X
HiHat:     X     X     X     X     X     X     X
```

The 4/4 bar followed by a bar that feels like it has a beat missing creates the signature Polvo "did they just mess up?" feeling. They didn't. It's deliberate.

### 6B. POLVO DYNAMIC SHIFT

Sudden, unannounced dynamic drops or spikes:
```
Bars 1-3: Full volume (accent everything)
Bar 4, beat 3: EVERYTHING drops to ghost/soft velocity
Bar 5: Stays quiet
Bar 6, beat 1: Back to full volume with crash
```

**No fill, no transition, no warning.** The dynamic shift IS the transition. Encode this as sudden velocity changes in the JSON.

### 6C. POLVO TEMPO DRIFT

Over 4-8 bars, the pattern gradually speeds up or slows down. Not through tempo changes in the MIDI file, but through progressive timing offsets:
```
Bar 1: timing_offset_ms 0 (on grid)
Bar 2: timing_offset_ms -2 (slightly ahead)
Bar 3: timing_offset_ms -4
Bar 4: timing_offset_ms -6 (noticeably rushing)
Bar 5: timing_offset_ms 0 (snaps back to grid — feels like a deep breath)
```

---

## 7. SCREAMO / EMOVIOLENCE (Jeromes Dream, Orchid, Pg.99)

### Philosophy
Extremes. Blast beats next to total silence. No middle ground. The dynamics ARE the composition. Sloppiness is authentic — this isn't about precision, it's about emotional catharsis. If it sounds too clean, it's wrong.

### 7A. EMOVIOLENCE BLAST SECTION

Uses the traditional blast (see 1A) BUT with specific emoviolence characteristics:
- Crashes on every bar's beat 1. Sometimes every 2 beats.
- Ride hand plays crash cymbal instead of ride/hi-hat during blast sections. This creates a washy, chaotic sound.
- Blast duration: usually 2-4 bars before a break.

```
Position:  1 e + a 2 e + a 3 e + a 4 e + a
Kick:      X . X . X . X . X . X . X . X .
Snare:     . X . X . X . X . X . X . X . X
Crash:     X . . . . . . . X . . . . . . .   (every 2 beats)
```

**Velocity:** Sloppy. Range 95-127 with HIGH variance. Humanize at 0.9+.
**Timing:** Sloppy. Humanize at 0.9+. Timing jitter ±8ms. These blasts should sound barely held together.

### 7B. EMOVIOLENCE ANGULAR BREAKDOWN

After a blast section, sudden shift to a slow, angular breakdown:
```
Tempo: usually HALF the blast tempo or slower
Position:  1           +           2           +           3           +           4           +
Kick:      X                                               X           X
Snare:                             X
Floor Tom:                                                                         X
Crash:     X
```

**Huge space between hits.** Let each hit RING. Velocity: everything 115-127. Every hit is a statement.

### 7C. EMOVIOLENCE CHAOTIC FILL

Fills in this style are NOT organized tom descents. They're flailing:
```
Duration: usually 1-2 beats (very fast)
Instruments: Random combination of toms, snare, kick
Pattern: No clear direction — NOT high tom → mid tom → floor tom
Instead: snare, floor tom, high tom, kick, snare, crash
```

Generate fills by semi-randomly selecting from snare/toms/kick on sixteenth notes for 1-2 beats, then crash on the downbeat after. Velocity 100-127, high variance.

---

## 8. EUROPEAN SCREAMO (Daitro, Raein, La Quiete)

### Philosophy
More compositional than US emoviolence. Dynamics are carefully built, not just smashed together. Post-rock influence in the build sections. When it blasts, it EARNS the blast through patient tension building. The emotional arc is everything.

### 8A. DAITRO QUIET BUILD

Starts sparse and gradually adds layers over 4-8 bars:
```
Bar 1-2: Ride bell only, quarter notes, soft (60-75 velocity)
Bar 3-4: Add kick on 1, ride moves to bow (louder, 80-90)
Bar 5-6: Add snare on 3 (soft, 70-80), kick adds beat 3, ride goes to eighths
Bar 7: Add ghost snares, ride at normal velocity, crash on beat 1
Bar 8: Full pattern — driving eighth note ride, kick/snare backbeat, leading into...
```

**The build is LINEAR and PATIENT.** Don't rush it. Each 2-bar block adds ONE element.

### 8B. DAITRO TREMOLO DRIVE

Under tremolo-picked guitar sections, a driving pattern that's NOT a blast:
```
Position:  1     +     2     +     3     +     4     +
Kick:      X     X           X     X           X
Snare:                 X                 X           X
Ride:      X     X     X     X     X     X     X     X
```

**Fast kick doubles but NOT blast speed.** Eighth note ride, not sixteenth. The pattern has momentum without going full blast. This is the Daitro sweet spot — intense but not unhinged.

**Velocity:**
- Ride: Normal (85-100) with accent on beat 1 (110).
- Kick: Normal (90-105). The doubles are slightly softer than downbeat kicks.
- Snare: Accent (110-120).

### 8C. DAITRO BLAST RELEASE

When the blast finally arrives (after a proper build), it should feel like a dam breaking:
```
Bar 1 of blast: Traditional blast (see 1A) with crash on EVERY beat 1
Bar 2-4: Blast continues, crashes thin out to every 2 bars
Bar 5-6: Blast with ride instead of crash (less washy, more controlled)
Bar 7-8: Blast decays — snare drops to half-blast (every eighth instead of sixteenth)
```

The blast has its own dynamic arc. It doesn't just start and stop — it swells and recedes.

### 8D. RAEIN MELODIC DRIVE

More groove-oriented than Daitro. Feels almost like indie rock drumming at points:
```
Position:  1     +     2     +     3     +     4     +
Kick:      X                       X
Snare:                 X                       X
HiHat:     X     x     X     x     X     x     X     x    (alternating accent/ghost)
Ghost Sn:        x                       x
```

Simple backbeat with dynamic hi-hat (accented on eighths, ghost on upbeats) and ghost snares. The hi-hat dynamics are what make this feel "European screamo" rather than generic indie rock.

---

## 9. EXPERIMENTAL BLACK METAL (Krallice, Yellow Eyes, Deafheaven)

### 9A. KRALLICE BLAST WITH MELODIC KICK

Krallice's drumming (Lev Weinstein) adds melodic kick patterns UNDER a blast framework:
```
Position:  1 e + a 2 e + a 3 e + a 4 e + a
Snare:     . X . X . X . X . X . X . X . X   (constant sixteenths)
Kick:      X . X . . . X . X . . X . . X .   (NOT constant — syncopated)
Ride:      X . X . X . X . X . X . X . X .   (with kick or eighths)
```

**The key:** Snare blasts on every sixteenth, but the KICK is playing a melodic rhythm underneath. The kick pattern changes every 2-4 bars, creating phrases within the blast. This is NOT a wall-of-sound blast — it has compositional movement.

**Velocity rules:**
- Snare: Two layers. Accented hits (110-120) on a slower rhythmic cycle (every 3rd or 4th hit), background hits (85-95).
- Kick: Normal (95-110). The syncopated pattern should be clear but not dominating the snare blast.
- Ride: Normal (90-100).

### 9B. ATMOSPHERIC POST-SECTION

Between blast sections, Krallice/Yellow Eyes/Deafheaven have spacious atmospheric passages:
```
Position:  1     +     2     +     3     +     4     +
Kick:      X
Ride Bell:       X                       X
Snare:                             X
HH Pedal:  x           x           x           x
```

Extremely sparse. Ride bell (not bow) for a pinging, atmospheric texture. Hi-hat pedal barely audible.

**Velocity:** Everything soft to normal (55-90). This is about SPACE and TEXTURE.
**Timing:** Relaxed. timing_offset_ms 3-8 on everything. Floating feel.

### 9C. DEAFHEAVEN BUILD-TO-BLAST

Deafheaven transitions from post-rock to blast beat via a specific build pattern:
```
Phase 1 (bars 1-4): Quarter note kick, ride on eighths, no snare
Phase 2 (bars 5-6): Eighth note kick, ride on eighths, snare on 2 and 4
Phase 3 (bars 7-8): Sixteenth note kick, ride on sixteenths, snare on 2 and 4 (double time feel)
Phase 4 (bar 9+): Full traditional blast with crash
```

Each phase doubles the kick density. The transition feels inevitable and climactic.

---

## 10. FILL VOCABULARY

### 10A. LINEAR FILL (Post-hardcore standard)
No two limbs hit simultaneously. Single-stroke rolls around the kit:
```
Duration: 1 bar
16ths:  1 e + a 2 e + a 3 e + a 4 e + a
Snare:  X     X         X     X
HiTom:    X         X
MidTom:       X               X
FlrTom:           X                 X
Kick:                     X           X
Crash:                                  X  (on next bar's beat 1)
```

**Velocity:** Crescendo from 80 to 120 across the fill. Each successive hit slightly louder.
**Timing:** Slight rush (timing_offset_ms decreasing from 0 to -5 across the fill).

### 10B. SNARE ROLL FILL (Screamo/blast entry)
Snare doubles leading into blast:
```
Duration: 2 beats
16ths:  3 e + a 4 e + a | 1
Snare:  X X X X X X X X | (blast begins)
Kick:         X       X | X
Crash:                   | X
```

**Velocity:** Crescendo 85 → 127. The last 4 hits should all be 120+.

### 10C. FLOOR TOM BREAKDOWN FILL (Noise rock)
Single floor tom hits, spaced out:
```
Duration: 1 bar
Position:  1           2     +     3                 4
Floor Tom: X                 X                       X
Kick:                  X                 X
```

**Velocity:** All accent (115-127). Each hit rings out. Let the space do the work.

### 10D. CHAOTIC FILL (Emoviolence)
Semi-random distribution across kit:
```
Duration: 2 beats (very fast)
16ths:   3 e + a 4 e + a
Hit:     S F K H S K F S    (S=snare, F=floor, K=kick, H=high tom)
```
Generate by randomly selecting from [snare, tom_high, tom_floor, kick] for each sixteenth, avoiding more than 2 consecutive hits on the same instrument.

**Velocity:** High with high variance (100-127, ±12). Messy.
**Timing:** Rushed. timing_offset_ms -3 to -8, progressively more ahead.

---

## 11. TRANSITION VOCABULARY

### 11A. CRASH + SILENCE
End section with crash on beat 1, then complete silence for 1-2 beats before next section begins.

### 11B. CYMBAL SWELL
Ride bell or crash played as repeated eighth notes with crescendo velocity over 1-2 bars, ending on a crash + kick on the new section's beat 1.

### 11C. SNARE PRESS ROLL
Snare buzz roll (use snare with velocity 40-60, many hits close together — every 32nd note for 1-2 beats) swelling from ghost to accent, crash on resolution.

### 11D. BREAKDOWN DROP
Last beat of previous section is EMPTY (total silence). New section enters on beat 1 with crash + kick at maximum velocity. The silence before the hit creates enormous impact.

### 11E. HALF-TIME TRANSITION
Last 2 bars of a section switch to half-time feel (snare on 3 instead of 2 and 4) to signal the section change. Very common in post-hardcore.

---

## 12. SECTION DYNAMICS MAP

Use these guidelines for overall section intensity:

| Section      | Intensity | Typical Patterns                              | Velocity Range |
|-------------|-----------|-----------------------------------------------|---------------|
| Intro       | 2-4       | Sparse, ride bell, minimal kick               | 50-85         |
| Verse       | 4-6       | Driving but controlled, ghost notes active     | 70-105        |
| Pre-chorus  | 5-7       | Building, add open hats, busier kick          | 80-115        |
| Chorus      | 7-9       | Full, crashes on bar 1, ride/crash alternation | 95-127        |
| Bridge      | 3-6       | Variable, often contrasting verse             | 55-100        |
| Breakdown   | 8-10      | Half-time, HEAVY, sparse but hard             | 110-127       |
| Buildup     | 3→9       | Linear crescendo (see DAITRO QUIET BUILD)     | 50→120        |
| Blast       | 9-10      | Full blast beat, crashes                      | 100-127       |
| Outro       | 2-5       | Deconstructing, fading, sparse                | 40-85         |

---

## 13. PHYSICAL CONSTRAINTS REMINDER

A real drummer has:
- **Right hand**: Ride OR hi-hat OR crash (only one at a time, must travel between them)
- **Left hand**: Snare (ghost notes and accents) OR toms (during fills)
- **Right foot**: Kick drum
- **Left foot**: Hi-hat pedal (open/close control)

**Rules:**
- NEVER write ride + crash simultaneously (same hand)
- NEVER write hi-hat hit + ride simultaneously (same hand)
- Snare + tom simultaneously = impossible (same hand). Exception: snare + floor tom if using cross-sticking, but this is rare
- Kick + hi-hat pedal simultaneously IS possible (different feet) but uncomfortable at high speed
- Ghost notes happen BETWEEN ride/hat hits — the left hand sneaks in while the right hand is on the cymbal
- During a fill, the right hand leaves the cymbal to play toms — there is NO cymbal during a fill (except crash at the end)
- Maximum realistic speed for alternating kick-snare: ~220 BPM in sixteenths (blast beat ceiling for most humans)
