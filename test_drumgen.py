"""Comprehensive test suite for drumgen."""

import os
import tempfile

import mido
import pytest

from assembler import (
    assemble, assemble_arrangement, assemble_layered,
    parse_arrangement, realize_probability_grid, extract_layer,
    _normalize_grid, _validate_physical_constraints, _resolve_layer_conflicts,
    _consolidate_time_signatures, LAYER_GROUPS,
)
from cell_library import (
    CELLS, STYLE_POOLS, SECTION_PREFERENCES,
    get_cell, get_pool, get_cell_for_section, get_fill_cells,
)
from humanizer import Humanizer
from midi_engine import position_to_ticks, calculate_bar_start_ticks, write_midi, DEFAULT_PPQ


# ── Helpers ───────────────────────────────────────────────────────────────────

BUILTIN_CELLS = {name: cell for name, cell in CELLS.items()
                 if cell.get("source") != "imported"}

VALID_INSTRUMENTS = {
    "kick", "snare", "snare_ghost", "snare_rim",
    "hihat_closed", "hihat_open", "hihat_pedal",
    "ride", "ride_bell",
    "crash_1", "crash_2", "china", "splash",
    "tom_high", "tom_mid", "tom_low", "tom_floor",
}

VALID_VELOCITY_LEVELS = {"ghost", "soft", "normal", "accent"}

VALID_DENOMINATORS = {2, 4, 8, 16}

REQUIRED_STYLES = [
    "blast", "dbeat", "shellac", "fugazi", "faraquet", "raein",
    "posthardcore", "noise_rock", "screamo", "emoviolence", "math",
    "euro_screamo", "daitro", "liturgy", "black_metal", "deafheaven",
    # Phase 3: Style palette expansion
    "sonic_youth", "slint", "post_punk", "wipers", "preoccupations",
    "dry_cleaning", "shame", "drive_like_jehu", "q_and_not_u", "atdi",
    "blood_brothers", "unwound", "city_of_caterpillar", "oxbow",
]


def _generate_and_write(style, tempo, bars, time_sig_str, **kwargs):
    """Generate a pattern and write to a temp MIDI file. Returns (MidiFile, note_on_count, path)."""
    with tempfile.NamedTemporaryFile(suffix=".mid", delete=False) as f:
        tmp_path = f.name
    try:
        result = assemble(
            style=style, tempo=tempo, bars=bars, time_sig=time_sig_str,
            seed=kwargs.get("seed", 42),
            humanize=kwargs.get("humanize", 0.5),
            swing=kwargs.get("swing", 0.0),
            vary=kwargs.get("vary", 0.0),
        )
        write_midi(
            events=result["events"], tempo=result["tempo"],
            time_signatures=result["time_signatures"],
            kit_mapping_path="ugritone", output_path=tmp_path,
        )
        mid = mido.MidiFile(tmp_path)
        note_ons = sum(
            1 for t in mid.tracks for m in t
            if m.type == "note_on" and m.velocity > 0
        )
        return mid, note_ons, tmp_path
    except Exception:
        os.unlink(tmp_path)
        raise


def _generate_arrangement_and_write(style, arrangement_str, tempo, time_sig_str, **kwargs):
    """Generate an arrangement and write to a temp MIDI file."""
    with tempfile.NamedTemporaryFile(suffix=".mid", delete=False) as f:
        tmp_path = f.name
    try:
        result = assemble_arrangement(
            style=style, arrangement_str=arrangement_str,
            tempo=tempo, time_sig=time_sig_str,
            seed=kwargs.get("seed", 42),
            humanize=kwargs.get("humanize", 0.5),
            swing=kwargs.get("swing", 0.0),
            vary=kwargs.get("vary", 0.0),
        )
        write_midi(
            events=result["events"], tempo=result["tempo"],
            time_signatures=result["time_signatures"],
            kit_mapping_path="ugritone", output_path=tmp_path,
        )
        mid = mido.MidiFile(tmp_path)
        note_ons = sum(
            1 for t in mid.tracks for m in t
            if m.type == "note_on" and m.velocity > 0
        )
        return mid, note_ons, tmp_path, result
    except Exception:
        os.unlink(tmp_path)
        raise


# ── TestCellLibrary ───────────────────────────────────────────────────────────

class TestCellLibrary:
    """Cell integrity tests (built-in cells only, skips imported)."""

    def test_all_builtin_cells_registered(self):
        assert len(BUILTIN_CELLS) >= 55  # 44 original + 11 new (phase 3)

    def test_required_fields(self):
        required_base = {"name", "tags", "time_sig", "num_bars", "humanize"}
        for name, cell in BUILTIN_CELLS.items():
            missing = required_base - set(cell.keys())
            assert not missing, f"Cell '{name}' missing fields: {missing}"
            if cell.get("type") == "probability":
                assert "grid" in cell, f"Probability cell '{name}' missing 'grid'"
            else:
                assert "hits" in cell, f"Cell '{name}' missing 'hits'"

    def test_valid_time_sig(self):
        for name, cell in BUILTIN_CELLS.items():
            ts = cell["time_sig"]
            assert isinstance(ts, tuple) and len(ts) == 2, \
                f"Cell '{name}' time_sig must be (num, den) tuple, got {ts}"
            assert ts[0] > 0, f"Cell '{name}' numerator must be positive"
            assert ts[1] in VALID_DENOMINATORS, \
                f"Cell '{name}' denominator {ts[1]} not in {VALID_DENOMINATORS}"

    def test_valid_tags(self):
        for name, cell in BUILTIN_CELLS.items():
            tags = cell["tags"]
            assert isinstance(tags, list) and len(tags) > 0, \
                f"Cell '{name}' must have non-empty tags list"
            for tag in tags:
                assert isinstance(tag, str), f"Cell '{name}' has non-string tag: {tag}"

    def test_humanize_in_range(self):
        for name, cell in BUILTIN_CELLS.items():
            h = cell["humanize"]
            assert 0.0 <= h <= 1.0, f"Cell '{name}' humanize={h} out of [0,1]"

    def test_no_empty_hits(self):
        for name, cell in BUILTIN_CELLS.items():
            if cell.get("type") == "probability":
                assert len(cell["grid"]) >= 1, f"Cell '{name}' has no grid entries"
            else:
                assert len(cell["hits"]) >= 1, f"Cell '{name}' has no hits"

    def test_hit_tuple_format(self):
        for name, cell in BUILTIN_CELLS.items():
            if cell.get("type") == "probability":
                continue  # grid entries tested in TestProbabilityGrids
            for i, hit in enumerate(cell["hits"]):
                assert len(hit) in (4, 5), \
                    f"Cell '{name}' hit[{i}] must be 4- or 5-tuple, got {len(hit)}"
                if len(hit) == 4:
                    beat, sub, inst, vel = hit
                else:
                    bar, beat, sub, inst, vel = hit
                    assert isinstance(bar, int) and bar >= 1, \
                        f"Cell '{name}' hit[{i}] bar must be int >= 1, got {bar}"
                assert isinstance(beat, int) and beat >= 1, \
                    f"Cell '{name}' hit[{i}] beat must be int >= 1, got {beat}"
                assert isinstance(sub, float), \
                    f"Cell '{name}' hit[{i}] sub must be float, got {type(sub)}"
                assert inst in VALID_INSTRUMENTS, \
                    f"Cell '{name}' hit[{i}] unknown instrument '{inst}'"
                assert vel in VALID_VELOCITY_LEVELS, \
                    f"Cell '{name}' hit[{i}] unknown velocity level '{vel}'"

    def test_beat_within_time_sig(self):
        for name, cell in BUILTIN_CELLS.items():
            if cell.get("type") == "probability":
                continue
            num = cell["time_sig"][0]
            for i, hit in enumerate(cell["hits"]):
                beat = hit[0] if len(hit) == 4 else hit[1]
                assert 1 <= beat <= num, \
                    f"Cell '{name}' hit[{i}] beat={beat} exceeds time_sig numerator={num}"

    def test_bar_within_num_bars(self):
        for name, cell in BUILTIN_CELLS.items():
            if cell.get("type") == "probability":
                continue
            for i, hit in enumerate(cell["hits"]):
                if len(hit) == 5:
                    bar = hit[0]
                    assert 1 <= bar <= cell["num_bars"], \
                        f"Cell '{name}' hit[{i}] bar={bar} outside 1..{cell['num_bars']}"

    def test_shellac_no_ghost_notes(self):
        """Shellac cells should use precise/hard hits, no ghost notes."""
        shellac_cells = [c for c in BUILTIN_CELLS.values()
                         if ("shellac" in c["tags"] or c["name"].startswith("shellac"))
                         and c.get("type") != "probability"]
        assert len(shellac_cells) > 0, "No shellac cells found"
        for cell in shellac_cells:
            for i, hit in enumerate(cell["hits"]):
                vel = hit[-1]
                assert vel != "ghost", \
                    f"Shellac cell '{cell['name']}' hit[{i}] has ghost velocity"

    def test_blast_density(self):
        """Blast cells should have kick+snare on every sixteenth subdivision."""
        blast_cells = [c for c in BUILTIN_CELLS.values()
                       if c["name"].startswith("blast_")]
        assert len(blast_cells) > 0, "No blast cells found"
        for cell in blast_cells:
            num = cell["time_sig"][0]
            subdivisions = set()
            for hit in cell["hits"]:
                if len(hit) == 4:
                    beat, sub, inst, vel = hit
                else:
                    bar, beat, sub, inst, vel = hit
                if inst in ("kick", "snare"):
                    subdivisions.add((beat, sub))
            # Every beat should have at least 2 kick/snare subdivisions
            for beat in range(1, num + 1):
                beat_subs = [(b, s) for b, s in subdivisions if b == beat]
                assert len(beat_subs) >= 2, \
                    f"Blast cell '{cell['name']}' beat {beat} has only {len(beat_subs)} K/S hits"


# ── TestTimeSignatures ────────────────────────────────────────────────────────

class TestTimeSignatures:
    """position_to_ticks math for various time signatures."""

    def _ts_list(self, num, den):
        return [{"bar_start": 1, "bar_end": 100, "numerator": num, "denominator": den}]

    def test_4_4_bar_length(self):
        ts = self._ts_list(4, 4)
        bar1_start = position_to_ticks(1, 1, 0.0, ts)
        bar2_start = position_to_ticks(2, 1, 0.0, ts)
        assert bar2_start - bar1_start == 1920

    def test_7_8_bar_length(self):
        ts = self._ts_list(7, 8)
        bar1_start = position_to_ticks(1, 1, 0.0, ts)
        bar2_start = position_to_ticks(2, 1, 0.0, ts)
        assert bar2_start - bar1_start == 1680

    def test_5_4_bar_length(self):
        ts = self._ts_list(5, 4)
        bar1_start = position_to_ticks(1, 1, 0.0, ts)
        bar2_start = position_to_ticks(2, 1, 0.0, ts)
        assert bar2_start - bar1_start == 2400

    def test_3_4_bar_length(self):
        ts = self._ts_list(3, 4)
        bar1_start = position_to_ticks(1, 1, 0.0, ts)
        bar2_start = position_to_ticks(2, 1, 0.0, ts)
        assert bar2_start - bar1_start == 1440

    def test_6_8_bar_length(self):
        ts = self._ts_list(6, 8)
        bar1_start = position_to_ticks(1, 1, 0.0, ts)
        bar2_start = position_to_ticks(2, 1, 0.0, ts)
        assert bar2_start - bar1_start == 1440

    def test_beat_position_within_bar(self):
        ts = self._ts_list(4, 4)
        ppq = DEFAULT_PPQ
        beat_ticks = ppq  # 4/4 → beat_ticks = 480
        for beat in range(1, 5):
            tick = position_to_ticks(1, beat, 0.0, ts)
            assert tick == (beat - 1) * beat_ticks

    def test_bar_2_starts_after_bar_1(self):
        ts = self._ts_list(4, 4)
        bar1 = position_to_ticks(1, 1, 0.0, ts)
        bar2 = position_to_ticks(2, 1, 0.0, ts)
        assert bar2 > bar1

    def test_sub_position_offset(self):
        """Sub=0.5 (eighth note) should offset by half a beat."""
        ts = self._ts_list(4, 4)
        on_beat = position_to_ticks(1, 1, 0.0, ts)
        eighth = position_to_ticks(1, 1, 0.5, ts)
        assert eighth - on_beat == DEFAULT_PPQ // 2

    def test_sub_sixteenth(self):
        """Sub=0.25 (sixteenth) should offset by quarter of a beat."""
        ts = self._ts_list(4, 4)
        on_beat = position_to_ticks(1, 1, 0.0, ts)
        sixteenth = position_to_ticks(1, 1, 0.25, ts)
        assert sixteenth - on_beat == DEFAULT_PPQ // 4


# ── TestStylePools ────────────────────────────────────────────────────────────

class TestStylePools:
    """Style pool integrity and selection logic."""

    def test_all_required_styles_have_pools(self):
        for style in REQUIRED_STYLES:
            assert style in STYLE_POOLS, f"Missing style pool: '{style}'"

    def test_all_pool_cells_exist(self):
        for style, cell_names in STYLE_POOLS.items():
            for name in cell_names:
                assert name in CELLS, \
                    f"Style pool '{style}' references non-existent cell '{name}'"

    def test_time_sig_aware_selection(self):
        """When a style has a matching time sig cell, it should be selected."""
        pool = get_pool("shellac")
        cell = get_cell_for_section(pool, "verse", requested_time_sig=(7, 8))
        assert tuple(cell["time_sig"]) == (7, 8)

    def test_shellac_5_4_selection(self):
        pool = get_pool("shellac")
        cell = get_cell_for_section(pool, "verse", requested_time_sig=(5, 4))
        assert tuple(cell["time_sig"]) == (5, 4)

    def test_fallback_when_no_match(self):
        """Raein has no 7/8 cell — should fall back to a 4/4 cell."""
        pool = get_pool("raein")
        cell = get_cell_for_section(pool, "verse", requested_time_sig=(7, 8))
        assert cell is not None  # falls back to 4/4

    def test_posthardcore_6_8_selects_driving_6_8(self):
        """The bug fix: posthardcore pool should select driving_6_8 for 6/8."""
        pool = get_pool("posthardcore")
        ts_match = [c for c in pool if tuple(c["time_sig"]) == (6, 8)]
        assert len(ts_match) >= 1, "No 6/8 cell in posthardcore pool"
        assert any(c["name"] == "driving_6_8" for c in ts_match)

    def test_fugazi_6_8_available(self):
        """Fugazi pool should also have driving_6_8."""
        pool = get_pool("fugazi")
        ts_match = [c for c in pool if tuple(c["time_sig"]) == (6, 8)]
        assert len(ts_match) >= 1, "No 6/8 cell in fugazi pool"

    def test_silence_section_returns_none(self):
        pool = get_pool("shellac")
        cell = get_cell_for_section(pool, "silence")
        assert cell is None

    def test_no_empty_pools(self):
        for style, cell_names in STYLE_POOLS.items():
            assert len(cell_names) > 0, f"Style pool '{style}' is empty"


# ── TestAssembler ─────────────────────────────────────────────────────────────

class TestAssembler:
    """Assembly and arrangement mode tests."""

    def test_assemble_returns_required_keys(self):
        result = assemble(style="shellac", tempo=120, bars=4, time_sig="4/4", seed=42)
        assert "events" in result
        assert "tempo" in result
        assert "time_signatures" in result
        assert "seed" in result

    def test_assemble_arrangement_returns_extra_keys(self):
        result = assemble_arrangement(
            style="shellac", arrangement_str="4:verse 4:chorus",
            tempo=120, time_sig="4/4", seed=42,
        )
        assert "total_bars" in result
        assert "section_summary" in result
        assert result["total_bars"] == 8

    def test_silence_section_no_events(self):
        """A pure silence arrangement should produce no note events."""
        result = assemble_arrangement(
            style="shellac", arrangement_str="4:silence",
            tempo=120, time_sig="4/4", seed=42,
        )
        assert len(result["events"]) == 0

    def test_seed_reproducibility(self):
        r1 = assemble(style="blast", tempo=180, bars=4, time_sig="4/4", seed=123)
        r2 = assemble(style="blast", tempo=180, bars=4, time_sig="4/4", seed=123)
        assert r1["events"] == r2["events"]

    def test_different_seeds_differ(self):
        r1 = assemble(style="blast", tempo=180, bars=4, time_sig="4/4", seed=100)
        r2 = assemble(style="blast", tempo=180, bars=4, time_sig="4/4", seed=200)
        # Events differ because humanizer is seeded differently
        assert r1["events"] != r2["events"]

    def test_vary_changes_output(self):
        """With vary > 0, repeated bars should differ from non-varied."""
        r_no_vary = assemble(style="shellac", tempo=120, bars=8, time_sig="4/4",
                             seed=42, vary=0.0)
        r_vary = assemble(style="shellac", tempo=120, bars=8, time_sig="4/4",
                          seed=42, vary=0.8)
        # vary introduces mutations on repeated bars, so event count or content should differ
        assert r_no_vary["events"] != r_vary["events"]

    def test_assemble_with_cell_name(self):
        result = assemble(cell_name="blast_traditional", tempo=200, bars=2,
                          time_sig="4/4", seed=42)
        assert len(result["events"]) > 0

    def test_unknown_style_raises(self):
        with pytest.raises(ValueError, match="Unknown style"):
            assemble(style="nonexistent_style", tempo=120, bars=4, time_sig="4/4")

    def test_6_8_assemble(self):
        """The fixed bug: posthardcore 6/8 should work without warnings."""
        result = assemble(style="posthardcore", tempo=130, bars=4,
                          time_sig="6/8", seed=42)
        assert len(result["events"]) > 0
        # Verify time sig is 6/8
        ts = result["time_signatures"][0]
        assert ts["numerator"] == 6 and ts["denominator"] == 8


# ── TestHumanizer ─────────────────────────────────────────────────────────────

class TestHumanizer:
    """Humanizer velocity and timing tests."""

    def test_velocity_in_midi_range(self):
        h = Humanizer(1.0, seed=42)
        for _ in range(500):
            for level in ("ghost", "soft", "normal", "accent"):
                vel = h.humanize_velocity(level, "snare")
                assert 1 <= vel <= 127

    def test_ghost_is_quiet(self):
        h = Humanizer(0.5, seed=42)
        vels = [h.humanize_velocity("ghost", "snare") for _ in range(200)]
        avg = sum(vels) / len(vels)
        assert avg < 60, f"Ghost average velocity {avg} too high"

    def test_accent_is_loud(self):
        h = Humanizer(0.5, seed=42)
        vels = [h.humanize_velocity("accent", "snare") for _ in range(200)]
        avg = sum(vels) / len(vels)
        assert avg > 95, f"Accent average velocity {avg} too low"

    def test_seed_reproducibility(self):
        h1 = Humanizer(0.5, seed=99)
        h2 = Humanizer(0.5, seed=99)
        v1 = [h1.humanize_velocity("normal", "kick") for _ in range(50)]
        v2 = [h2.humanize_velocity("normal", "kick") for _ in range(50)]
        assert v1 == v2

    def test_different_seeds_differ(self):
        h1 = Humanizer(0.5, seed=1)
        h2 = Humanizer(0.5, seed=2)
        v1 = [h1.humanize_velocity("normal", "kick") for _ in range(50)]
        v2 = [h2.humanize_velocity("normal", "kick") for _ in range(50)]
        assert v1 != v2

    def test_timing_non_negative(self):
        h = Humanizer(0.5, seed=42)
        for _ in range(200):
            tick = h.humanize_timing(1000, "snare", 120)
            assert tick >= 0

    def test_zero_humanize_minimal_variance(self):
        """With humanize_amount=0, velocity should be very close to center."""
        h = Humanizer(0.0, seed=42)
        vels = [h.humanize_velocity("normal", "kick") for _ in range(100)]
        # With 0 humanize, variance is scaled to max(3, 0) = 3
        center = (75 + 105) // 2  # 90
        for v in vels:
            assert abs(v - center) <= 5, f"Zero-humanize velocity {v} too far from center {center}"


# ── TestMidiEngine ────────────────────────────────────────────────────────────

class TestMidiEngine:
    """MIDI file output integrity tests."""

    def _make_midi(self, style="shellac", time_sig="4/4", bars=4, tempo=120):
        mid, note_ons, path = _generate_and_write(
            style, tempo, bars, time_sig, seed=42,
        )
        return mid, note_ons, path

    def test_valid_midi_file(self):
        mid, _, path = self._make_midi()
        try:
            assert len(mid.tracks) > 0
        finally:
            os.unlink(path)

    def test_all_notes_channel_9(self):
        mid, _, path = self._make_midi()
        try:
            for track in mid.tracks:
                for msg in track:
                    if msg.type in ("note_on", "note_off"):
                        assert msg.channel == 9, \
                            f"Expected channel 9, got {msg.channel}"
        finally:
            os.unlink(path)

    def test_no_negative_delta_times(self):
        mid, _, path = self._make_midi()
        try:
            for track in mid.tracks:
                for msg in track:
                    assert msg.time >= 0, f"Negative delta time: {msg.time}"
        finally:
            os.unlink(path)

    def test_velocity_range_in_midi(self):
        mid, _, path = self._make_midi()
        try:
            for track in mid.tracks:
                for msg in track:
                    if msg.type == "note_on":
                        assert 0 <= msg.velocity <= 127
        finally:
            os.unlink(path)

    def test_time_sig_metadata_4_4(self):
        mid, _, path = self._make_midi()
        try:
            ts_msgs = [m for t in mid.tracks for m in t
                       if m.type == "time_signature"]
            assert len(ts_msgs) >= 1
            assert ts_msgs[0].numerator == 4
            assert ts_msgs[0].denominator == 4
        finally:
            os.unlink(path)

    def test_time_sig_metadata_7_8(self):
        mid, _, path = self._make_midi(style="shellac", time_sig="7/8")
        try:
            ts_msgs = [m for t in mid.tracks for m in t
                       if m.type == "time_signature"]
            assert len(ts_msgs) >= 1
            assert ts_msgs[0].numerator == 7
            assert ts_msgs[0].denominator == 8
        finally:
            os.unlink(path)

    def test_time_sig_metadata_6_8(self):
        mid, _, path = self._make_midi(style="posthardcore", time_sig="6/8")
        try:
            ts_msgs = [m for t in mid.tracks for m in t
                       if m.type == "time_signature"]
            assert len(ts_msgs) >= 1
            assert ts_msgs[0].numerator == 6
            assert ts_msgs[0].denominator == 8
        finally:
            os.unlink(path)

    def test_has_note_events(self):
        _, note_ons, path = self._make_midi()
        try:
            assert note_ons > 0, "MIDI file has no note_on events"
        finally:
            os.unlink(path)


# ── TestEndToEnd ──────────────────────────────────────────────────────────────

class TestEndToEnd:
    """Full pipeline tests: style × time sig → valid MIDI."""

    # ── All 16 styles in 4/4 ──

    @pytest.mark.parametrize("style", REQUIRED_STYLES)
    def test_all_styles_4_4(self, style):
        mid, note_ons, path = _generate_and_write(style, 120, 4, "4/4")
        try:
            assert note_ons > 0, f"{style} 4/4 produced no notes"
        finally:
            os.unlink(path)

    # ── Odd meter combos ──

    @pytest.mark.parametrize("style", [
        "shellac", "faraquet", "blast", "dbeat", "posthardcore", "black_metal",
    ])
    def test_styles_7_8(self, style):
        mid, note_ons, path = _generate_and_write(style, 140, 4, "7/8")
        try:
            assert note_ons > 0, f"{style} 7/8 produced no notes"
        finally:
            os.unlink(path)

    @pytest.mark.parametrize("style", ["shellac", "faraquet", "blast", "posthardcore"])
    def test_styles_5_4(self, style):
        mid, note_ons, path = _generate_and_write(style, 130, 4, "5/4")
        try:
            assert note_ons > 0, f"{style} 5/4 produced no notes"
        finally:
            os.unlink(path)

    @pytest.mark.parametrize("style", ["shellac", "blast", "posthardcore"])
    def test_styles_3_4(self, style):
        mid, note_ons, path = _generate_and_write(style, 150, 4, "3/4")
        try:
            assert note_ons > 0, f"{style} 3/4 produced no notes"
        finally:
            os.unlink(path)

    @pytest.mark.parametrize("style", ["shellac", "posthardcore"])
    def test_styles_6_8(self, style):
        mid, note_ons, path = _generate_and_write(style, 130, 4, "6/8")
        try:
            assert note_ons > 0, f"{style} 6/8 produced no notes"
        finally:
            os.unlink(path)

    # ── Arrangement mode ──

    def test_arrangement_4_4(self):
        mid, note_ons, path, result = _generate_arrangement_and_write(
            "posthardcore", "4:verse 4:chorus 2:blast", 130, "4/4",
        )
        try:
            assert note_ons > 0
            assert result["total_bars"] == 10
        finally:
            os.unlink(path)

    def test_arrangement_7_8(self):
        mid, note_ons, path, result = _generate_arrangement_and_write(
            "shellac", "4:verse 4:drive", 140, "7/8",
        )
        try:
            assert note_ons > 0
            assert result["total_bars"] == 8
        finally:
            os.unlink(path)

    def test_arrangement_with_silence(self):
        mid, note_ons, path, result = _generate_arrangement_and_write(
            "shellac", "2:verse 2:silence 2:drive", 120, "4/4",
        )
        try:
            # Should have notes from verse and drive, but not silence
            assert note_ons > 0
            assert result["total_bars"] == 6
        finally:
            os.unlink(path)

    # ── Vary flag ──

    def test_vary_produces_different_output(self):
        _, n1, p1 = _generate_and_write("shellac", 120, 8, "4/4", vary=0.0, seed=42)
        _, n2, p2 = _generate_and_write("shellac", 120, 8, "4/4", vary=0.8, seed=42)
        try:
            mid1 = open(p1, "rb").read()
            mid2 = open(p2, "rb").read()
            assert mid1 != mid2, "Vary should produce different MIDI output"
        finally:
            os.unlink(p1)
            os.unlink(p2)

    # ── Seed reproducibility ──

    def test_same_seed_byte_identical(self):
        _, _, p1 = _generate_and_write("blast", 180, 4, "4/4", seed=777)
        _, _, p2 = _generate_and_write("blast", 180, 4, "4/4", seed=777)
        try:
            b1 = open(p1, "rb").read()
            b2 = open(p2, "rb").read()
            assert b1 == b2, "Same seed should produce byte-identical MIDI"
        finally:
            os.unlink(p1)
            os.unlink(p2)

    # ── 6/8 bug fix verification ──

    def test_posthardcore_6_8_uses_correct_cell(self):
        """Verify the 6/8 fix: should use driving_6_8 with only kick/snare/hihat."""
        mid, note_ons, path = _generate_and_write(
            "posthardcore", 130, 4, "6/8", seed=42,
        )
        try:
            assert note_ons > 0
            # Check that MIDI notes correspond to kick/snare/hihat, not exotic instruments
            from midi_engine import load_kit_mapping
            kit = load_kit_mapping("ugritone")
            mapping = kit["mapping"]
            expected_instruments = {"kick", "snare", "snare_ghost",
                                    "hihat_closed", "crash_1"}
            expected_notes = {mapping[inst] for inst in expected_instruments
                              if inst in mapping}
            for track in mid.tracks:
                for msg in track:
                    if msg.type == "note_on" and msg.velocity > 0:
                        assert msg.note in expected_notes, \
                            f"Unexpected MIDI note {msg.note} in posthardcore 6/8 output"
        finally:
            os.unlink(path)


# ── TestProbabilityGrids ─────────────────────────────────────────────────────

class TestProbabilityGrids:
    """Test probability grid cell definitions and realization."""

    def test_prob_cells_registered(self):
        prob_names = [
            "prob_faraquet_4_4", "prob_shellac_4_4", "prob_posthardcore_4_4",
            "prob_dbeat_4_4", "prob_blast_4_4", "prob_euro_screamo_4_4",
            "prob_faraquet_7_8",
        ]
        for name in prob_names:
            assert name in CELLS, f"{name} not in CELLS registry"
            cell = CELLS[name]
            assert cell["type"] == "probability"
            assert "grid" in cell

    def test_prob_cells_in_style_pools(self):
        assert "prob_faraquet_4_4" in STYLE_POOLS["faraquet"]
        assert "prob_faraquet_7_8" in STYLE_POOLS["faraquet"]
        assert "prob_shellac_4_4" in STYLE_POOLS["shellac"]
        assert "prob_posthardcore_4_4" in STYLE_POOLS["posthardcore"]
        assert "prob_dbeat_4_4" in STYLE_POOLS["dbeat"]
        assert "prob_blast_4_4" in STYLE_POOLS["blast"]
        assert "prob_euro_screamo_4_4" in STYLE_POOLS["euro_screamo"]
        assert "prob_faraquet_4_4" in STYLE_POOLS["math"]

    def test_normalize_grid_5tuple(self):
        cell = CELLS["prob_shellac_4_4"]
        normalized = _normalize_grid(cell)
        for entry in normalized:
            assert len(entry) == 6, f"Expected 6-tuple, got {entry}"
            assert entry[0] == 1  # single-bar cell

    def test_realize_produces_hits(self):
        import random
        cell = CELLS["prob_shellac_4_4"]
        rng = random.Random(42)
        hits = realize_probability_grid(cell, 4, rng)
        assert len(hits) > 0
        for h in hits:
            assert len(h) == 5

    def test_realize_different_seeds_different_output(self):
        import random
        cell = CELLS["prob_faraquet_4_4"]
        hits1 = realize_probability_grid(cell, 8, random.Random(1))
        hits2 = realize_probability_grid(cell, 8, random.Random(2))
        assert hits1 != hits2

    def test_realize_same_seed_same_output(self):
        import random
        cell = CELLS["prob_faraquet_4_4"]
        hits1 = realize_probability_grid(cell, 8, random.Random(42))
        hits2 = realize_probability_grid(cell, 8, random.Random(42))
        assert hits1 == hits2

    def test_realize_respects_bar_range(self):
        import random
        cell = CELLS["prob_blast_4_4"]
        hits = realize_probability_grid(cell, 4, random.Random(42))
        bars_present = {h[0] for h in hits}
        for bar in bars_present:
            assert 1 <= bar <= 4

    def test_realize_near_deterministic_shellac(self):
        import random
        cell = CELLS["prob_shellac_4_4"]
        hits = realize_probability_grid(cell, 1, random.Random(42))
        instruments = {h[3] for h in hits}
        assert "ride" in instruments

    def test_validate_physical_constraints(self):
        bar_hits = [
            (1, 1, 0.0, "ride", "normal"),
            (1, 1, 0.0, "hihat_closed", "normal"),
            (1, 1, 0.0, "snare", "accent"),
            (1, 1, 0.0, "tom_high", "normal"),
            (1, 1, 0.0, "kick", "accent"),
        ]
        filtered = _validate_physical_constraints(bar_hits)
        instruments = {h[3] for h in filtered}
        assert "ride" in instruments
        assert "hihat_closed" not in instruments
        assert "snare" in instruments
        assert "tom_high" not in instruments
        assert "kick" in instruments

    def test_assemble_generative(self):
        result = assemble(style="faraquet", bars=4, tempo=140, generative=True, seed=42)
        assert len(result["events"]) > 0
        assert result["tempo"] == 140

    def test_assemble_arrangement_generative(self):
        result = assemble_arrangement(
            style="shellac", arrangement_str="4:verse 2:blast",
            tempo=130, generative=True, seed=42,
        )
        assert len(result["events"]) > 0
        assert result["total_bars"] == 6

    def test_prob_grid_valid_entries(self):
        """All probability grid entries should have valid instruments and probabilities."""
        for name, cell in CELLS.items():
            if cell.get("type") != "probability":
                continue
            for entry in cell["grid"]:
                if len(entry) == 5:
                    beat, sub, inst, prob, vel = entry
                else:
                    bar, beat, sub, inst, prob, vel = entry
                assert 0.0 <= prob <= 1.0, \
                    f"Cell '{name}' has probability {prob} outside [0,1]"
                assert inst in VALID_INSTRUMENTS, \
                    f"Cell '{name}' has unknown instrument '{inst}'"
                assert vel in VALID_VELOCITY_LEVELS, \
                    f"Cell '{name}' has unknown velocity '{vel}'"


# ── TestLayerMode ────────────────────────────────────────────────────────────

class TestLayerMode:
    """Test layer extraction, conflict resolution, and layered assembly."""

    def test_layer_groups_complete(self):
        assert "kick" in LAYER_GROUPS
        assert "snare" in LAYER_GROUPS
        assert "cymbal" in LAYER_GROUPS
        assert "toms" in LAYER_GROUPS

    def test_extract_layer_kick(self):
        from assembler import _normalize_hits
        cell = get_cell("blast_traditional")
        hits = _normalize_hits(cell)
        kick_hits = extract_layer(hits, "kick")
        for h in kick_hits:
            assert h[3] == "kick"
        assert len(kick_hits) > 0

    def test_extract_layer_cymbal(self):
        from assembler import _normalize_hits
        cell = get_cell("shellac_floor_tom_drive")
        hits = _normalize_hits(cell)
        cymbal_hits = extract_layer(hits, "cymbal")
        for h in cymbal_hits:
            assert h[3] in LAYER_GROUPS["cymbal"]

    def test_resolve_layer_conflicts_cymbal_priority(self):
        hits = [
            (1, 1, 0.0, "ride", "normal"),
            (1, 1, 0.0, "crash_1", "accent"),
        ]
        resolved = _resolve_layer_conflicts(hits)
        instruments = {h[3] for h in resolved}
        assert "crash_1" in instruments
        assert "ride" not in instruments

    def test_resolve_layer_conflicts_same_instrument(self):
        hits = [
            (1, 1, 0.0, "kick", "normal"),
            (1, 1, 0.0, "kick", "accent"),
        ]
        resolved = _resolve_layer_conflicts(hits)
        kick_hits = [h for h in resolved if h[3] == "kick"]
        assert len(kick_hits) == 1
        assert kick_hits[0][4] == "accent"

    def test_assemble_layered_basic(self):
        result = assemble_layered(
            layers={"kick": "blast_traditional", "cymbal": "shellac_floor_tom_drive"},
            bars=2, tempo=160, seed=42,
        )
        assert len(result["events"]) > 0
        instruments = {inst for _, inst, _ in result["events"]}
        assert "kick" in instruments

    def test_assemble_layered_single_layer(self):
        result = assemble_layered(
            layers={"snare": "blast_traditional"},
            bars=2, tempo=160, seed=42,
        )
        instruments = {inst for _, inst, _ in result["events"]}
        for inst in instruments:
            assert inst in LAYER_GROUPS["snare"]

    def test_assemble_layered_all_layers(self):
        result = assemble_layered(
            layers={
                "kick": "blast_traditional",
                "snare": "dbeat_standard",
                "cymbal": "shellac_floor_tom_drive",
                "toms": "emoviolence_angular_breakdown",
            },
            bars=2, tempo=140, seed=42,
        )
        assert len(result["events"]) > 0


# ── TestMixedMeters ──────────────────────────────────────────────────────────

class TestMixedMeters:
    """Test mixed meters in arrangement mode."""

    def test_parse_arrangement_default_time_sig(self):
        sections = parse_arrangement("4:verse 2:blast")
        assert len(sections) == 2
        assert sections[0] == (4, "verse", (4, 4))
        assert sections[1] == (2, "blast", (4, 4))

    def test_parse_arrangement_with_time_sig(self):
        sections = parse_arrangement("4:verse@7/8 2:blast@4/4")
        assert sections[0] == (4, "verse", (7, 8))
        assert sections[1] == (2, "blast", (4, 4))

    def test_parse_arrangement_mixed(self):
        sections = parse_arrangement("4:build 8:drive@7/8 2:blast", default_time_sig="4/4")
        assert sections[0] == (4, "build", (4, 4))
        assert sections[1] == (8, "drive", (7, 8))
        assert sections[2] == (2, "blast", (4, 4))

    def test_parse_arrangement_custom_default(self):
        sections = parse_arrangement("4:verse 2:blast", default_time_sig="7/8")
        assert sections[0] == (4, "verse", (7, 8))
        assert sections[1] == (2, "blast", (7, 8))

    def test_parse_arrangement_invalid_time_sig(self):
        with pytest.raises(ValueError):
            parse_arrangement("4:verse@invalid")

    def test_consolidate_time_signatures(self):
        ts = [
            {"bar_start": 1, "bar_end": 4, "numerator": 4, "denominator": 4},
            {"bar_start": 5, "bar_end": 8, "numerator": 4, "denominator": 4},
            {"bar_start": 9, "bar_end": 12, "numerator": 7, "denominator": 8},
        ]
        consolidated = _consolidate_time_signatures(ts)
        assert len(consolidated) == 2
        assert consolidated[0]["bar_end"] == 8
        assert consolidated[1]["numerator"] == 7

    def test_consolidate_all_same(self):
        ts = [
            {"bar_start": 1, "bar_end": 4, "numerator": 4, "denominator": 4},
            {"bar_start": 5, "bar_end": 8, "numerator": 4, "denominator": 4},
        ]
        consolidated = _consolidate_time_signatures(ts)
        assert len(consolidated) == 1
        assert consolidated[0]["bar_end"] == 8

    def test_assemble_arrangement_mixed_meters(self):
        result = assemble_arrangement(
            style="shellac", arrangement_str="4:verse@7/8 2:verse@4/4 4:verse@7/8",
            tempo=130, seed=42,
        )
        assert result["total_bars"] == 10
        ts = result["time_signatures"]
        assert len(ts) > 1
        assert len(result["events"]) > 0

    def test_assemble_arrangement_single_meter(self):
        result = assemble_arrangement(
            style="shellac", arrangement_str="4:verse@4/4 4:drive@4/4",
            tempo=120, seed=42,
        )
        assert len(result["time_signatures"]) == 1

    def test_mixed_meter_midi_output(self):
        """Write mixed meter arrangement to MIDI and verify time sig meta messages and note spread."""
        result = assemble_arrangement(
            style="shellac", arrangement_str="2:verse@7/8 2:drive@4/4",
            tempo=130, seed=42,
        )
        with tempfile.NamedTemporaryFile(suffix=".mid", delete=False) as f:
            tmp_path = f.name
        try:
            write_midi(
                events=result["events"], tempo=result["tempo"],
                time_signatures=result["time_signatures"],
                kit_mapping_path="ugritone", output_path=tmp_path,
            )
            mid = mido.MidiFile(tmp_path)
            ts_msgs = [m for t in mid.tracks for m in t if m.type == "time_signature"]
            assert len(ts_msgs) >= 2, f"Expected 2+ time sig messages, got {len(ts_msgs)}"
            assert ts_msgs[0].numerator == 7
            assert ts_msgs[0].denominator == 8
            assert ts_msgs[1].numerator == 4
            assert ts_msgs[1].denominator == 4

            # Verify notes are NOT all clustered in second half (the bug)
            running_tick = 0
            note_on_ticks = []
            for track in mid.tracks:
                running_tick = 0
                for msg in track:
                    running_tick += msg.time
                    if msg.type == "note_on" and msg.velocity > 0:
                        note_on_ticks.append(running_tick)
            assert len(note_on_ticks) > 0, "No note_on events in MIDI"
            mid_point = max(note_on_ticks) // 2
            first_half = [t for t in note_on_ticks if t < mid_point]
            assert len(first_half) > 0, "All notes clustered in second half — time sig interleaving bug"
        finally:
            os.unlink(tmp_path)

    def test_mixed_meter_note_positions(self):
        """Regression: notes in each section land within that section's tick range."""
        result = assemble_arrangement(
            style="shellac", arrangement_str="2:verse@7/8 2:drive@4/4",
            tempo=130, seed=42,
        )
        ts = result["time_signatures"]
        ppq = DEFAULT_PPQ

        # Calculate expected tick boundaries for each section
        # Section 1: bars 1-2 in 7/8, Section 2: bars 3-4 in 4/4
        section1_start = calculate_bar_start_ticks(1, ts, ppq)
        section2_start = calculate_bar_start_ticks(3, ts, ppq)
        section2_end = calculate_bar_start_ticks(5, ts, ppq)

        with tempfile.NamedTemporaryFile(suffix=".mid", delete=False) as f:
            tmp_path = f.name
        try:
            write_midi(
                events=result["events"], tempo=result["tempo"],
                time_signatures=ts,
                kit_mapping_path="ugritone", output_path=tmp_path,
            )
            mid = mido.MidiFile(tmp_path)

            # Collect absolute ticks for all note_on events
            note_on_ticks = []
            for track in mid.tracks:
                running_tick = 0
                for msg in track:
                    running_tick += msg.time
                    if msg.type == "note_on" and msg.velocity > 0:
                        note_on_ticks.append(running_tick)

            assert len(note_on_ticks) > 0

            # Notes should span from section 1 start through section 2
            first_note = min(note_on_ticks)
            last_note = max(note_on_ticks)

            # First note should be near the beginning (within section 1)
            assert first_note < section2_start, \
                f"First note at tick {first_note} >= section 2 start {section2_start}"

            # There should be notes in section 1 range
            section1_notes = [t for t in note_on_ticks if section1_start <= t < section2_start]
            assert len(section1_notes) > 0, \
                f"No notes in section 1 (ticks {section1_start}-{section2_start})"

            # There should be notes in section 2 range
            section2_notes = [t for t in note_on_ticks if section2_start <= t < section2_end]
            assert len(section2_notes) > 0, \
                f"No notes in section 2 (ticks {section2_start}-{section2_end})"

            # No notes should exceed the total arrangement length (with small tolerance for humanizer)
            humanizer_tolerance = ppq  # one beat of tolerance for timing humanization
            assert last_note < section2_end + humanizer_tolerance, \
                f"Note at tick {last_note} exceeds arrangement end {section2_end}"
        finally:
            os.unlink(tmp_path)


# ── TestVariations ───────────────────────────────────────────────────────────

class TestVariations:
    """Test that different seeds produce different outputs in generative mode."""

    def test_different_seeds_different_events(self):
        r1 = assemble(style="faraquet", bars=4, tempo=140, generative=True, seed=1)
        r2 = assemble(style="faraquet", bars=4, tempo=140, generative=True, seed=2)
        assert r1["events"] != r2["events"]

    def test_same_seed_reproducible(self):
        r1 = assemble(style="faraquet", bars=4, tempo=140, generative=True, seed=42)
        r2 = assemble(style="faraquet", bars=4, tempo=140, generative=True, seed=42)
        assert r1["events"] == r2["events"]

    def test_generative_7_8(self):
        result = assemble(style="faraquet", bars=4, tempo=140,
                          time_sig="7/8", generative=True, seed=42)
        assert len(result["events"]) > 0

    def test_generative_midi_output(self):
        """Full pipeline: generative -> MIDI file."""
        result = assemble(style="shellac", bars=4, tempo=120, generative=True, seed=42)
        with tempfile.NamedTemporaryFile(suffix=".mid", delete=False) as f:
            tmp_path = f.name
        try:
            write_midi(
                events=result["events"], tempo=result["tempo"],
                time_signatures=result["time_signatures"],
                kit_mapping_path="ugritone", output_path=tmp_path,
            )
            mid = mido.MidiFile(tmp_path)
            note_ons = sum(
                1 for t in mid.tracks for m in t
                if m.type == "note_on" and m.velocity > 0
            )
            assert note_ons > 0
        finally:
            os.unlink(tmp_path)


# ── TestNewStyleCells ────────────────────────────────────────────────────────

class TestNewStyleCells:
    """Cell-specific assertions for Phase 3 style palette expansion."""

    def test_motorik_pulse_no_ghost_no_ride(self):
        cell = CELLS["motorik_pulse"]
        instruments = {h[2] for h in cell["hits"]}
        velocities = {h[3] for h in cell["hits"]}
        assert "ride" not in instruments
        assert "ghost" not in velocities
        hh_hits = [h for h in cell["hits"] if h[2] == "hihat_closed"]
        assert len(hh_hits) == 8

    def test_motorik_build_structure(self):
        cell = CELLS["motorik_build"]
        assert cell["num_bars"] == 4
        assert "humanize_per_bar" in cell
        bar1_vels = {h[4] for h in cell["hits"] if h[0] == 1}
        bar4_vels = {h[4] for h in cell["hits"] if h[0] == 4}
        assert bar1_vels == {"ghost"}
        assert bar4_vels == {"accent"}

    def test_slint_explosion_has_floor_tom_and_ride(self):
        cell = CELLS["slint_explosion"]
        instruments = {h[2] for h in cell["hits"]}
        assert "tom_floor" in instruments
        assert "ride" in instruments

    def test_athletic_angular_structure(self):
        cell = CELLS["athletic_angular"]
        assert cell["num_bars"] == 2
        instruments = {h[3] for h in cell["hits"]}
        assert "snare_ghost" in instruments
        assert "tom_floor" in instruments

    def test_postpunk_machine_no_ghost_has_hihat(self):
        cell = CELLS["postpunk_machine"]
        instruments = {h[2] for h in cell["hits"]}
        velocities = {h[3] for h in cell["hits"]}
        assert "hihat_closed" in instruments
        assert "ride" not in instruments
        assert "ghost" not in velocities

    def test_postpunk_busy_has_hihat_open(self):
        cell = CELLS["postpunk_busy"]
        assert cell["num_bars"] == 2
        instruments = {h[3] for h in cell["hits"]}
        assert "hihat_open" in instruments

    def test_unwound_dynamics_structure(self):
        cell = CELLS["unwound_dynamics"]
        assert cell["num_bars"] == 4
        assert "humanize_per_bar" in cell
        # Quiet bars have ride_bell
        quiet_instruments = {h[3] for h in cell["hits"] if h[0] in (1, 2)}
        assert "ride_bell" in quiet_instruments
        # Loud bars have kick + ride + snare
        loud_instruments = {h[3] for h in cell["hits"] if h[0] in (3, 4)}
        assert "kick" in loud_instruments
        assert "ride" in loud_instruments
        assert "snare" in loud_instruments

    def test_city_of_caterpillar_build_structure(self):
        cell = CELLS["city_of_caterpillar_build"]
        assert cell["num_bars"] == 8
        assert "humanize_per_bar" in cell
        # Progressive instrumentation: bars 1-2 ride_bell only
        bar1_instruments = {h[3] for h in cell["hits"] if h[0] == 1}
        assert bar1_instruments == {"ride_bell"}
        # Bars 7-8 should have kick, snare, ride
        bar7_instruments = {h[3] for h in cell["hits"] if h[0] == 7}
        assert "kick" in bar7_instruments
        assert "snare" in bar7_instruments
        assert "ride" in bar7_instruments
        assert "crash_1" in bar7_instruments

    def test_prob_slint_multi_bar_grid(self):
        cell = CELLS["prob_slint_4_4"]
        assert cell["num_bars"] == 4
        assert cell.get("type") == "probability"
        # All grid entries should be 6-tuples (multi-bar)
        for entry in cell["grid"]:
            assert len(entry) == 6, f"Expected 6-tuple, got {len(entry)}-tuple: {entry}"


# ── TestNewStylePools ────────────────────────────────────────────────────────

class TestNewStylePools:
    """Verify Phase 3 style pools exist and reference valid cells."""

    NEW_STYLES = [
        "sonic_youth", "slint", "post_punk", "wipers", "preoccupations",
        "dry_cleaning", "shame", "drive_like_jehu", "q_and_not_u", "atdi",
        "blood_brothers", "unwound", "city_of_caterpillar", "oxbow",
    ]

    @pytest.mark.parametrize("style", NEW_STYLES)
    def test_pool_exists(self, style):
        assert style in STYLE_POOLS, f"Style '{style}' missing from STYLE_POOLS"

    @pytest.mark.parametrize("style", NEW_STYLES)
    def test_pool_cells_exist(self, style):
        for cell_name in STYLE_POOLS[style]:
            assert cell_name in CELLS, f"Cell '{cell_name}' in pool '{style}' not found in CELLS"

    def test_posthardcore_has_athletic_angular(self):
        assert "athletic_angular" in STYLE_POOLS["posthardcore"]

    def test_noise_rock_has_unwound_dynamics(self):
        assert "unwound_dynamics" in STYLE_POOLS["noise_rock"]

    def test_screamo_has_city_of_caterpillar_build(self):
        assert "city_of_caterpillar_build" in STYLE_POOLS["screamo"]

    def test_euro_screamo_has_city_of_caterpillar_build(self):
        assert "city_of_caterpillar_build" in STYLE_POOLS["euro_screamo"]


# ── TestNewStylesEndToEnd ────────────────────────────────────────────────────

class TestNewStylesEndToEnd:
    """End-to-end generation tests for Phase 3 styles."""

    NEW_STYLES = [
        "sonic_youth", "slint", "post_punk", "wipers", "preoccupations",
        "dry_cleaning", "shame", "drive_like_jehu", "q_and_not_u", "atdi",
        "blood_brothers", "unwound", "city_of_caterpillar", "oxbow",
    ]

    @pytest.mark.parametrize("style", NEW_STYLES)
    def test_generate_4_bars(self, style):
        result = assemble(style=style, bars=4, tempo=130, seed=42)
        assert len(result["events"]) > 0

    @pytest.mark.parametrize("style", ["sonic_youth", "slint", "drive_like_jehu"])
    def test_generative_mode(self, style):
        result = assemble(style=style, bars=4, tempo=130, generative=True, seed=42)
        assert len(result["events"]) > 0

    def test_prob_slint_multi_bar_realization(self):
        """prob_slint_4_4 is 4-bar grid — 8 bars should cycle 2x."""
        result = assemble(style="slint", bars=8, tempo=100, generative=True, seed=42)
        assert len(result["events"]) > 0

    def test_slint_arrangement_mode(self):
        result = assemble_arrangement(
            style="slint", arrangement_str="4:atmospheric 4:drive",
            tempo=100, seed=42,
        )
        assert len(result["events"]) > 0

    def test_city_of_caterpillar_arrangement(self):
        result = assemble_arrangement(
            style="city_of_caterpillar",
            arrangement_str="8:build 4:blast 4:breakdown",
            tempo=130, seed=42,
        )
        assert len(result["events"]) > 0
