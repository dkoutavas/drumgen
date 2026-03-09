import random


# Section drift profiles: how timing shifts across bars within a section
_SECTION_DRIFT = {
    "verse":       "gradual_drag",
    "atmospheric": "gradual_drag",
    "intro":       "gradual_drag",
    "quiet":       "gradual_drag",
    "outro":       "gradual_drag",
    "chorus":      "constant_push",
    "blast":       "constant_push",
    "drive":       "constant_push",
    "build":       "gradual_push",
    "buildup":     "gradual_push",
    "crescendo":   "gradual_push",
    "breakdown":   "constant_drag",
    "half_time":   "constant_drag",
    "fill":        "fill_rush",
}

# Tag → ghost clustering amount (0.0 = none, 1.0 = max)
_CLUSTER_TAG_AMOUNTS = {
    "faraquet": 0.7, "angular": 0.6, "math": 0.6,
    "raein": 0.5, "euro_screamo": 0.5, "daitro": 0.5,
    "fugazi": 0.4, "posthardcore": 0.4, "driving": 0.4,
    "screamo": 0.3, "emoviolence": 0.3,
    "shellac": 0.0, "noise_rock": 0.0, "blast": 0.0,
    "post_punk": 0.0, "motorik": 0.0,
}


def get_cluster_amount(cell):
    """Return ghost clustering amount based on cell tags. 0.0-1.0."""
    tags = cell.get("tags", [])
    amounts = [_CLUSTER_TAG_AMOUNTS[t] for t in tags if t in _CLUSTER_TAG_AMOUNTS]
    return max(amounts) if amounts else 0.3


def infer_section_type(cell):
    """Infer section type from cell tags for push/pull drift."""
    tags = cell.get("tags", [])
    if any(t in tags for t in ("blast", "extreme")):
        return "blast"
    if any(t in tags for t in ("build", "crescendo")):
        return "build"
    if any(t in tags for t in ("breakdown", "half_time")):
        return "breakdown"
    if any(t in tags for t in ("atmospheric", "sparse", "quiet")):
        return "atmospheric"
    if any(t in tags for t in ("driving", "intense")):
        return "drive"
    if any(t in tags for t in ("fill",)):
        return "fill"
    return "verse"


class Humanizer:
    VELOCITY_RANGES = {
        "ghost": (20, 50),
        "soft": (50, 75),
        "normal": (75, 105),
        "accent": (105, 127),
    }

    INSTRUMENT_VARIANCE = {
        "hihat_closed": 25,
        "hihat_open": 20,
        "ride": 22,
        "ride_bell": 20,
        "snare": 18,
        "snare_ghost": 20,
        "snare_rim": 15,
        "kick": 16,
        "crash_1": 14,
        "crash_2": 14,
        "china": 16,
        "splash": 14,
        "tom_high": 18,
        "tom_mid": 18,
        "tom_low": 18,
        "tom_floor": 18,
        "hihat_pedal": 15,
        "tom_mid_high": 18,
        "hihat_wide_open": 20,
        "crash_1_choke": 8,
        "crash_2_choke": 8,
        "ride_crash": 16,
        "fx_cymbal_1": 14,
        "fx_cymbal_2": 14,
        "china_2": 16,
    }

    TIMING_TENDENCIES = {
        "snare": 2,
        "snare_ghost": 4,
        "snare_rim": 2,
        "kick": 0,
        "hihat_closed": -1,
        "ride": -2,
        "ride_bell": -2,
        "crash_1": -3,
        "crash_2": -3,
        "china": -2,
        "tom_floor": 1,
        "tom_low": 1,
        "tom_mid": 0,
        "tom_high": 0,
        "hihat_pedal": 0,
        "hihat_open": -1,
        "splash": -2,
        "tom_mid_high": 0,
        "hihat_wide_open": -1,
        "crash_1_choke": 0,
        "crash_2_choke": 0,
        "ride_crash": -3,
        "fx_cymbal_1": -2,
        "fx_cymbal_2": -2,
        "china_2": -2,
    }

    CONTOUR_INSTRUMENTS = {"ride", "ride_bell", "hihat_closed", "hihat_open", "hihat_wide_open"}

    CONTOUR_OFFSETS = {
        0.0:  +6,   # downbeat — strongest
        0.25: -4,   # first sixteenth — weak
        0.5:  +3,   # eighth — medium strong
        0.75: -5,   # third sixteenth — weakest
    }

    def __init__(self, humanize_amount, seed=None):
        self.rng = random.Random(seed)
        self.humanize_amount = humanize_amount

    def humanize_velocity(self, velocity_level, instrument):
        level = velocity_level.lower()
        if level not in self.VELOCITY_RANGES:
            level = "normal"
        low, high = self.VELOCITY_RANGES[level]
        center = (low + high) // 2
        variance = self.INSTRUMENT_VARIANCE.get(instrument, 12)
        scaled_variance = max(3, int(variance * self.humanize_amount))
        if scaled_variance <= 0:
            return max(1, min(127, center))
        vel = self.rng.randint(center - scaled_variance, center + scaled_variance)
        return max(1, min(127, vel))

    def humanize_timing(self, abs_tick, instrument, tempo_bpm, ppq=480):
        ms_per_tick = (60000.0 / tempo_bpm) / ppq
        tendency_ms = self.TIMING_TENDENCIES.get(instrument, 0)
        tendency_ticks = int(tendency_ms * self.humanize_amount / ms_per_tick)
        jitter_ms = self.rng.gauss(0, 6 * self.humanize_amount)
        jitter_ticks = int(jitter_ms / ms_per_tick)
        return max(0, abs_tick + tendency_ticks + jitter_ticks)

    def apply_swing(self, abs_tick, is_upbeat, swing_amount, ticks_per_beat):
        if not is_upbeat or swing_amount <= 0:
            return abs_tick
        triplet_offset = ticks_per_beat // 3
        swing_offset = int(triplet_offset * swing_amount)
        return abs_tick + swing_offset

    def velocity_contour(self, velocity, instrument, beat, sub):
        """Apply wrist-pattern contour + beat-1 emphasis to cymbal instruments."""
        if instrument not in self.CONTOUR_INSTRUMENTS:
            return velocity
        contour_amount = self.humanize_amount * 0.8
        if contour_amount == 0:
            return velocity
        closest_sub = min(self.CONTOUR_OFFSETS, key=lambda s: abs(s - sub))
        offset = self.CONTOUR_OFFSETS[closest_sub] * contour_amount
        if beat == 1:
            offset += 4 * contour_amount
        return max(1, min(127, velocity + int(offset)))

    def compute_section_drift_ms(self, section_type, bar_index, total_bars):
        """Compute timing drift in ms for a bar based on section type."""
        drift_amount = self.humanize_amount * 0.7
        if drift_amount == 0:
            return 0.0
        profile = _SECTION_DRIFT.get(section_type)
        if not profile:
            return 0.0
        progress = bar_index / max(1, total_bars - 1) if total_bars > 1 else 0.0
        if profile == "gradual_drag":
            return min(6.0, bar_index * 1.0) * drift_amount
        elif profile == "constant_push":
            return -4.0 * drift_amount
        elif profile == "constant_drag":
            return 5.0 * drift_amount
        elif profile == "gradual_push":
            return -6.0 * progress * drift_amount
        elif profile == "fill_rush":
            return -8.0 * progress * drift_amount
        return 0.0

    def apply_flam(self, events, tempo, ppq):
        """Pull kick earlier when simultaneous with snare. Returns new list."""
        if self.humanize_amount < 0.2:
            return list(events)
        ms_per_tick = (60000.0 / tempo) / ppq
        tick_map = {}
        for i, (tick, inst, vel) in enumerate(events):
            tick_map.setdefault(tick, []).append(i)
        result = list(events)
        for tick, indices in tick_map.items():
            instruments = {result[i][1]: i for i in indices}
            velocities = {result[i][1]: result[i][2] for i in indices}
            has_kick = "kick" in instruments and velocities.get("kick", 0) > 75
            has_snare = any(
                inst in instruments and velocities.get(inst, 0) > 75
                for inst in ("snare", "snare_rim")
            )
            if has_kick and has_snare:
                flam_ms = self.rng.uniform(5, 12)
                flam_ticks = max(1, int(flam_ms / ms_per_tick))
                ki = instruments["kick"]
                old = result[ki]
                result[ki] = (max(0, old[0] - flam_ticks), old[1], old[2])
        return result

    def apply_ghost_clustering(self, events, cluster_amount, tempo, ppq):
        """Pull ghost notes toward nearby snare accents. Returns new list."""
        effective = cluster_amount * self.humanize_amount
        if effective <= 0:
            return list(events)
        sixteenth_ticks = ppq // 4
        ghosts, accents, others = [], [], []
        for i, (tick, inst, vel) in enumerate(events):
            if inst == "snare_ghost" or (inst == "snare" and vel < 50):
                ghosts.append([tick, inst, vel])
            elif inst in ("snare", "snare_rim") and vel >= 100:
                accents.append((tick, inst, vel))
                others.append((tick, inst, vel))
            else:
                others.append((tick, inst, vel))
        if not accents or not ghosts:
            return list(events)
        accent_ticks = sorted(set(t for t, _, _ in accents))
        new_ghosts = []
        all_ticks = set(e[0] for e in events)
        for ghost in ghosts:
            nearest = min(accent_ticks, key=lambda t: abs(t - ghost[0]))
            distance = nearest - ghost[0]
            pull = int(distance * effective * 0.4)
            pull = max(-sixteenth_ticks, min(sixteenth_ticks, pull))
            ghost[0] = max(0, ghost[0] + pull)
            ghost[0] = max(0, ghost[0] + self.rng.randint(-5, 5))
            if self.rng.random() < effective * 0.3:
                if distance > 0:
                    new_tick = nearest + self.rng.randint(sixteenth_ticks // 2, sixteenth_ticks)
                else:
                    new_tick = nearest - self.rng.randint(sixteenth_ticks // 2, sixteenth_ticks)
                new_tick = max(0, new_tick)
                if not any(abs(new_tick - t) < sixteenth_ticks // 4 for t in all_ticks):
                    new_vel = self.rng.randint(20, 45)
                    new_ghosts.append((new_tick, "snare_ghost", new_vel))
                    all_ticks.add(new_tick)
        return others + [tuple(g) for g in ghosts] + new_ghosts
