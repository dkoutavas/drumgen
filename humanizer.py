import random


class Humanizer:
    VELOCITY_RANGES = {
        "ghost": (20, 50),
        "soft": (50, 75),
        "normal": (75, 105),
        "accent": (105, 127),
    }

    INSTRUMENT_VARIANCE = {
        "hihat_closed": 20,
        "hihat_open": 15,
        "ride": 18,
        "ride_bell": 15,
        "snare": 12,
        "snare_ghost": 15,
        "snare_rim": 10,
        "kick": 10,
        "crash_1": 8,
        "crash_2": 8,
        "china": 10,
        "splash": 10,
        "tom_high": 14,
        "tom_mid": 14,
        "tom_low": 14,
        "tom_floor": 14,
        "hihat_pedal": 12,
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
        scaled_variance = int(variance * self.humanize_amount)
        if scaled_variance <= 0:
            return max(1, min(127, center))
        vel = self.rng.randint(center - scaled_variance, center + scaled_variance)
        return max(1, min(127, vel))

    def humanize_timing(self, abs_tick, instrument, tempo_bpm, ppq=480):
        ms_per_tick = (60000.0 / tempo_bpm) / ppq
        tendency_ms = self.TIMING_TENDENCIES.get(instrument, 0)
        tendency_ticks = int(tendency_ms * self.humanize_amount / ms_per_tick)
        jitter_ms = self.rng.gauss(0, 3 * self.humanize_amount)
        jitter_ticks = int(jitter_ms / ms_per_tick)
        return max(0, abs_tick + tendency_ticks + jitter_ticks)

    def apply_swing(self, abs_tick, is_upbeat, swing_amount, ticks_per_beat):
        if not is_upbeat or swing_amount <= 0:
            return abs_tick
        triplet_offset = ticks_per_beat // 3
        swing_offset = int(triplet_offset * swing_amount)
        return abs_tick + swing_offset
