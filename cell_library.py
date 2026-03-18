# Cell Library — Phase 2
#
# Each cell is a dict with:
#   name, tags, time_sig, num_bars, humanize, role, hits
#   Optional: humanize_per_bar — dict of (bar_start, bar_end) → humanize override
#
# Single-bar cells: hits are (beat, sub, instrument, velocity_level) 4-tuples
# Multi-bar cells:  hits are (bar, beat, sub, instrument, velocity_level) 5-tuples
#   where bar is 1-indexed within the cell.
#
# Sub values: 0.0 = on beat, 0.25 = sixteenth, 0.5 = eighth, 0.75 = dotted eighth

import json
import os
import sys


# ── Phase 1 cells ──────────────────────────────────────────────────────────────

def _blast_traditional():
    """Traditional blast beat: K/S alternate every sixteenth, ride every sixteenth."""
    hits = []
    for beat in range(1, 5):
        hits.append((beat, 0.0, "kick", "accent"))
        hits.append((beat, 0.5, "kick", "accent"))
        hits.append((beat, 0.25, "snare", "accent"))
        hits.append((beat, 0.75, "snare", "accent"))
        hits.append((beat, 0.0, "ride", "accent"))
        hits.append((beat, 0.25, "ride", "normal"))
        hits.append((beat, 0.5, "ride", "accent"))
        hits.append((beat, 0.75, "ride", "normal"))
    return {
        "name": "blast_traditional",
        "tags": ["blast", "extreme", "screamo", "metal", "intense"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.8,
        "role": "groove",
        "hits": hits,
    }


def _dbeat_standard():
    """Standard d-beat: X.XX kick pattern, snare on 2/4 upbeats, HH eighths."""
    hits = [
        (1, 0.0, "kick", "accent"),
        (2, 0.0, "kick", "normal"),
        (2, 0.5, "kick", "normal"),
        (3, 0.0, "kick", "accent"),
        (4, 0.0, "kick", "normal"),
        (4, 0.5, "kick", "normal"),
        (1, 0.5, "snare", "accent"),
        (3, 0.5, "snare", "accent"),
        (1, 0.0, "hihat_closed", "normal"),
        (1, 0.5, "hihat_closed", "normal"),
        (2, 0.0, "hihat_closed", "normal"),
        (2, 0.5, "hihat_closed", "normal"),
        (3, 0.0, "hihat_closed", "normal"),
        (3, 0.5, "hihat_closed", "normal"),
        (4, 0.0, "hihat_closed", "normal"),
        (4, 0.5, "hihat_closed", "normal"),
    ]
    return {
        "name": "dbeat_standard",
        "tags": ["dbeat", "punk", "crust", "hardcore", "driving"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.4,
        "role": "groove",
        "hits": hits,
    }


def _shellac_floor_tom_drive():
    """Shellac floor tom drive: floor tom 1/3, snare 2/4, ride quarters. NO ghost notes."""
    hits = [
        (1, 0.0, "tom_floor", "accent"),
        (3, 0.0, "tom_floor", "accent"),
        (2, 0.0, "snare", "accent"),
        (4, 0.0, "snare", "accent"),
        (1, 0.0, "ride", "normal"),
        (2, 0.0, "ride", "normal"),
        (3, 0.0, "ride", "normal"),
        (4, 0.0, "ride", "normal"),
    ]
    return {
        "name": "shellac_floor_tom_drive",
        "tags": ["shellac", "noise_rock", "sparse", "precise", "driving"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.2,
        "role": "groove",
        "hits": hits,
    }


def _fugazi_driving_chorus():
    """Fugazi driving chorus: K on 1, 2+, 3. S on 2, 4. Ride eighths."""
    hits = [
        (1, 0.0, "kick", "accent"),
        (2, 0.5, "kick", "accent"),
        (3, 0.0, "kick", "accent"),
        (2, 0.0, "snare", "accent"),
        (4, 0.0, "snare", "accent"),
        (1, 0.0, "ride", "normal"),
        (1, 0.5, "ride", "normal"),
        (2, 0.0, "ride", "normal"),
        (2, 0.5, "ride", "normal"),
        (3, 0.0, "ride", "normal"),
        (3, 0.5, "ride", "normal"),
        (4, 0.0, "ride", "normal"),
        (4, 0.5, "ride", "normal"),
    ]
    return {
        "name": "fugazi_driving_chorus",
        "tags": ["fugazi", "posthardcore", "driving", "chorus"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.5,
        "role": "groove",
        "hits": hits,
    }


def _faraquet_displaced_4_4():
    """Faraquet displaced backbeat: 2-bar cell. Snare on 2+ and 4, ghost notes, ride eighths."""
    hits = [
        # --- Bar 1 ---
        (1, 1, 0.0, "kick", "accent"),
        (1, 2, 0.0, "kick", "normal"),
        (1, 3, 0.5, "kick", "normal"),
        (1, 2, 0.5, "snare", "accent"),
        (1, 4, 0.0, "snare", "accent"),
        (1, 1, 0.5, "snare_ghost", "ghost"),
        (1, 3, 0.0, "snare_ghost", "ghost"),
        (1, 4, 0.5, "snare_ghost", "ghost"),
        (1, 1, 0.0, "ride", "normal"),
        (1, 1, 0.5, "ride", "normal"),
        (1, 2, 0.0, "ride", "normal"),
        (1, 2, 0.5, "ride", "normal"),
        (1, 3, 0.0, "ride", "normal"),
        (1, 3, 0.5, "ride", "normal"),
        (1, 4, 0.0, "ride", "normal"),
        (1, 4, 0.5, "ride", "normal"),
        # --- Bar 2 (displaced) ---
        (2, 1, 0.5, "kick", "normal"),
        (2, 2, 0.5, "kick", "accent"),
        (2, 4, 0.0, "kick", "normal"),
        (2, 2, 0.0, "snare", "accent"),
        (2, 3, 0.5, "snare", "accent"),
        (2, 1, 0.0, "snare_ghost", "ghost"),
        (2, 3, 0.0, "snare_ghost", "ghost"),
        (2, 4, 0.5, "snare_ghost", "ghost"),
        (2, 1, 0.0, "ride", "normal"),
        (2, 1, 0.5, "ride", "normal"),
        (2, 2, 0.0, "ride", "normal"),
        (2, 2, 0.5, "ride", "normal"),
        (2, 3, 0.0, "ride", "normal"),
        (2, 3, 0.5, "ride", "normal"),
        (2, 4, 0.0, "ride", "normal"),
        (2, 4, 0.5, "ride", "normal"),
    ]
    return {
        "name": "faraquet_displaced_4_4",
        "tags": ["faraquet", "angular", "math", "posthardcore"],
        "time_sig": (4, 4),
        "num_bars": 2,
        "humanize": 0.65,
        "role": "groove",
        "hits": hits,
    }


def _raein_melodic_drive():
    """Raein melodic drive: K 1/3, S 2/4, HH eighths alternating accent/ghost, ghost snares."""
    hits = [
        (1, 0.0, "kick", "accent"),
        (3, 0.0, "kick", "accent"),
        (2, 0.0, "snare", "accent"),
        (4, 0.0, "snare", "accent"),
        (2, 0.5, "snare_ghost", "ghost"),
        (4, 0.5, "snare_ghost", "ghost"),
        (1, 0.0, "hihat_closed", "accent"),
        (1, 0.5, "hihat_closed", "ghost"),
        (2, 0.0, "hihat_closed", "accent"),
        (2, 0.5, "hihat_closed", "ghost"),
        (3, 0.0, "hihat_closed", "accent"),
        (3, 0.5, "hihat_closed", "ghost"),
        (4, 0.0, "hihat_closed", "accent"),
        (4, 0.5, "hihat_closed", "ghost"),
    ]
    return {
        "name": "raein_melodic_drive",
        "tags": ["raein", "euro_screamo", "melodic", "groovy", "driving"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.6,
        "role": "groove",
        "hits": hits,
    }


def _fill_linear_1bar():
    """Linear fill: single-stroke roll descending through kit on sixteenths. Velocity crescendo."""
    sequence = [
        (1, 0.0, "snare"),
        (1, 0.25, "tom_high"),
        (1, 0.5, "snare"),
        (1, 0.75, "tom_mid"),
        (2, 0.0, "tom_high"),
        (2, 0.25, "snare"),
        (2, 0.5, "tom_mid"),
        (2, 0.75, "tom_floor"),
        (3, 0.0, "kick"),
        (3, 0.25, "snare"),
        (3, 0.5, "tom_high"),
        (3, 0.75, "tom_mid"),
        (4, 0.0, "tom_floor"),
        (4, 0.25, "kick"),
        (4, 0.5, "tom_floor"),
        (4, 0.75, "kick"),
    ]
    hits = []
    for i, (beat, sub, inst) in enumerate(sequence):
        vel_value = 80 + int((120 - 80) * i / 15)
        if vel_value < 50:
            vel_level = "ghost"
        elif vel_value < 75:
            vel_level = "soft"
        elif vel_value < 105:
            vel_level = "normal"
        else:
            vel_level = "accent"
        hits.append((beat, sub, inst, vel_level))
    return {
        "name": "fill_linear_1bar",
        "tags": ["fill", "posthardcore", "general"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.4,
        "role": "fill",
        "hits": hits,
    }


# ── Phase 2 cells ──────────────────────────────────────────────────────────────

def _emoviolence_angular_breakdown():
    """Half-time breakdown. Every hit is a statement."""
    hits = [
        # Kick: 1, 3, 3.5
        (1, 0.0, "kick", "accent"),
        (3, 0.0, "kick", "accent"),
        (3, 0.5, "kick", "accent"),
        # Snare: 3 only (lands with kick)
        (3, 0.0, "snare", "accent"),
        # Floor tom: 4.5
        (4, 0.5, "tom_floor", "accent"),
    ]
    return {
        "name": "emoviolence_angular_breakdown",
        "tags": ["screamo", "emoviolence", "breakdown", "halftime", "heavy", "slow"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.6,
        "role": "groove",
        "hits": hits,
    }


def _emoviolence_blast_crash():
    """Traditional blast BUT crash on every quarter note. 2-bar cell."""
    hits = []
    for bar in range(1, 3):
        for beat in range(1, 5):
            # K/S alternating sixteenths
            hits.append((bar, beat, 0.0, "kick", "accent"))
            hits.append((bar, beat, 0.5, "kick", "accent"))
            hits.append((bar, beat, 0.25, "snare", "accent"))
            hits.append((bar, beat, 0.75, "snare", "accent"))
            # Crash on every quarter note
            hits.append((bar, beat, 0.0, "crash_1", "accent"))
    return {
        "name": "emoviolence_blast_crash",
        "tags": ["screamo", "emoviolence", "blast", "chaotic", "intense"],
        "time_sig": (4, 4),
        "num_bars": 2,
        "humanize": 0.9,
        "role": "groove",
        "hits": hits,
    }


def _emoviolence_chaotic_fill():
    """Beats 1-2 silence, beats 3-4: sixteenths across kit. No cymbals."""
    # 8 sixteenth positions on beats 3-4
    pattern = [
        (3, 0.0, "snare"),
        (3, 0.25, "tom_high"),
        (3, 0.5, "kick"),
        (3, 0.75, "tom_mid"),
        (4, 0.0, "tom_floor"),
        (4, 0.25, "snare"),
        (4, 0.5, "tom_high"),
        (4, 0.75, "kick"),
    ]
    hits = [(beat, sub, inst, "accent") for beat, sub, inst in pattern]
    return {
        "name": "emoviolence_chaotic_fill",
        "tags": ["screamo", "emoviolence", "fill", "chaotic"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.9,
        "role": "fill",
        "hits": hits,
    }


def _daitro_quiet_build():
    """8-bar build: ride bell → ride + kick → add snare/ghosts → full. Crescendo humanize."""
    hits = []
    # Bars 1-2: ride_bell quarter notes only
    for bar in (1, 2):
        for beat in range(1, 5):
            hits.append((bar, beat, 0.0, "ride_bell", "soft"))

    # Bars 3-4: ride eighths + kick on 1 and 3
    for bar in (3, 4):
        hits.append((bar, 1, 0.0, "kick", "normal"))
        hits.append((bar, 3, 0.0, "kick", "normal"))
        for beat in range(1, 5):
            hits.append((bar, beat, 0.0, "ride", "normal"))
            hits.append((bar, beat, 0.5, "ride", "normal"))

    # Bars 5-6: add snare 2/4, ghost snares, kick adds 2.5 syncopation
    for bar in (5, 6):
        hits.append((bar, 1, 0.0, "kick", "normal"))
        hits.append((bar, 2, 0.5, "kick", "normal"))
        hits.append((bar, 3, 0.0, "kick", "normal"))
        hits.append((bar, 2, 0.0, "snare", "soft"))
        hits.append((bar, 4, 0.0, "snare", "soft"))
        hits.append((bar, 2, 0.5, "snare_ghost", "ghost"))
        hits.append((bar, 4, 0.5, "snare_ghost", "ghost"))
        for beat in range(1, 5):
            hits.append((bar, beat, 0.0, "ride", "normal"))
            hits.append((bar, beat, 0.5, "ride", "normal"))

    # Bars 7-8: full — ride accent eighths, driving kick, snare accent
    for bar in (7, 8):
        hits.append((bar, 1, 0.0, "kick", "accent"))
        hits.append((bar, 2, 0.5, "kick", "accent"))
        hits.append((bar, 3, 0.0, "kick", "accent"))
        hits.append((bar, 2, 0.0, "snare", "accent"))
        hits.append((bar, 4, 0.0, "snare", "accent"))
        hits.append((bar, 2, 0.5, "snare_ghost", "ghost"))
        hits.append((bar, 4, 0.5, "snare_ghost", "ghost"))
        for beat in range(1, 5):
            # Skip ride on bar 7 beat 1 — crash_1 replaces it
            if not (bar == 7 and beat == 1):
                hits.append((bar, beat, 0.0, "ride", "accent"))
            hits.append((bar, beat, 0.5, "ride", "accent"))
    # Crash on bar 7 beat 1
    hits.append((7, 1, 0.0, "crash_1", "accent"))

    return {
        "name": "daitro_quiet_build",
        "tags": ["daitro", "euro_screamo", "build", "crescendo", "intro", "atmospheric"],
        "time_sig": (4, 4),
        "num_bars": 8,
        "humanize": 0.6,
        "humanize_per_bar": {(1, 2): 0.4, (3, 4): 0.5, (5, 6): 0.6, (7, 8): 0.8},
        "role": "groove",
        "hits": hits,
    }


def _daitro_tremolo_drive():
    """Fast kick doubles, snare 2/4 + ghost, ride eighths. Euro-screamo verse/chorus driver."""
    hits = [
        # Kick: 1, 1.5, 2.5, 3, 3.5, 4.5
        (1, 0.0, "kick", "accent"),
        (1, 0.5, "kick", "normal"),
        (2, 0.5, "kick", "normal"),
        (3, 0.0, "kick", "accent"),
        (3, 0.5, "kick", "normal"),
        (4, 0.5, "kick", "normal"),
        # Snare: 2 and 4 accent, 4.5 normal
        (2, 0.0, "snare", "accent"),
        (4, 0.0, "snare", "accent"),
        (4, 0.5, "snare", "normal"),
        # Ride eighths
        (1, 0.0, "ride", "normal"),
        (1, 0.5, "ride", "normal"),
        (2, 0.0, "ride", "normal"),
        (2, 0.5, "ride", "normal"),
        (3, 0.0, "ride", "normal"),
        (3, 0.5, "ride", "normal"),
        (4, 0.0, "ride", "normal"),
        (4, 0.5, "ride", "normal"),
    ]
    return {
        "name": "daitro_tremolo_drive",
        "tags": ["daitro", "euro_screamo", "driving", "intense", "verse", "chorus", "tremolo"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.6,
        "role": "groove",
        "hits": hits,
    }


def _daitro_blast_release():
    """4-bar blast release: bars 1-3 full blast, bar 4 half-blast (snare on eighths only)."""
    hits = []
    # Bars 1-2: traditional blast + crash on beat 1
    for bar in (1, 2):
        hits.append((bar, 1, 0.0, "crash_1", "accent"))
        for beat in range(1, 5):
            hits.append((bar, beat, 0.0, "kick", "accent"))
            hits.append((bar, beat, 0.5, "kick", "accent"))
            hits.append((bar, beat, 0.25, "snare", "accent"))
            hits.append((bar, beat, 0.75, "snare", "accent"))

    # Bar 3: blast + ride instead of crash
    bar = 3
    for beat in range(1, 5):
        hits.append((bar, beat, 0.0, "kick", "accent"))
        hits.append((bar, beat, 0.5, "kick", "accent"))
        hits.append((bar, beat, 0.25, "snare", "accent"))
        hits.append((bar, beat, 0.75, "snare", "accent"))
        hits.append((bar, beat, 0.0, "ride", "accent"))
        hits.append((bar, beat, 0.5, "ride", "normal"))

    # Bar 4: half-blast — snare on quarters only, kick stays on sixteenths
    bar = 4
    for beat in range(1, 5):
        hits.append((bar, beat, 0.0, "kick", "accent"))
        hits.append((bar, beat, 0.25, "kick", "normal"))
        hits.append((bar, beat, 0.5, "kick", "accent"))
        hits.append((bar, beat, 0.75, "kick", "normal"))
        hits.append((bar, beat, 0.0, "snare", "accent"))
        hits.append((bar, beat, 0.0, "ride", "normal"))
        hits.append((bar, beat, 0.5, "ride", "normal"))

    return {
        "name": "daitro_blast_release",
        "tags": ["daitro", "euro_screamo", "blast", "release", "climax", "intense"],
        "time_sig": (4, 4),
        "num_bars": 4,
        "humanize": 0.7,
        "role": "groove",
        "hits": hits,
    }


def _liturgy_burst_beat():
    """Burst beat: K/S near-simultaneous (flammed) on every sixteenth. 3-over-4 snare accents."""
    hits = []
    accent_positions = {0, 3, 6, 9, 12, 15}  # every 3rd = 3-over-4 polyrhythm
    pos = 0
    for beat in range(1, 5):
        for sub in (0.0, 0.25, 0.5, 0.75):
            # Kick: every sixteenth, all accent
            hits.append((beat, sub, "kick", "accent"))
            # Snare: every sixteenth, with sub offset +0.02 for flam
            vel = "accent" if pos in accent_positions else "normal"
            hits.append((beat, sub + 0.02, "snare", vel))
            pos += 1
    # Hi-hat open on quarter notes
    for beat in range(1, 5):
        hits.append((beat, 0.0, "hihat_open", "accent"))
    return {
        "name": "liturgy_burst_beat",
        "tags": ["liturgy", "black_metal", "blast", "experimental", "polyrhythmic", "intense"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.5,
        "role": "groove",
        "hits": hits,
    }


def _blackmetal_atmospheric():
    """Sparse atmospheric black metal: kick 1, ride bell pings, snare 3, hi-hat pedal quarters."""
    hits = [
        (1, 0.0, "kick", "normal"),
        (3, 0.0, "snare", "normal"),
        (2, 0.5, "ride_bell", "soft"),
        (4, 0.5, "ride_bell", "soft"),
        (1, 0.0, "hihat_pedal", "ghost"),
        (2, 0.0, "hihat_pedal", "ghost"),
        (3, 0.0, "hihat_pedal", "ghost"),
        (4, 0.0, "hihat_pedal", "ghost"),
    ]
    return {
        "name": "blackmetal_atmospheric",
        "tags": ["black_metal", "atmospheric", "post", "sparse", "intro", "bridge", "quiet"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.8,
        "role": "groove",
        "hits": hits,
    }


def _deafheaven_build_to_blast():
    """8-bar build: kick quarters → eighths → sixteenths → full blast."""
    hits = []

    # Bars 1-2: kick quarters, ride eighths, no snare
    for bar in (1, 2):
        for beat in range(1, 5):
            hits.append((bar, beat, 0.0, "kick", "normal"))
            hits.append((bar, beat, 0.0, "ride", "normal"))
            hits.append((bar, beat, 0.5, "ride", "normal"))

    # Bars 3-4: kick eighths, ride eighths, snare 2/4
    for bar in (3, 4):
        for beat in range(1, 5):
            hits.append((bar, beat, 0.0, "kick", "normal"))
            hits.append((bar, beat, 0.5, "kick", "normal"))
            hits.append((bar, beat, 0.0, "ride", "accent"))
            hits.append((bar, beat, 0.5, "ride", "normal"))
        hits.append((bar, 2, 0.0, "snare", "accent"))
        hits.append((bar, 4, 0.0, "snare", "accent"))

    # Bars 5-6: kick sixteenths, ride sixteenths, snare 2/4
    for bar in (5, 6):
        for beat in range(1, 5):
            hits.append((bar, beat, 0.0, "kick", "accent"))
            hits.append((bar, beat, 0.25, "kick", "normal"))
            hits.append((bar, beat, 0.5, "kick", "accent"))
            hits.append((bar, beat, 0.75, "kick", "normal"))
            hits.append((bar, beat, 0.0, "ride", "accent"))
            hits.append((bar, beat, 0.25, "ride", "normal"))
            hits.append((bar, beat, 0.5, "ride", "accent"))
            hits.append((bar, beat, 0.75, "ride", "normal"))
        hits.append((bar, 2, 0.0, "snare", "accent"))
        hits.append((bar, 4, 0.0, "snare", "accent"))

    # Bars 7-8: full traditional blast + crash on bar 7 beat 1
    hits.append((7, 1, 0.0, "crash_1", "accent"))
    for bar in (7, 8):
        for beat in range(1, 5):
            hits.append((bar, beat, 0.0, "kick", "accent"))
            hits.append((bar, beat, 0.5, "kick", "accent"))
            hits.append((bar, beat, 0.25, "snare", "accent"))
            hits.append((bar, beat, 0.75, "snare", "accent"))

    return {
        "name": "deafheaven_build_to_blast",
        "tags": ["deafheaven", "black_metal", "build", "transition", "climax", "intense"],
        "time_sig": (4, 4),
        "num_bars": 8,
        "humanize": 0.6,
        "humanize_per_bar": {(1, 2): 0.4, (3, 4): 0.5, (5, 6): 0.6, (7, 8): 0.8},
        "role": "groove",
        "hits": hits,
    }


# ── Transitions ────────────────────────────────────────────────────────────────

def _transition_crash_silence():
    """Beat 1: crash + kick. Rest of bar: silence."""
    hits = [
        (1, 0.0, "crash_1", "accent"),
        (1, 0.0, "kick", "accent"),
    ]
    return {
        "name": "transition_crash_silence",
        "tags": ["transition", "any"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.3,
        "role": "transition",
        "hits": hits,
    }


def _transition_half_time_shift():
    """2-bar half-time transition. Kick syncopation, snare on 3 only, ride eighths."""
    hits = []
    for bar in (1, 2):
        hits.append((bar, 1, 0.0, "kick", "accent"))
        hits.append((bar, 2, 0.5, "kick", "normal"))
        hits.append((bar, 3, 0.0, "kick", "accent"))
        hits.append((bar, 3, 0.0, "snare", "accent"))
        for beat in range(1, 5):
            hits.append((bar, beat, 0.0, "ride", "normal"))
            hits.append((bar, beat, 0.5, "ride", "normal"))
    return {
        "name": "transition_half_time_shift",
        "tags": ["transition", "posthardcore", "buildup", "halftime"],
        "time_sig": (4, 4),
        "num_bars": 2,
        "humanize": 0.5,
        "role": "transition",
        "hits": hits,
    }


def _transition_snare_roll_to_crash():
    """Beats 1-2 silence, beats 3-4 snare sixteenths crescendo, kick on last 2 sixteenths."""
    hits = []
    # 8 snare hits on beats 3-4 with velocity crescendo
    positions = [
        (3, 0.0), (3, 0.25), (3, 0.5), (3, 0.75),
        (4, 0.0), (4, 0.25), (4, 0.5), (4, 0.75),
    ]
    vel_levels = ["soft", "soft", "soft", "normal", "normal", "normal", "accent", "accent"]
    for (beat, sub), vel in zip(positions, vel_levels):
        hits.append((beat, sub, "snare", vel))
    # Kick on last 2 sixteenths of beat 4
    hits.append((4, 0.5, "kick", "accent"))
    hits.append((4, 0.75, "kick", "accent"))
    return {
        "name": "transition_snare_roll_to_crash",
        "tags": ["transition", "fill", "blast_entry", "screamo", "buildup"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.5,
        "role": "transition",
        "hits": hits,
    }


def _transition_cymbal_swell():
    """2-bar ride bell swell with velocity crescendo. Kick joins in bar 2."""
    hits = []
    # Bar 1: ride bell eighths, ghost → soft
    bar1_vels = ["ghost", "ghost", "ghost", "ghost", "soft", "soft", "soft", "soft"]
    idx = 0
    for beat in range(1, 5):
        hits.append((1, beat, 0.0, "ride_bell", bar1_vels[idx]))
        idx += 1
        hits.append((1, beat, 0.5, "ride_bell", bar1_vels[idx]))
        idx += 1
    # Bar 2: ride bell eighths, normal → accent + kick quarters
    bar2_vels = ["normal", "normal", "normal", "normal", "accent", "accent", "accent", "accent"]
    idx = 0
    for beat in range(1, 5):
        hits.append((2, beat, 0.0, "ride_bell", bar2_vels[idx]))
        idx += 1
        hits.append((2, beat, 0.5, "ride_bell", bar2_vels[idx]))
        idx += 1
        # Kick on quarters in bar 2, soft to normal
        vel = "soft" if beat <= 2 else "normal"
        hits.append((2, beat, 0.0, "kick", vel))
    return {
        "name": "transition_cymbal_swell",
        "tags": ["transition", "build", "euro_screamo", "atmospheric"],
        "time_sig": (4, 4),
        "num_bars": 2,
        "humanize": 0.5,
        "role": "transition",
        "hits": hits,
    }


# ── Extra fills ────────────────────────────────────────────────────────────────

def _fill_floor_tom_sparse():
    """3 floor tom hits only. Massive, sparse."""
    hits = [
        (1, 0.0, "tom_floor", "accent"),
        (2, 0.5, "tom_floor", "accent"),
        (4, 0.0, "tom_floor", "accent"),
    ]
    return {
        "name": "fill_floor_tom_sparse",
        "tags": ["fill", "noise_rock", "shellac", "heavy"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.3,
        "role": "fill",
        "hits": hits,
    }


# ── Odd meter cells ───────────────────────────────────────────────────────────

# -- 7/8 cells --

def _faraquet_7_8():
    """Faraquet displaced backbeat in 7/8. 2+2+3 grouping, ghost snares, ride on every beat."""
    hits = [
        # Bar 1
        (1, 1, 0.0, "kick", "accent"),
        (1, 3, 0.0, "kick", "normal"),
        (1, 5, 0.0, "kick", "normal"),
        (1, 4, 0.0, "snare", "accent"),
        (1, 7, 0.0, "snare", "accent"),
        (1, 2, 0.0, "snare_ghost", "ghost"),
        (1, 6, 0.0, "snare_ghost", "ghost"),
    ]
    for beat in range(1, 8):
        hits.append((1, beat, 0.0, "ride", "normal"))
    hits.extend([
        # Bar 2 (displaced)
        (2, 2, 0.0, "kick", "normal"),
        (2, 4, 0.0, "kick", "accent"),
        (2, 7, 0.0, "kick", "normal"),
        (2, 3, 0.0, "snare", "accent"),
        (2, 6, 0.0, "snare", "accent"),
        (2, 1, 0.0, "snare_ghost", "ghost"),
        (2, 5, 0.0, "snare_ghost", "ghost"),
    ])
    for beat in range(1, 8):
        hits.append((2, beat, 0.0, "ride", "normal"))
    return {
        "name": "faraquet_7_8",
        "tags": ["faraquet", "angular", "math", "posthardcore", "odd_meter"],
        "time_sig": (7, 8),
        "num_bars": 2,
        "humanize": 0.65,
        "role": "groove",
        "hits": hits,
    }


def _shellac_7_8():
    """Shellac 7/8: floor tom on 1, snare on 4, ride on every beat. 3+4 grouping."""
    hits = [
        (1, 0.0, "tom_floor", "accent"),
        (4, 0.0, "snare", "accent"),
    ]
    for beat in range(1, 8):
        hits.append((beat, 0.0, "ride", "normal"))
    return {
        "name": "shellac_7_8",
        "tags": ["shellac", "noise_rock", "sparse", "precise", "odd_meter"],
        "time_sig": (7, 8),
        "num_bars": 1,
        "humanize": 0.2,
        "role": "groove",
        "hits": hits,
    }


def _blast_7_8():
    """Traditional blast in 7/8: K/S alternating every sixteenth, ride on every position."""
    hits = []
    for beat in range(1, 8):
        hits.append((beat, 0.0, "kick", "accent"))
        hits.append((beat, 0.5, "snare", "accent"))
        hits.append((beat, 0.0, "ride", "accent"))
        hits.append((beat, 0.5, "ride", "normal"))
    return {
        "name": "blast_7_8",
        "tags": ["blast", "extreme", "screamo", "metal", "odd_meter"],
        "time_sig": (7, 8),
        "num_bars": 1,
        "humanize": 0.8,
        "role": "groove",
        "hits": hits,
    }


def _dbeat_7_8():
    """D-beat in 7/8: X.XX kick pattern adapted to 2+2+3 grouping. HH on every beat."""
    hits = [
        # Group 1 (1-2): K on 1, S upbeat, K doubles on 2
        (1, 0.0, "kick", "accent"),
        (1, 0.5, "snare", "accent"),
        (2, 0.0, "kick", "normal"),
        (2, 0.5, "kick", "normal"),
        # Group 2 (3-4): K on 3, S upbeat, K doubles on 4
        (3, 0.0, "kick", "accent"),
        (3, 0.5, "snare", "accent"),
        (4, 0.0, "kick", "normal"),
        (4, 0.5, "kick", "normal"),
        # Group 3 (5-6-7): K on 5, S upbeat, K on 6-7
        (5, 0.0, "kick", "accent"),
        (5, 0.5, "snare", "accent"),
        (6, 0.0, "kick", "normal"),
        (7, 0.0, "kick", "normal"),
    ]
    for beat in range(1, 8):
        hits.append((beat, 0.0, "hihat_closed", "normal"))
    return {
        "name": "dbeat_7_8",
        "tags": ["dbeat", "punk", "hardcore", "odd_meter"],
        "time_sig": (7, 8),
        "num_bars": 1,
        "humanize": 0.4,
        "role": "groove",
        "hits": hits,
    }


def _driving_7_8():
    """Driving 7/8: syncopated kick, snare backbeat, ride on every beat. 2+2+3."""
    hits = [
        (1, 0.0, "kick", "accent"),
        (3, 0.0, "kick", "accent"),
        (5, 0.0, "kick", "accent"),
        (6, 0.5, "kick", "normal"),
        (4, 0.0, "snare", "accent"),
        (7, 0.0, "snare", "accent"),
    ]
    for beat in range(1, 8):
        hits.append((beat, 0.0, "ride", "normal"))
    return {
        "name": "driving_7_8",
        "tags": ["posthardcore", "fugazi", "driving", "odd_meter"],
        "time_sig": (7, 8),
        "num_bars": 1,
        "humanize": 0.6,
        "role": "groove",
        "hits": hits,
    }


def _atmospheric_7_8():
    """Atmospheric black metal 7/8: sparse kick, ride bell pings, 3+4 grouping."""
    hits = [
        (1, 0.0, "kick", "normal"),
        (4, 0.0, "snare", "normal"),
        (3, 0.0, "ride_bell", "soft"),
        (6, 0.0, "ride_bell", "soft"),
        (1, 0.0, "hihat_pedal", "ghost"),
        (2, 0.0, "hihat_pedal", "ghost"),
        (4, 0.0, "hihat_pedal", "ghost"),
        (5, 0.0, "hihat_pedal", "ghost"),
    ]
    return {
        "name": "atmospheric_7_8",
        "tags": ["black_metal", "atmospheric", "sparse", "odd_meter"],
        "time_sig": (7, 8),
        "num_bars": 1,
        "humanize": 0.8,
        "role": "groove",
        "hits": hits,
    }


# -- 5/4 cells --

def _faraquet_5_4():
    """Faraquet displaced backbeat in 5/4. Ghost snares, ride eighths."""
    hits = [
        # Bar 1
        (1, 1, 0.0, "kick", "accent"),
        (1, 3, 0.0, "kick", "normal"),
        (1, 4, 0.5, "kick", "normal"),
        (1, 2, 0.5, "snare", "accent"),
        (1, 5, 0.0, "snare", "accent"),
        (1, 1, 0.5, "snare_ghost", "ghost"),
        (1, 4, 0.0, "snare_ghost", "ghost"),
    ]
    for beat in range(1, 6):
        hits.append((1, beat, 0.0, "ride", "normal"))
        hits.append((1, beat, 0.5, "ride", "normal"))
    hits.extend([
        # Bar 2 (displaced)
        (2, 2, 0.0, "kick", "normal"),
        (2, 4, 0.5, "kick", "accent"),
        (2, 3, 0.0, "snare", "accent"),
        (2, 5, 0.0, "snare", "accent"),
        (2, 1, 0.0, "snare_ghost", "ghost"),
        (2, 3, 0.5, "snare_ghost", "ghost"),
    ])
    for beat in range(1, 6):
        hits.append((2, beat, 0.0, "ride", "normal"))
        hits.append((2, beat, 0.5, "ride", "normal"))
    return {
        "name": "faraquet_5_4",
        "tags": ["faraquet", "angular", "math", "posthardcore", "odd_meter"],
        "time_sig": (5, 4),
        "num_bars": 2,
        "humanize": 0.65,
        "role": "groove",
        "hits": hits,
    }


def _shellac_5_4():
    """Shellac 5/4: floor tom on 1, snare on 4, ride quarters."""
    hits = [
        (1, 0.0, "tom_floor", "accent"),
        (4, 0.0, "snare", "accent"),
    ]
    for beat in range(1, 6):
        hits.append((beat, 0.0, "ride", "normal"))
    return {
        "name": "shellac_5_4",
        "tags": ["shellac", "noise_rock", "sparse", "precise", "odd_meter"],
        "time_sig": (5, 4),
        "num_bars": 1,
        "humanize": 0.2,
        "role": "groove",
        "hits": hits,
    }


def _driving_5_4():
    """Driving 5/4: syncopated kick, snare backbeat, ride eighths."""
    hits = [
        (1, 0.0, "kick", "accent"),
        (3, 0.0, "kick", "accent"),
        (4, 0.5, "kick", "normal"),
        (2, 0.0, "snare", "accent"),
        (5, 0.0, "snare", "accent"),
    ]
    for beat in range(1, 6):
        hits.append((beat, 0.0, "ride", "normal"))
        hits.append((beat, 0.5, "ride", "normal"))
    return {
        "name": "driving_5_4",
        "tags": ["posthardcore", "fugazi", "driving", "odd_meter"],
        "time_sig": (5, 4),
        "num_bars": 1,
        "humanize": 0.6,
        "role": "groove",
        "hits": hits,
    }


def _blast_5_4():
    """Traditional blast in 5/4: K/S alternating sixteenths, ride sixteenths."""
    hits = []
    for beat in range(1, 6):
        hits.append((beat, 0.0, "kick", "accent"))
        hits.append((beat, 0.5, "kick", "accent"))
        hits.append((beat, 0.25, "snare", "accent"))
        hits.append((beat, 0.75, "snare", "accent"))
        hits.append((beat, 0.0, "ride", "accent"))
        hits.append((beat, 0.25, "ride", "normal"))
        hits.append((beat, 0.5, "ride", "accent"))
        hits.append((beat, 0.75, "ride", "normal"))
    return {
        "name": "blast_5_4",
        "tags": ["blast", "extreme", "odd_meter"],
        "time_sig": (5, 4),
        "num_bars": 1,
        "humanize": 0.8,
        "role": "groove",
        "hits": hits,
    }


# -- 3/4 cells --

def _waltz_punk():
    """Punk waltz: kick on 1, snare on 2, HH eighths. Angular 3/4."""
    hits = [
        (1, 0.0, "kick", "accent"),
        (2, 0.5, "kick", "normal"),
        (2, 0.0, "snare", "accent"),
        (3, 0.0, "snare", "normal"),
        (1, 0.0, "hihat_closed", "accent"),
        (1, 0.5, "hihat_closed", "normal"),
        (2, 0.0, "hihat_closed", "accent"),
        (2, 0.5, "hihat_closed", "normal"),
        (3, 0.0, "hihat_closed", "accent"),
        (3, 0.5, "hihat_closed", "normal"),
    ]
    return {
        "name": "waltz_punk",
        "tags": ["posthardcore", "punk", "odd_meter", "angular"],
        "time_sig": (3, 4),
        "num_bars": 1,
        "humanize": 0.4,
        "role": "groove",
        "hits": hits,
    }


def _shellac_3_4():
    """Shellac 3/4: floor tom on 1, snare on 3, ride quarters."""
    hits = [
        (1, 0.0, "tom_floor", "accent"),
        (3, 0.0, "snare", "accent"),
        (1, 0.0, "ride", "normal"),
        (2, 0.0, "ride", "normal"),
        (3, 0.0, "ride", "normal"),
    ]
    return {
        "name": "shellac_3_4",
        "tags": ["shellac", "noise_rock", "sparse", "precise", "odd_meter"],
        "time_sig": (3, 4),
        "num_bars": 1,
        "humanize": 0.2,
        "role": "groove",
        "hits": hits,
    }


def _driving_3_4():
    """Driving 3/4: syncopated kick, snare on 3, ride eighths."""
    hits = [
        (1, 0.0, "kick", "accent"),
        (2, 0.5, "kick", "normal"),
        (3, 0.0, "snare", "accent"),
    ]
    for beat in range(1, 4):
        hits.append((beat, 0.0, "ride", "normal"))
        hits.append((beat, 0.5, "ride", "normal"))
    return {
        "name": "driving_3_4",
        "tags": ["posthardcore", "fugazi", "driving", "odd_meter"],
        "time_sig": (3, 4),
        "num_bars": 1,
        "humanize": 0.6,
        "role": "groove",
        "hits": hits,
    }


def _blast_3_4():
    """Traditional blast in 3/4: K/S alternating sixteenths, ride sixteenths."""
    hits = []
    for beat in range(1, 4):
        hits.append((beat, 0.0, "kick", "accent"))
        hits.append((beat, 0.5, "kick", "accent"))
        hits.append((beat, 0.25, "snare", "accent"))
        hits.append((beat, 0.75, "snare", "accent"))
        hits.append((beat, 0.0, "ride", "accent"))
        hits.append((beat, 0.25, "ride", "normal"))
        hits.append((beat, 0.5, "ride", "accent"))
        hits.append((beat, 0.75, "ride", "normal"))
    return {
        "name": "blast_3_4",
        "tags": ["blast", "extreme", "odd_meter"],
        "time_sig": (3, 4),
        "num_bars": 1,
        "humanize": 0.8,
        "role": "groove",
        "hits": hits,
    }


# -- 6/8 cells --

def _driving_6_8():
    """Driving 6/8: compound duple feel (3+3), kick on 1/4, snare on 4, HH every beat, ghost snares."""
    hits = [
        (1, 0.0, "kick", "accent"),
        (4, 0.0, "kick", "accent"),
        (4, 0.0, "snare", "accent"),
        (3, 0.0, "snare_ghost", "ghost"),
        (6, 0.0, "snare_ghost", "ghost"),
        (1, 0.0, "hihat_closed", "accent"),
        (2, 0.0, "hihat_closed", "normal"),
        (3, 0.0, "hihat_closed", "normal"),
        (4, 0.0, "hihat_closed", "accent"),
        (5, 0.0, "hihat_closed", "normal"),
        (6, 0.0, "hihat_closed", "normal"),
    ]
    return {
        "name": "driving_6_8",
        "tags": ["posthardcore", "driving", "odd_meter"],
        "time_sig": (6, 8),
        "num_bars": 1,
        "humanize": 0.6,
        "role": "groove",
        "hits": hits,
    }


def _shellac_6_8():
    """Shellac 6/8: floor tom on 1, snare on 4, ride on every beat."""
    hits = [
        (1, 0.0, "tom_floor", "accent"),
        (4, 0.0, "snare", "accent"),
    ]
    for beat in range(1, 7):
        hits.append((beat, 0.0, "ride", "normal"))
    return {
        "name": "shellac_6_8",
        "tags": ["shellac", "noise_rock", "sparse", "precise", "odd_meter"],
        "time_sig": (6, 8),
        "num_bars": 1,
        "humanize": 0.2,
        "role": "groove",
        "hits": hits,
    }


# ── 6/4 cells ─────────────────────────────────────────────────────────────────

def _postrock_6_4():
    """Post-rock 6/4: spacious feel, kick on 1+4, snare on 4, ride quarters, ghost snares."""
    hits = [
        (1, 0.0, "kick", "accent"),
        (4, 0.0, "kick", "normal"),
        (4, 0.0, "snare", "accent"),
        (2, 0.0, "snare_ghost", "ghost"),
        (5, 0.5, "snare_ghost", "ghost"),
    ]
    for beat in range(1, 7):
        hits.append((beat, 0.0, "ride", "normal"))
    return {
        "name": "postrock_6_4",
        "tags": ["postrock", "atmospheric", "sparse", "odd_meter"],
        "time_sig": (6, 4),
        "num_bars": 1,
        "humanize": 0.4,
        "role": "groove",
        "hits": hits,
    }


def _driving_6_4():
    """Driving 6/4: syncopated kick, snare on 4, ride eighths."""
    hits = [
        (1, 0.0, "kick", "accent"),
        (3, 0.5, "kick", "normal"),
        (5, 0.0, "kick", "normal"),
        (4, 0.0, "snare", "accent"),
        (6, 0.5, "snare", "normal"),
    ]
    for beat in range(1, 7):
        hits.append((beat, 0.0, "ride", "normal"))
        hits.append((beat, 0.5, "ride", "normal"))
    return {
        "name": "driving_6_4",
        "tags": ["posthardcore", "driving", "odd_meter"],
        "time_sig": (6, 4),
        "num_bars": 1,
        "humanize": 0.5,
        "role": "groove",
        "hits": hits,
    }


# ── Probability grid cells ─────────────────────────────────────────────────────
#
# Grid entries: (beat, sub, instrument, probability, velocity_level) — 5-tuple for single-bar
# Multi-bar grids: (bar, beat, sub, instrument, probability, velocity_level) — 6-tuple

def _prob_faraquet_4_4():
    """Faraquet-style angular math rock probability grid. Displaced snare, syncopated kicks."""
    grid = []
    # Ride on eighths, high probability
    for beat in range(1, 5):
        grid.append((beat, 0.0, "ride", 0.9, "normal"))
        grid.append((beat, 0.5, "ride", 0.9, "normal"))
    # Syncopated kicks — displaced, variable probability
    grid.extend([
        (1, 0.0, "kick", 0.7, "accent"),
        (1, 0.75, "kick", 0.4, "normal"),
        (2, 0.5, "kick", 0.6, "normal"),
        (3, 0.0, "kick", 0.65, "accent"),
        (3, 0.5, "kick", 0.45, "normal"),
        (4, 0.25, "kick", 0.5, "normal"),
    ])
    # Displaced snare
    grid.extend([
        (2, 0.0, "snare", 0.5, "accent"),
        (2, 0.5, "snare", 0.85, "accent"),
        (4, 0.0, "snare", 0.8, "accent"),
        (4, 0.5, "snare", 0.55, "normal"),
    ])
    # Ghost notes
    grid.extend([
        (1, 0.5, "snare_ghost", 0.35, "ghost"),
        (3, 0.25, "snare_ghost", 0.3, "ghost"),
        (3, 0.75, "snare_ghost", 0.45, "ghost"),
    ])
    return {
        "name": "prob_faraquet_4_4",
        "type": "probability",
        "tags": ["faraquet", "angular", "math", "generative"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.65,
        "role": "groove",
        "grid": grid,
    }


def _prob_shellac_4_4():
    """Shellac near-deterministic grid. Floor tom + snare + ride. Almost always the same."""
    grid = [
        # Floor tom on beats 1 and 3 — near-certain
        (1, 0.0, "tom_floor", 0.98, "accent"),
        (3, 0.0, "tom_floor", 0.98, "accent"),
        # Snare on 2 and 4 — near-certain
        (2, 0.0, "snare", 0.98, "accent"),
        (4, 0.0, "snare", 0.98, "accent"),
        # Ride on every quarter — deterministic
        (1, 0.0, "ride", 1.0, "normal"),
        (2, 0.0, "ride", 1.0, "normal"),
        (3, 0.0, "ride", 1.0, "normal"),
        (4, 0.0, "ride", 1.0, "normal"),
    ]
    return {
        "name": "prob_shellac_4_4",
        "type": "probability",
        "tags": ["shellac", "noise_rock", "generative"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.2,
        "role": "groove",
        "grid": grid,
    }


def _prob_posthardcore_4_4():
    """Post-hardcore/Fugazi driving probability grid. Ride eighths, solid backbeat."""
    grid = []
    # Ride on eighths
    for beat in range(1, 5):
        grid.append((beat, 0.0, "ride", 0.95, "normal"))
        grid.append((beat, 0.5, "ride", 0.95, "normal"))
    # Kick on 1 and 3
    grid.extend([
        (1, 0.0, "kick", 0.9, "accent"),
        (3, 0.0, "kick", 0.9, "accent"),
        (3, 0.5, "kick", 0.35, "normal"),
        (4, 0.5, "kick", 0.3, "normal"),
    ])
    # Snare on 2 and 4
    grid.extend([
        (2, 0.0, "snare", 0.85, "accent"),
        (4, 0.0, "snare", 0.85, "accent"),
    ])
    # Ghost notes
    grid.extend([
        (1, 0.5, "snare_ghost", 0.3, "ghost"),
        (3, 0.5, "snare_ghost", 0.4, "ghost"),
    ])
    # Occasional hi-hat open
    grid.extend([
        (2, 0.5, "hihat_open", 0.15, "normal"),
        (4, 0.5, "hihat_open", 0.15, "normal"),
    ])
    return {
        "name": "prob_posthardcore_4_4",
        "type": "probability",
        "tags": ["posthardcore", "fugazi", "driving", "generative"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.6,
        "role": "groove",
        "grid": grid,
    }


def _prob_dbeat_4_4():
    """D-beat probability grid. Classic X.XX kick pattern with HH eighths."""
    grid = []
    # HH on eighths
    for beat in range(1, 5):
        grid.append((beat, 0.0, "hihat_closed", 0.9, "normal"))
        grid.append((beat, 0.5, "hihat_closed", 0.9, "normal"))
    # D-beat kick pattern: 1, 2, 2+, 4 (X.XX)
    grid.extend([
        (1, 0.0, "kick", 0.95, "accent"),
        (2, 0.0, "kick", 0.95, "normal"),
        (2, 0.5, "kick", 0.95, "normal"),
        (4, 0.0, "kick", 0.95, "normal"),
    ])
    # Snare on upbeats 1+ and 3+
    grid.extend([
        (1, 0.5, "snare", 0.95, "accent"),
        (2, 0.5, "snare", 0.95, "accent"),
        (3, 0.0, "snare", 0.95, "accent"),
        (3, 0.5, "snare", 0.95, "accent"),
        (4, 0.5, "snare", 0.95, "accent"),
    ])
    return {
        "name": "prob_dbeat_4_4",
        "type": "probability",
        "tags": ["dbeat", "punk", "generative"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.4,
        "role": "groove",
        "grid": grid,
    }


def _prob_blast_4_4():
    """Blast beat probability grid. K/S alternating sixteenths, ride sixteenths."""
    grid = []
    for beat in range(1, 5):
        # Kick on downbeats and +
        grid.append((beat, 0.0, "kick", 0.92, "accent"))
        grid.append((beat, 0.5, "kick", 0.92, "accent"))
        # Snare on e and a
        grid.append((beat, 0.25, "snare", 0.92, "accent"))
        grid.append((beat, 0.75, "snare", 0.92, "accent"))
        # Ride on all sixteenths
        grid.append((beat, 0.0, "ride", 0.88, "accent"))
        grid.append((beat, 0.25, "ride", 0.88, "normal"))
        grid.append((beat, 0.5, "ride", 0.88, "accent"))
        grid.append((beat, 0.75, "ride", 0.88, "normal"))
    # Crash on beat 1 — rare
    grid.append((1, 0.0, "crash_1", 0.3, "accent"))
    return {
        "name": "prob_blast_4_4",
        "type": "probability",
        "tags": ["blast", "extreme", "generative"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.8,
        "role": "groove",
        "grid": grid,
    }


def _prob_euro_screamo_4_4():
    """Euro-screamo/Daitro probability grid. Driving with ghost note texture."""
    grid = []
    # Ride on eighths
    for beat in range(1, 5):
        grid.append((beat, 0.0, "ride", 0.95, "normal"))
        grid.append((beat, 0.5, "ride", 0.95, "normal"))
    # Kick on 1 and 3
    grid.extend([
        (1, 0.0, "kick", 0.85, "accent"),
        (3, 0.0, "kick", 0.85, "accent"),
        (2, 0.5, "kick", 0.4, "normal"),
        (4, 0.5, "kick", 0.35, "normal"),
    ])
    # Snare on 2 and 4
    grid.extend([
        (2, 0.0, "snare", 0.9, "accent"),
        (4, 0.0, "snare", 0.9, "accent"),
    ])
    # Ghost notes — textural
    grid.extend([
        (1, 0.5, "snare_ghost", 0.35, "ghost"),
        (2, 0.25, "snare_ghost", 0.35, "ghost"),
        (3, 0.5, "snare_ghost", 0.35, "ghost"),
        (4, 0.25, "snare_ghost", 0.35, "ghost"),
    ])
    return {
        "name": "prob_euro_screamo_4_4",
        "type": "probability",
        "tags": ["euro_screamo", "daitro", "generative"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.55,
        "role": "groove",
        "grid": grid,
    }


def _prob_faraquet_7_8():
    """Faraquet angular 7/8 probability grid. 2+2+3 grouping."""
    grid = []
    # Ride on all 7 eighth-note beats
    for beat in range(1, 8):
        grid.append((beat, 0.0, "ride", 0.95, "normal"))
    # Kick on group downbeats: 1, 3, 5
    grid.extend([
        (1, 0.0, "kick", 0.9, "accent"),
        (3, 0.0, "kick", 0.9, "accent"),
        (5, 0.0, "kick", 0.9, "accent"),
        (6, 0.5, "kick", 0.4, "normal"),
    ])
    # Snare on 2 and 6 (group offbeats)
    grid.extend([
        (2, 0.0, "snare", 0.85, "accent"),
        (6, 0.0, "snare", 0.85, "accent"),
        (4, 0.0, "snare", 0.45, "normal"),
    ])
    # Ghost notes
    grid.extend([
        (1, 0.5, "snare_ghost", 0.3, "ghost"),
        (5, 0.5, "snare_ghost", 0.35, "ghost"),
        (7, 0.0, "snare_ghost", 0.4, "ghost"),
    ])
    return {
        "name": "prob_faraquet_7_8",
        "type": "probability",
        "tags": ["faraquet", "angular", "odd_meter", "generative"],
        "time_sig": (7, 8),
        "num_bars": 1,
        "humanize": 0.65,
        "role": "groove",
        "grid": grid,
    }


# ── Phase 3: Style palette expansion ──────────────────────────────────────────

def _motorik_pulse():
    """Motorik 4/4: HH closed eighths, kick 1/3, snare 2/4. Steady machine beat."""
    hits = []
    for beat in range(1, 5):
        hits.append((beat, 0.0, "hihat_closed", "accent"))
        hits.append((beat, 0.5, "hihat_closed", "normal"))
    hits.extend([
        (1, 0.0, "kick", "accent"),
        (3, 0.0, "kick", "accent"),
        (2, 0.0, "snare", "accent"),
        (4, 0.0, "snare", "accent"),
    ])
    return {
        "name": "motorik_pulse",
        "tags": ["motorik", "krautrock", "sonic_youth", "post_punk", "driving", "verse"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.25,
        "role": "groove",
        "hits": hits,
    }


def _motorik_build():
    """4-bar motorik crescendo: ghost → soft → normal + HH open → accent."""
    hits = []
    vel_map = {1: "ghost", 2: "soft", 3: "normal", 4: "accent"}
    for bar in range(1, 5):
        vel = vel_map[bar]
        for beat in range(1, 5):
            hits.append((bar, beat, 0.0, "hihat_closed", vel))
            hits.append((bar, beat, 0.5, "hihat_closed", vel))
        hits.extend([
            (bar, 1, 0.0, "kick", vel),
            (bar, 3, 0.0, "kick", vel),
            (bar, 2, 0.0, "snare", vel),
            (bar, 4, 0.0, "snare", vel),
        ])
    # Bar 3: add hihat_open on beat 4 sub 0.5 (remove hihat_closed at that position)
    hits = [(b, bt, s, i, v) for (b, bt, s, i, v) in hits
            if not (b == 3 and bt == 4 and s == 0.5 and i == "hihat_closed")]
    hits.append((3, 4, 0.5, "hihat_open", "normal"))
    return {
        "name": "motorik_build",
        "tags": ["motorik", "slint", "build", "crescendo", "atmospheric"],
        "time_sig": (4, 4),
        "num_bars": 4,
        "humanize": 0.4,
        "humanize_per_bar": {(1, 2): 0.3, (3, 4): 0.4},
        "role": "groove",
        "hits": hits,
    }


def _slint_explosion():
    """Slint climax: heavy kick pattern, snare 2/4 accent, floor tom, ride eighths."""
    hits = [
        (1, 0.0, "kick", "accent"),
        (2, 0.5, "kick", "accent"),
        (3, 0.0, "kick", "accent"),
        (4, 0.0, "kick", "accent"),
        (2, 0.0, "snare", "accent"),
        (4, 0.0, "snare", "accent"),
        (3, 0.5, "tom_floor", "accent"),
    ]
    for beat in range(1, 5):
        hits.append((beat, 0.0, "ride", "accent"))
        hits.append((beat, 0.5, "ride", "normal"))
    return {
        "name": "slint_explosion",
        "tags": ["slint", "post_punk", "driving", "intense", "climax"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.5,
        "role": "groove",
        "hits": hits,
    }


def _athletic_angular():
    """Drive Like Jehu angular 2-bar: busy syncopated kick, ride eighths, ghost snares, floor tom."""
    hits = [
        # --- Bar 1 ---
        (1, 1, 0.0, "kick", "accent"),
        (1, 1, 0.5, "kick", "normal"),
        (1, 3, 0.0, "kick", "accent"),
        (1, 3, 0.5, "kick", "normal"),
        (1, 4, 0.5, "kick", "normal"),
        (1, 2, 0.0, "snare", "accent"),
        (1, 4, 0.0, "snare", "accent"),
        (1, 1, 0.5, "snare_ghost", "ghost"),
        (1, 3, 0.5, "snare_ghost", "ghost"),
        (1, 4, 0.5, "snare_ghost", "ghost"),
        (1, 2, 0.5, "tom_floor", "accent"),
        (1, 1, 0.0, "ride", "accent"),
        (1, 1, 0.5, "ride", "normal"),
        (1, 2, 0.0, "ride", "normal"),
        (1, 2, 0.5, "ride", "normal"),
        (1, 3, 0.0, "ride", "accent"),
        (1, 3, 0.5, "ride", "normal"),
        (1, 4, 0.0, "ride", "normal"),
        (1, 4, 0.5, "ride", "normal"),
        # --- Bar 2 (shifted) ---
        (2, 1, 0.0, "kick", "accent"),
        (2, 2, 0.5, "kick", "normal"),
        (2, 3, 0.0, "kick", "accent"),
        (2, 4, 0.0, "kick", "normal"),
        (2, 4, 0.5, "kick", "normal"),
        (2, 2, 0.5, "snare", "accent"),
        (2, 4, 0.0, "snare", "accent"),
        (2, 1, 0.5, "snare_ghost", "ghost"),
        (2, 3, 0.5, "snare_ghost", "ghost"),
        (2, 4, 0.5, "tom_floor", "accent"),
        (2, 1, 0.0, "ride", "normal"),
        (2, 1, 0.5, "ride", "normal"),
        (2, 2, 0.0, "ride", "accent"),
        (2, 2, 0.5, "ride", "normal"),
        (2, 3, 0.0, "ride", "normal"),
        (2, 3, 0.5, "ride", "normal"),
        (2, 4, 0.0, "ride", "accent"),
        (2, 4, 0.5, "ride", "normal"),
    ]
    return {
        "name": "athletic_angular",
        "tags": ["athletic", "angular", "driving", "drive_like_jehu", "posthardcore", "intense"],
        "time_sig": (4, 4),
        "num_bars": 2,
        "humanize": 0.7,
        "role": "groove",
        "hits": hits,
    }


def _postpunk_machine():
    """Post-punk machine beat: kick 1/3, snare 2/4, HH closed eighths. No ghost, no ride, no toms."""
    hits = []
    for beat in range(1, 5):
        hits.append((beat, 0.0, "hihat_closed", "accent"))
        hits.append((beat, 0.5, "hihat_closed", "normal"))
    hits.extend([
        (1, 0.0, "kick", "accent"),
        (3, 0.0, "kick", "accent"),
        (2, 0.0, "snare", "accent"),
        (4, 0.0, "snare", "accent"),
    ])
    return {
        "name": "postpunk_machine",
        "tags": ["post_punk", "motorik", "driving", "verse"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.3,
        "role": "groove",
        "hits": hits,
    }


def _postpunk_busy():
    """Athletic post-punk 2-bar: busy kick, ride eighths, ghost snares, HH open accent."""
    hits = [
        # --- Bar 1 ---
        (1, 1, 0.0, "kick", "accent"),
        (1, 1, 0.5, "kick", "normal"),
        (1, 2, 0.5, "kick", "normal"),
        (1, 3, 0.0, "kick", "accent"),
        (1, 3, 0.5, "kick", "normal"),
        (1, 4, 0.5, "kick", "normal"),
        (1, 2, 0.0, "snare", "accent"),
        (1, 4, 0.0, "snare", "accent"),
        (1, 1, 0.5, "snare_ghost", "ghost"),
        (1, 3, 0.5, "snare_ghost", "ghost"),
        (1, 1, 0.0, "ride", "accent"),
        (1, 1, 0.5, "ride", "normal"),
        (1, 2, 0.0, "ride", "normal"),
        (1, 2, 0.5, "ride", "normal"),
        (1, 3, 0.0, "ride", "normal"),
        (1, 3, 0.5, "ride", "normal"),
        (1, 4, 0.0, "ride", "normal"),
        # hihat_open on beat 4 sub 0.5 instead of ride
        (1, 4, 0.5, "hihat_open", "accent"),
        # --- Bar 2 ---
        (2, 1, 0.0, "kick", "accent"),
        (2, 2, 0.0, "kick", "normal"),
        (2, 2, 0.5, "kick", "normal"),
        (2, 3, 0.5, "kick", "normal"),
        (2, 4, 0.0, "kick", "normal"),
        (2, 4, 0.5, "kick", "normal"),
        (2, 2, 0.0, "snare", "accent"),
        (2, 4, 0.0, "snare", "accent"),
        (2, 2, 0.5, "snare_ghost", "ghost"),
        (2, 4, 0.5, "snare_ghost", "ghost"),
        (2, 1, 0.0, "ride", "accent"),
        (2, 1, 0.5, "ride", "normal"),
        (2, 2, 0.0, "ride", "normal"),
        (2, 2, 0.5, "ride", "normal"),
        (2, 3, 0.0, "ride", "normal"),
        (2, 3, 0.5, "ride", "normal"),
        (2, 4, 0.0, "ride", "normal"),
        (2, 4, 0.5, "ride", "normal"),
    ]
    return {
        "name": "postpunk_busy",
        "tags": ["post_punk", "athletic", "atdi", "blood_brothers", "driving", "intense"],
        "time_sig": (4, 4),
        "num_bars": 2,
        "humanize": 0.65,
        "role": "groove",
        "hits": hits,
    }


def _unwound_dynamics():
    """Unwound 4-bar dynamic cell: quiet ride_bell/rim → loud ride/kick/snare explosion."""
    hits = [
        # --- Bars 1-2 (quiet) ---
        # Ride bell quarters
        (1, 1, 0.0, "ride_bell", "soft"),
        (1, 2, 0.0, "ride_bell", "soft"),
        (1, 3, 0.0, "ride_bell", "soft"),
        (1, 4, 0.0, "ride_bell", "soft"),
        (1, 3, 0.0, "snare_rim", "soft"),
        (1, 2, 0.0, "hihat_pedal", "ghost"),
        (1, 4, 0.0, "hihat_pedal", "ghost"),
        (2, 1, 0.0, "ride_bell", "soft"),
        (2, 2, 0.0, "ride_bell", "soft"),
        (2, 3, 0.0, "ride_bell", "soft"),
        (2, 4, 0.0, "ride_bell", "soft"),
        (2, 3, 0.0, "snare_rim", "soft"),
        (2, 2, 0.0, "hihat_pedal", "ghost"),
        (2, 4, 0.0, "hihat_pedal", "ghost"),
        (2, 2, 0.5, "snare_ghost", "ghost"),
        # --- Bars 3-4 (loud) ---
        (3, 1, 0.0, "kick", "accent"),
        (3, 2, 0.5, "kick", "accent"),
        (3, 3, 0.0, "kick", "accent"),
        (3, 2, 0.0, "snare", "accent"),
        (3, 4, 0.0, "snare", "accent"),
        (3, 3, 0.5, "tom_floor", "accent"),
        (4, 1, 0.0, "kick", "accent"),
        (4, 3, 0.0, "kick", "accent"),
        (4, 3, 0.5, "kick", "accent"),
        (4, 4, 0.0, "kick", "accent"),
        (4, 2, 0.0, "snare", "accent"),
        (4, 4, 0.0, "snare", "accent"),
        (4, 3, 0.5, "tom_floor", "accent"),
    ]
    # Ride eighths for bars 3-4
    for bar in (3, 4):
        for beat in range(1, 5):
            hits.append((bar, beat, 0.0, "ride", "accent"))
            hits.append((bar, beat, 0.5, "ride", "accent"))
    return {
        "name": "unwound_dynamics",
        "tags": ["unwound", "noise_rock", "dynamic", "atmospheric", "driving"],
        "time_sig": (4, 4),
        "num_bars": 4,
        "humanize": 0.55,
        "humanize_per_bar": {(1, 2): 0.6, (3, 4): 0.5},
        "role": "groove",
        "hits": hits,
    }


def _city_of_caterpillar_build():
    """8-bar screamo crescendo: ride bell → ride eighths → add floor tom/ghost → full blast."""
    hits = []
    # Bars 1-2: ride_bell quarters only
    for bar in (1, 2):
        for beat in range(1, 5):
            hits.append((bar, beat, 0.0, "ride_bell", "soft"))
    # Bars 3-4: ride_bell eighths + hihat_pedal 2/4
    for bar in (3, 4):
        for beat in range(1, 5):
            hits.append((bar, beat, 0.0, "ride_bell", "normal"))
            hits.append((bar, beat, 0.5, "ride_bell", "normal"))
        hits.append((bar, 2, 0.0, "hihat_pedal", "soft"))
        hits.append((bar, 4, 0.0, "hihat_pedal", "soft"))
    # Bars 5-6: ride (bow) eighths + floor tom beat 1 + snare_ghost beat 3
    for bar in (5, 6):
        for beat in range(1, 5):
            hits.append((bar, beat, 0.0, "ride", "normal"))
            hits.append((bar, beat, 0.5, "ride", "normal"))
        hits.append((bar, 1, 0.0, "tom_floor", "soft"))
        hits.append((bar, 3, 0.0, "snare_ghost", "ghost"))
    # Bars 7-8: full — kick, snare, ride accent, floor tom, crash bar 7
    for bar in (7, 8):
        hits.extend([
            (bar, 1, 0.0, "kick", "accent"),
            (bar, 2, 0.5, "kick", "accent"),
            (bar, 3, 0.0, "kick", "accent"),
            (bar, 2, 0.0, "snare", "accent"),
            (bar, 4, 0.0, "snare", "accent"),
        ])
        for beat in range(1, 5):
            # Skip ride on bar 7 beat 1 — crash_1 replaces it
            if not (bar == 7 and beat == 1):
                hits.append((bar, beat, 0.0, "ride", "accent"))
            hits.append((bar, beat, 0.5, "ride", "accent"))
        hits.append((bar, 1, 0.0, "tom_floor", "normal"))
    hits.append((7, 1, 0.0, "crash_1", "accent"))
    return {
        "name": "city_of_caterpillar_build",
        "tags": ["screamo", "emoviolence", "city_of_caterpillar", "build", "crescendo", "atmospheric", "intro"],
        "time_sig": (4, 4),
        "num_bars": 8,
        "humanize": 0.6,
        "humanize_per_bar": {(1, 2): 0.35, (3, 4): 0.45, (5, 6): 0.55, (7, 8): 0.7},
        "role": "groove",
        "hits": hits,
    }


def _prob_postpunk_4_4():
    """Post-punk probability grid. HH closed eighths, kick 1/3, snare 2/4, rare HH open."""
    grid = []
    # HH closed eighths
    for beat in range(1, 5):
        grid.append((beat, 0.0, "hihat_closed", 1.0, "normal"))
        grid.append((beat, 0.5, "hihat_closed", 1.0, "normal"))
    # Kick on 1/3 near-certain, rare syncopation
    grid.extend([
        (1, 0.0, "kick", 0.95, "accent"),
        (3, 0.0, "kick", 0.95, "accent"),
        (2, 0.5, "kick", 0.15, "normal"),
        (4, 0.5, "kick", 0.15, "normal"),
    ])
    # Snare on 2/4
    grid.extend([
        (2, 0.0, "snare", 0.95, "accent"),
        (4, 0.0, "snare", 0.95, "accent"),
    ])
    # Rare HH open
    grid.append((4, 0.5, "hihat_open", 0.08, "normal"))
    # Rare ghost snare
    grid.extend([
        (2, 0.5, "snare_ghost", 0.05, "ghost"),
        (4, 0.5, "snare_ghost", 0.05, "ghost"),
    ])
    return {
        "name": "prob_postpunk_4_4",
        "type": "probability",
        "tags": ["post_punk", "motorik", "generative"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.3,
        "role": "groove",
        "grid": grid,
    }


def _prob_angular_athletic_4_4():
    """Athletic angular probability grid. Ride eighths, variable kick, displaced snare, ghost notes."""
    grid = []
    # Ride eighths with accent on 1/3
    for beat in range(1, 5):
        vel = "accent" if beat in (1, 3) else "normal"
        prob = 0.95 if beat in (1, 3) else 0.9
        grid.append((beat, 0.0, "ride", prob, vel))
        grid.append((beat, 0.5, "ride", 0.9, "normal"))
    # Kick: variable syncopation
    grid.extend([
        (1, 0.0, "kick", 0.85, "accent"),
        (1, 0.5, "kick", 0.55, "normal"),
        (2, 0.5, "kick", 0.6, "normal"),
        (3, 0.0, "kick", 0.8, "accent"),
        (3, 0.5, "kick", 0.5, "normal"),
        (4, 0.5, "kick", 0.45, "normal"),
    ])
    # Snare on 2/4
    grid.extend([
        (2, 0.0, "snare", 0.8, "accent"),
        (4, 0.0, "snare", 0.85, "accent"),
    ])
    # Ghost snares on upbeat eighths
    grid.extend([
        (1, 0.5, "snare_ghost", 0.4, "ghost"),
        (2, 0.5, "snare_ghost", 0.4, "ghost"),
        (3, 0.5, "snare_ghost", 0.4, "ghost"),
        (4, 0.5, "snare_ghost", 0.4, "ghost"),
    ])
    # Floor tom on 2.5/4.5
    grid.extend([
        (2, 0.5, "tom_floor", 0.3, "accent"),
        (4, 0.5, "tom_floor", 0.3, "accent"),
    ])
    return {
        "name": "prob_angular_athletic_4_4",
        "type": "probability",
        "tags": ["athletic", "angular", "drive_like_jehu", "atdi", "generative"],
        "time_sig": (4, 4),
        "num_bars": 1,
        "humanize": 0.65,
        "role": "groove",
        "grid": grid,
    }


def _prob_slint_4_4():
    """Slint 4-bar probability grid: quiet ride bell → loud ride/kick/snare explosion."""
    grid = []
    # Bars 1-2 (quiet): ride_bell quarters, hihat pedal, sparse snare_rim/kick
    for bar in (1, 2):
        for beat in range(1, 5):
            grid.append((bar, beat, 0.0, "ride_bell", 0.85, "soft"))
        grid.append((bar, 2, 0.0, "hihat_pedal", 0.6, "ghost"))
        grid.append((bar, 4, 0.0, "hihat_pedal", 0.6, "ghost"))
        grid.append((bar, 3, 0.0, "snare_rim", 0.5, "soft"))
        grid.append((bar, 1, 0.0, "kick", 0.4, "soft"))
    # Bars 3-4 (loud): ride eighths, kick, snare, floor tom, crash
    for bar in (3, 4):
        for beat in range(1, 5):
            grid.append((bar, beat, 0.0, "ride", 0.92, "accent"))
            grid.append((bar, beat, 0.5, "ride", 0.92, "accent"))
        grid.extend([
            (bar, 1, 0.0, "kick", 0.9, "accent"),
            (bar, 2, 0.5, "kick", 0.85, "accent"),
            (bar, 3, 0.0, "kick", 0.9, "accent"),
            (bar, 4, 0.0, "kick", 0.85, "accent"),
        ])
        grid.extend([
            (bar, 2, 0.0, "snare", 0.9, "accent"),
            (bar, 4, 0.0, "snare", 0.9, "accent"),
        ])
        grid.append((bar, 3, 0.5, "tom_floor", 0.5, "accent"))
    # Crash on bar 3 beat 1
    grid.append((3, 1, 0.0, "crash_1", 0.6, "accent"))
    return {
        "name": "prob_slint_4_4",
        "type": "probability",
        "tags": ["slint", "post_punk", "dynamic", "build", "generative"],
        "time_sig": (4, 4),
        "num_bars": 4,
        "humanize": 0.5,
        "role": "groove",
        "grid": grid,
    }


# ── Registry ───────────────────────────────────────────────────────────────────

CELLS = {cell["name"]: cell for cell in [
    # Phase 1
    _blast_traditional(),
    _dbeat_standard(),
    _shellac_floor_tom_drive(),
    _fugazi_driving_chorus(),
    _faraquet_displaced_4_4(),
    _raein_melodic_drive(),
    _fill_linear_1bar(),
    # Phase 2 grooves
    _emoviolence_angular_breakdown(),
    _emoviolence_blast_crash(),
    _daitro_quiet_build(),
    _daitro_tremolo_drive(),
    _daitro_blast_release(),
    _liturgy_burst_beat(),
    _blackmetal_atmospheric(),
    _deafheaven_build_to_blast(),
    # Phase 2 fills
    _emoviolence_chaotic_fill(),
    _fill_floor_tom_sparse(),
    # Phase 2 transitions
    _transition_crash_silence(),
    _transition_half_time_shift(),
    _transition_snare_roll_to_crash(),
    _transition_cymbal_swell(),
    # Odd meter cells
    _faraquet_7_8(),
    _shellac_7_8(),
    _blast_7_8(),
    _dbeat_7_8(),
    _driving_7_8(),
    _atmospheric_7_8(),
    _faraquet_5_4(),
    _shellac_5_4(),
    _driving_5_4(),
    _blast_5_4(),
    _waltz_punk(),
    _shellac_3_4(),
    _driving_3_4(),
    _blast_3_4(),
    _driving_6_8(),
    _shellac_6_8(),
    # 6/4 cells
    _postrock_6_4(),
    _driving_6_4(),
    # Probability grid cells
    _prob_faraquet_4_4(),
    _prob_shellac_4_4(),
    _prob_posthardcore_4_4(),
    _prob_dbeat_4_4(),
    _prob_blast_4_4(),
    _prob_euro_screamo_4_4(),
    _prob_faraquet_7_8(),
    # Phase 3: Style palette expansion
    _motorik_pulse(),
    _motorik_build(),
    _slint_explosion(),
    _athletic_angular(),
    _postpunk_machine(),
    _postpunk_busy(),
    _unwound_dynamics(),
    _city_of_caterpillar_build(),
    _prob_postpunk_4_4(),
    _prob_angular_athletic_4_4(),
    _prob_slint_4_4(),
]}

USER_CELLS_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "user_cells")


def load_user_cells(directory=None):
    """Load user cell JSON files from user_cells/ directory."""
    if directory is None:
        directory = USER_CELLS_DIR
    user_cells = {}
    if not os.path.isdir(directory):
        return user_cells
    for filename in sorted(os.listdir(directory)):
        if not filename.endswith(".json"):
            continue
        filepath = os.path.join(directory, filename)
        try:
            with open(filepath, "r") as f:
                cell = json.load(f)
            cell["time_sig"] = tuple(cell["time_sig"])
            cell["hits"] = [tuple(h) for h in cell["hits"]]
            cell.setdefault("humanize", 0.5)
            cell.setdefault("role", "groove")
            cell.setdefault("tags", ["imported"])
            user_cells[cell["name"]] = cell
        except (json.JSONDecodeError, KeyError, TypeError) as e:
            print(f"Warning: skipping {filepath}: {e}", file=sys.stderr)
    return user_cells


CELLS.update(load_user_cells())

# ── Dynamic style pool integration for imported cells ─────────────────────────

# Maps cell tags to style pools where those cells should participate
TAG_TO_POOLS = {
    "blast": ["blast", "screamo", "emoviolence", "black_metal"],
    "driving": ["posthardcore", "screamo", "euro_screamo", "noise_rock"],
    "groovy": ["posthardcore", "math"],
    "angular": ["math", "faraquet"],
    "math": ["math", "faraquet"],
    "halftime": ["screamo", "emoviolence"],
    "breakdown": ["screamo", "emoviolence"],
    "sparse": ["black_metal", "euro_screamo", "noise_rock"],
    "atmospheric": ["black_metal", "euro_screamo", "noise_rock"],
    "intense": ["screamo", "black_metal"],
    "heavy": ["screamo", "emoviolence", "noise_rock"],
    "fill": [],  # fills are found by role, not pool
    "odd_meter": ["posthardcore", "math", "noise_rock"],
    "motorik": ["sonic_youth", "post_punk", "preoccupations", "dry_cleaning"],
    "post_punk": ["post_punk", "wipers", "preoccupations", "dry_cleaning", "shame"],
    "slint": ["slint", "noise_rock"],
    "athletic": ["drive_like_jehu", "q_and_not_u", "atdi", "blood_brothers", "posthardcore"],
    "krautrock": ["sonic_youth"],
    "sonic_youth": ["sonic_youth"],
    "drive_like_jehu": ["drive_like_jehu"],
    "atdi": ["atdi", "blood_brothers"],
    "blood_brothers": ["blood_brothers", "atdi"],
    "unwound": ["unwound", "noise_rock"],
    "city_of_caterpillar": ["city_of_caterpillar", "screamo", "emoviolence"],
    "dynamic": ["slint", "unwound"],
    "climax": ["slint", "unwound"],
}

STYLE_POOLS = {
    "blast": ["blast_traditional", "emoviolence_blast_crash", "blast_7_8", "blast_5_4", "blast_3_4",
              "prob_blast_4_4"],
    "dbeat": ["dbeat_standard", "dbeat_7_8", "prob_dbeat_4_4"],
    "shellac": ["shellac_floor_tom_drive", "shellac_7_8", "shellac_5_4", "shellac_3_4", "shellac_6_8",
                "prob_shellac_4_4"],
    "fugazi": ["fugazi_driving_chorus", "driving_7_8", "driving_5_4", "driving_3_4", "driving_6_8", "driving_6_4"],
    "faraquet": ["faraquet_displaced_4_4", "faraquet_7_8", "faraquet_5_4",
                 "prob_faraquet_4_4", "prob_faraquet_7_8"],
    "raein": ["raein_melodic_drive"],
    "posthardcore": ["fugazi_driving_chorus", "faraquet_displaced_4_4", "raein_melodic_drive",
                     "driving_7_8", "driving_5_4", "driving_3_4", "driving_6_8", "driving_6_4",
                     "faraquet_7_8", "faraquet_5_4", "waltz_punk",
                     "prob_posthardcore_4_4",
                     "athletic_angular", "postpunk_busy", "slint_explosion", "prob_angular_athletic_4_4"],
    "noise_rock": ["shellac_floor_tom_drive", "shellac_7_8", "shellac_5_4", "shellac_3_4", "shellac_6_8",
                   "prob_shellac_4_4", "unwound_dynamics", "prob_postpunk_4_4"],
    "screamo": ["emoviolence_blast_crash", "emoviolence_angular_breakdown", "blast_traditional", "city_of_caterpillar_build"],
    "emoviolence": ["emoviolence_blast_crash", "emoviolence_angular_breakdown", "blast_traditional"],
    "math": ["faraquet_displaced_4_4", "faraquet_7_8", "faraquet_5_4",
             "prob_faraquet_4_4", "prob_faraquet_7_8"],
    "euro_screamo": ["daitro_tremolo_drive", "daitro_quiet_build", "daitro_blast_release", "raein_melodic_drive",
                     "prob_euro_screamo_4_4", "city_of_caterpillar_build"],
    "daitro": ["daitro_quiet_build", "daitro_tremolo_drive", "daitro_blast_release"],
    "liturgy": ["liturgy_burst_beat"],
    "black_metal": ["liturgy_burst_beat", "blackmetal_atmospheric", "deafheaven_build_to_blast", "atmospheric_7_8"],
    "deafheaven": ["deafheaven_build_to_blast", "blackmetal_atmospheric"],
    # Phase 3: Style palette expansion
    "sonic_youth": ["motorik_pulse", "motorik_build", "prob_postpunk_4_4"],
    "slint": ["motorik_build", "slint_explosion", "unwound_dynamics", "prob_slint_4_4"],
    "post_punk": ["postpunk_machine", "postpunk_busy", "motorik_pulse", "prob_postpunk_4_4"],
    "wipers": ["postpunk_machine", "prob_postpunk_4_4"],
    "preoccupations": ["postpunk_machine", "motorik_pulse", "prob_postpunk_4_4"],
    "dry_cleaning": ["postpunk_machine", "motorik_pulse", "prob_postpunk_4_4"],
    "shame": ["postpunk_machine", "postpunk_busy", "prob_postpunk_4_4"],
    "drive_like_jehu": ["athletic_angular", "postpunk_busy", "slint_explosion", "prob_angular_athletic_4_4"],
    "q_and_not_u": ["athletic_angular", "postpunk_busy", "prob_angular_athletic_4_4"],
    "atdi": ["postpunk_busy", "athletic_angular", "prob_angular_athletic_4_4"],
    "blood_brothers": ["postpunk_busy", "athletic_angular", "prob_angular_athletic_4_4"],
    "unwound": ["unwound_dynamics", "postpunk_machine", "slint_explosion"],
    "city_of_caterpillar": ["city_of_caterpillar_build", "emoviolence_blast_crash", "emoviolence_angular_breakdown"],
    "oxbow": ["unwound_dynamics", "shellac_floor_tom_drive", "slint_explosion"],
    "postrock": ["postrock_6_4", "blackmetal_atmospheric", "city_of_caterpillar_build",
                 "motorik_build", "slint_explosion"],
}

def _integrate_user_cells_into_pools():
    """Scan imported cells and add them to matching STYLE_POOLS based on tags."""
    for name, cell in CELLS.items():
        if cell.get("source") != "imported":
            continue
        cell_tags = set(cell.get("tags", []))
        pools_added = set()
        for tag in cell_tags:
            for pool_name in TAG_TO_POOLS.get(tag, []):
                if pool_name in STYLE_POOLS and name not in STYLE_POOLS[pool_name]:
                    STYLE_POOLS[pool_name].append(name)
                    pools_added.add(pool_name)
        if pools_added:
            cell["_pools"] = sorted(pools_added)

_integrate_user_cells_into_pools()

# Backward compat
STYLE_MAP = {k: v[0] for k, v in STYLE_POOLS.items()}

SECTION_PREFERENCES = {
    "intro": ["build", "sparse", "atmospheric", "quiet"],
    "build": ["build", "crescendo", "atmospheric"],
    "verse": ["driving", "groovy", "melodic"],
    "chorus": ["driving", "intense", "accent"],
    "drive": ["driving", "intense", "tremolo"],
    "blast": ["blast", "intense", "extreme"],
    "breakdown": ["breakdown", "halftime", "heavy", "slow"],
    "atmospheric": ["atmospheric", "sparse", "quiet"],
    "silence": [],
    "fill": ["fill"],
    "outro": ["sparse", "atmospheric", "quiet"],
}


def _suggest_match(name, options, n=3):
    """Return close matches for a misspelled name."""
    import difflib
    return difflib.get_close_matches(name, options, n=n, cutoff=0.5)


def get_cell(name):
    if name not in CELLS:
        available = sorted(CELLS.keys())
        suggestions = _suggest_match(name, available)
        hint = f" Did you mean: {', '.join(suggestions)}?" if suggestions else ""
        raise KeyError(
            f"Unknown cell: '{name}'.{hint}\n"
            f"Run 'python drumgen.py --list-cells' to see all available cells."
        )
    return CELLS[name]


def get_pool(style):
    """Return list of cell dicts for a style pool."""
    style_lower = style.lower()
    if style_lower not in STYLE_POOLS:
        available = sorted(STYLE_POOLS.keys())
        suggestions = _suggest_match(style_lower, available)
        hint = f" Did you mean: {', '.join(suggestions)}?" if suggestions else ""
        raise KeyError(
            f"Unknown style: '{style}'.{hint}\n"
            f"Available styles: {', '.join(available)}"
        )
    return [CELLS[name] for name in STYLE_POOLS[style_lower]]


def get_cell_for_section(pool_cells, section_type, requested_time_sig=None, rng=None):
    """Pick best cell from pool for a section type. Returns None for silence.

    Scoring: tags earlier in the preference list score higher (first pref = highest weight).
    Built-in cells get a +1 scoring bonus so they're preferred when equally matched.
    If requested_time_sig is given, prefer cells matching that time signature.
    If rng is provided, ties are broken randomly; otherwise the first match is used.
    """
    section_lower = section_type.lower()
    if section_lower == "silence":
        return None

    # Filter by time signature if requested
    if requested_time_sig:
        ts_match = [c for c in pool_cells if tuple(c["time_sig"]) == tuple(requested_time_sig)]
        if ts_match:
            pool_cells = ts_match

    prefs = SECTION_PREFERENCES.get(section_lower, [])
    if prefs and pool_cells:
        n = len(prefs)
        # Earlier prefs score higher: first pref = n points, last = 1
        pref_weights = {tag: n - i for i, tag in enumerate(prefs)}
        scored = []
        for cell in pool_cells:
            score = sum(pref_weights.get(tag, 0) for tag in cell["tags"])
            # Built-in cells get a tiebreaker bonus
            if cell.get("source") != "imported":
                score += 1
            scored.append((score, cell))
        best_score = max(s for s, _ in scored)
        if best_score > 0:
            best_cells = [cell for score, cell in scored if score == best_score]
            if rng and len(best_cells) > 1:
                return rng.choice(best_cells)
            return best_cells[0]
    # Fallback: first cell in pool
    return pool_cells[0] if pool_cells else None


def get_transition_cells():
    """Return all cells with role='transition'."""
    return [c for c in CELLS.values() if c["role"] == "transition"]


def get_cells_by_style(style):
    style_lower = style.lower()
    return [c for c in CELLS.values() if style_lower in c["tags"]]


def get_fill_cells():
    return [c for c in CELLS.values() if c["role"] == "fill"]


def list_cells(style_filter=None):
    cells = CELLS.values()
    if style_filter:
        style_lower = style_filter.lower()
        cells = [c for c in cells if style_lower in c["tags"]]
    return list(cells)
