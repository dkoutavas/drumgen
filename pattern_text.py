"""Text step-grid of a generated pattern, for the Streamlit GUI's preview.

Pure (no Streamlit import), so pytest covers it. One column per sixteenth,
whatever the meter: 4 per beat in /4, 2 per beat in /8, 8 per beat in /2.
Bar boundaries come from the real meter map, so an arrangement that turns
from 4/4 to 7/8 previews each bar in its own meter.
"""
from bisect import bisect_right

from midi_engine import DEFAULT_PPQ, calculate_bar_start_ticks, get_time_sig_for_bar

_SUBS = {4: ["e", "+", "a"], 2: ["+"]}


def _beat_labels(num, den):
    per_beat = max(1, 16 // den)
    rest = _SUBS.get(per_beat, ["."] * (per_beat - 1))
    labels = []
    for beat in range(1, num + 1):
        labels.append(str(beat))
        labels.extend(rest)
    return labels


def build_pattern_preview(events, time_signatures, max_bars=4, ppq=DEFAULT_PPQ):
    """Render up to `max_bars` bars of (tick, instrument, velocity) events.

    X = accent (>= 100), x = normal (>= 60), o = ghost, . = silent.
    """
    sixteenth = ppq // 4
    starts = [calculate_bar_start_ticks(b, time_signatures, ppq) for b in range(1, max_bars + 2)]

    bars_data = {}
    for tick, inst, vel in events:
        bar = bisect_right(starts, tick)  # 1-based: starts[0] is bar 1's start
        if bar < 1 or bar > max_bars:
            continue
        num, den = get_time_sig_for_bar(bar, time_signatures)
        n_slots = num * max(1, 16 // den)
        slot = min(round((tick - starts[bar - 1]) / sixteenth), n_slots - 1)
        cells = bars_data.setdefault(bar, {}).setdefault(inst, {})
        cells[slot] = max(vel, cells.get(slot, 0))

    if not bars_data:
        return "No events to display."

    instruments = sorted({inst for bar in bars_data.values() for inst in bar})
    lines = []
    for bar in sorted(bars_data):
        num, den = get_time_sig_for_bar(bar, time_signatures)
        labels = _beat_labels(num, den)
        meter = "" if (num, den) == (4, 4) else f" ({num}/{den})"
        lines.append(f"Bar {bar}{meter}:  " + " ".join(f"{l:>2}" for l in labels))
        for inst in instruments:
            cells = bars_data[bar].get(inst, {})
            row = []
            for s in range(len(labels)):
                v = cells.get(s)
                row.append(" ." if v is None else " X" if v >= 100 else " x" if v >= 60 else " o")
            lines.append(inst[:12].ljust(12) + "".join(row))
        lines.append("")
    return "\n".join(lines)
