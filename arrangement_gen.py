"""Generated song forms: a shape plus a form seed becomes an arrangement string.

A form is the same `N:section[@n/d]` string Song Mode and songs.txt use, so
everything downstream (assemble_arrangement, the plugin's validator, SAVE .MID,
notation) already plays it. Only the skeleton is generated; the notes still
come from the style's authored cells, chosen per section as in Song Mode.

Design note: docs/generated-arrangements.md. In short:

- Shapes are DATA (FORMS below), abstracted from the shipped songs and the
  records they cite. Each is a list of slots; a slot names the section types
  it may become, the bar counts it may take, and an optional chance. A
  `repeat` slot plays its `body` a rolled number of times.
- Every choice comes from SplitMix64, a ten-line integer hash, never from an
  engine RNG, so the Rust plugin can produce the same form for the same shape
  and seed (Mersenne Twister and ChaCha8 cannot agree, a hash can).
- Structure and meter turns use two separate dice streams: the structure of
  "arc 42" is the same in every style, only the turns depend on which meters
  the style owns.
"""

MASK = (1 << 64) - 1
GOLDEN = 0x9E3779B97F4A7C15

# Sections the form treats as a landing point for a fill (the into_<next>
# fill tags aim at these).
INTENSE = ("chorus", "blast", "drive", "breakdown")
# Sections a meter turn may land on. The shipped forms turn verses and drives;
# a turned blast or build would lose its section's cells to whatever the style
# owns in that meter.
TURNABLE = ("verse", "drive")
MIN_BARS, MAX_BARS = 12, 40
FORM_SEED_MAX = 9999   # small numbers are memorable: "ARC 42"

# label: stepper text (<= 10 chars, like the SONG labels).
# tag: short name for the status line and a songs.txt line ("ARC 42").
# turns / turn_chance: at most `turns` meter turns, each eligible section
#   rolling `turn_chance` per mille. Only when the meter is Auto.
# max_silences: silence is a spice (PROJECT.md lesson 2); Stop/Go is the one
#   shape where the gaps are the point.
# Slot: types (picked uniformly, never the type just placed when another is
#   offered), bars (picked uniformly), chance (per mille, default 1000).
FORMS = {
    "verse_chorus": {
        "label": "Verse/Chor", "tag": "VC", "turns": 0, "turn_chance": 0, "max_silences": 2,
        "about": "intro, two verse/chorus rounds, sometimes a bridge and a last chorus, outro",
        "slots": [
            {"types": ["intro"], "bars": [1, 2, 4]},
            {"types": ["verse"], "bars": [4, 8]},
            {"types": ["fill"], "bars": [1], "chance": 350},
            {"types": ["chorus"], "bars": [2, 4]},
            {"types": ["verse"], "bars": [4, 8]},
            {"types": ["fill"], "bars": [1], "chance": 500},
            {"types": ["chorus"], "bars": [4, 8]},
            {"repeat": [1, 1], "chance": 400, "body": [
                {"types": ["breakdown", "atmospheric"], "bars": [2, 4]},
                {"types": ["fill"], "bars": [1], "chance": 500},
                {"types": ["chorus"], "bars": [2, 4]},
            ]},
            {"types": ["outro"], "bars": [1, 2]},
        ],
    },
    "arc": {
        "label": "Arc", "tag": "ARC", "turns": 1, "turn_chance": 500, "max_silences": 2,
        "about": "one long rise: an 8-bar build erupting into the peak, then the landing",
        "slots": [
            {"types": ["intro", "atmospheric"], "bars": [2, 4]},
            {"types": ["verse"], "bars": [4, 8], "chance": 400},
            {"types": ["build"], "bars": [8]},
            {"types": ["fill"], "bars": [1], "chance": 600},
            {"types": ["blast", "drive"], "bars": [4, 8]},
            {"types": ["breakdown", "atmospheric", "outro"], "bars": [1, 2, 4]},
        ],
    },
    "stop_go": {
        "label": "Stop/Go", "tag": "STOP", "turns": 0, "turn_chance": 0, "max_silences": 3,
        "about": "short bursts punched apart by dead air, then the weight lands",
        "slots": [
            {"types": ["blast", "chorus"], "bars": [2, 4]},
            {"repeat": [3, 4], "body": [
                {"types": ["silence"], "bars": [1], "chance": 750},
                {"types": ["blast", "chorus"], "bars": [1, 2, 3]},
            ]},
            {"types": ["breakdown"], "bars": [4]},
            {"types": ["chorus", "blast"], "bars": [2, 3, 4], "chance": 500},
        ],
    },
    "quiet_loud": {
        "label": "Quiet/Loud", "tag": "QL", "turns": 0, "turn_chance": 0, "max_silences": 2,
        "about": "fragile, erupts, holds its breath, fragile again, then the full wall",
        "slots": [
            {"types": ["atmospheric"], "bars": [2, 4]},
            {"types": ["drive", "chorus"], "bars": [2, 4]},
            {"types": ["silence"], "bars": [1], "chance": 700},
            {"types": ["atmospheric"], "bars": [2, 4]},
            {"types": ["fill"], "bars": [1], "chance": 400},
            {"types": ["blast"], "bars": [4, 8]},
            {"types": ["outro", "atmospheric"], "bars": [2, 4]},
        ],
    },
    "maze": {
        "label": "Maze", "tag": "MAZE", "turns": 2, "turn_chance": 350, "max_silences": 2,
        "about": "a run of short, odd-length sections with fills between and meter turns",
        "slots": [
            {"types": ["intro", "atmospheric"], "bars": [2]},
            {"repeat": [4, 7], "body": [
                {"types": ["fill"], "bars": [1], "chance": 450},
                {"types": ["verse", "blast", "build", "drive"], "bars": [2, 3, 4]},
            ]},
            {"types": ["breakdown", "outro"], "bars": [2]},
        ],
    },
    "slow_burn": {
        "label": "Slow Burn", "tag": "BURN", "turns": 0, "turn_chance": 0, "max_silences": 2,
        "about": "a long rise to a peak that never comes: it holds its breath and fades",
        "slots": [
            {"types": ["atmospheric", "intro"], "bars": [4, 8]},
            {"types": ["verse"], "bars": [4, 8], "chance": 600},
            {"types": ["build"], "bars": [8]},
            {"types": ["drive"], "bars": [4], "chance": 500},
            {"types": ["silence"], "bars": [1], "chance": 750},
            {"types": ["outro", "atmospheric"], "bars": [4, 8]},
        ],
    },
}

STRUCTURE_STREAM = 0
TURN_STREAM = 1


def _mix(z):
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
    return z ^ (z >> 31)


class Dice:
    """SplitMix64. Integer-only so the Rust port draws the identical stream."""

    def __init__(self, seed, stream):
        self.state = (seed ^ ((stream * GOLDEN) & MASK)) & MASK

    def draw(self):
        self.state = (self.state + GOLDEN) & MASK
        return _mix(self.state)

    def chance(self, per_mille):
        """Always draws, so a slot's chance never shifts the rest of the stream
        depending on its value."""
        return self.draw() % 1000 < per_mille

    def pick(self, options):
        return options[self.draw() % len(options)]

    def between(self, lo, hi):
        return lo + self.draw() % (hi - lo + 1)


def _place(slots, dice, out):
    """Realise slots into out: a list of [bars, section]."""
    for slot in slots:
        if not dice.chance(slot.get("chance", 1000)):
            continue
        if "repeat" in slot:
            lo, hi = slot["repeat"]
            for _ in range(dice.between(lo, hi)):
                _place(slot["body"], dice, out)
            continue
        types = slot.get("types", [])
        if not types or not slot.get("bars"):
            continue
        prev = out[-1][1] if out else None
        fresh = [t for t in types if t != prev] or types
        section = dice.pick(fresh)
        out.append([dice.pick(slot["bars"]), section])


def _tidy(sections, max_silences):
    """The rules every shape obeys, applied without dice (so the order of
    draws above never depends on them)."""
    # Fills only directly before an intense section.
    out = []
    for i, (bars, sec) in enumerate(sections):
        nxt = sections[i + 1][1] if i + 1 < len(sections) else None
        if sec == "fill" and nxt not in INTENSE:
            continue
        out.append([bars, sec])
    # Silence: never first or last, never twice in a row, at most max_silences.
    kept, silences = [], 0
    for i, (bars, sec) in enumerate(out):
        if sec == "silence":
            last = i == len(out) - 1
            if not kept or last or kept[-1][1] == "silence" or silences >= max_silences:
                continue
            silences += 1
            bars = 1
        kept.append([bars, sec])
    # A dropped fill or silence can leave two of a kind side by side: one section.
    merged = []
    for bars, sec in kept:
        if merged and merged[-1][1] == sec and sec not in ("fill", "silence"):
            merged[-1][0] = min(32, merged[-1][0] + bars)
        else:
            merged.append([bars, sec])
    return _fit_length(merged)


def _fit_length(sections):
    """Guard for edited templates: keep the total inside MIN_BARS..MAX_BARS by
    halving the longest section, or doubling the longest playing one."""
    def total():
        return sum(b for b, _ in sections)
    while total() > MAX_BARS:
        i = max(range(len(sections)), key=lambda k: (sections[k][0], -k))
        if sections[i][0] == 1:
            break
        sections[i][0] //= 2
    while total() < MIN_BARS:
        playing = [k for k, (_, s) in enumerate(sections) if s not in ("fill", "silence")]
        if not playing:
            break
        i = max(playing, key=lambda k: (sections[k][0], -k))
        grown = min(32, sections[i][0] * 2, sections[i][0] + MAX_BARS - total())
        if grown == sections[i][0]:
            break
        sections[i][0] = grown
    return sections


def style_meters(style):
    """Meters the style's BUILT-IN groove cells are written in, sorted.
    Imported cells are left out: the plugin never sees them, and the same
    form must come out of both engines."""
    from cell_library import CELLS, STYLE_POOLS
    pool = STYLE_POOLS.get(style, [])
    return sorted({tuple(CELLS[n]["time_sig"]) for n in pool
                   if n in CELLS and CELLS[n].get("source") != "imported"
                   and CELLS[n].get("role", "groove") != "fill"},
                  key=lambda ts: (ts[1], ts[0]))


def generate_sections(shape, form_seed, meters=(), home=(4, 4), meter_auto=True):
    """[(bars, section, (num, den) or None)], None meaning the home meter."""
    if shape not in FORMS:
        raise ValueError(f"Unknown form '{shape}'. Forms: {', '.join(FORMS)}")
    if not 0 <= form_seed <= MASK:
        raise ValueError(f"Form seed must be 0 or more, got {form_seed}")
    form = FORMS[shape]
    placed = []
    _place(form["slots"], Dice(form_seed, STRUCTURE_STREAM), placed)
    sections = [(b, s, None) for b, s in _tidy(placed, form["max_silences"])]

    candidates = [tuple(m) for m in meters if tuple(m) != tuple(home)]
    if not meter_auto or not candidates or form["turns"] == 0:
        return sections
    dice, turns = Dice(form_seed, TURN_STREAM), 0
    turned = []
    for bars, sec, ts in sections:
        if sec in TURNABLE and turns < form["turns"] and dice.chance(form["turn_chance"]):
            ts = dice.pick(candidates)
            turns += 1
        turned.append((bars, sec, ts))
    return turned


def to_arrangement(sections):
    return " ".join(f"{b}:{s}" + (f"@{ts[0]}/{ts[1]}" if ts else "") for b, s, ts in sections)


def generate_form(shape, form_seed, style=None, home=(4, 4), meter_auto=True):
    """The arrangement string for (shape, form_seed). With a style and the
    meter on Auto, verses and drives may turn into the style's other meters."""
    meters = style_meters(style) if style else ()
    return to_arrangement(generate_sections(shape, form_seed, meters, home, meter_auto))


def form_name(shape, form_seed):
    """'ARC 42': what the status line shows and a songs.txt line is called
    (the plugin keeps the first 10 characters of a song name)."""
    return f"{FORMS[shape]['tag']} {form_seed}"
