import random


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
