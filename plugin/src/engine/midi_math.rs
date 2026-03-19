/// Pulses per quarter note — matches Python MIDI engine.
pub const PPQ: i64 = 480;

/// Note duration in ticks — matches Python MIDI engine.
pub const NOTE_DURATION: i64 = 30;

/// A time signature entry for a range of bars.
#[derive(Debug, Clone)]
pub struct TimeSigEntry {
    /// First bar this time sig applies to (1-indexed).
    pub bar_start: i32,
    /// Last bar this time sig applies to (inclusive, 1-indexed).
    pub bar_end: i32,
    pub numerator: i32,
    pub denominator: i32,
}

/// Get the (numerator, denominator) for a given bar number.
pub fn get_time_sig_for_bar(bar_number: i32, time_signatures: &[TimeSigEntry]) -> (i32, i32) {
    for ts in time_signatures {
        if ts.bar_start <= bar_number && bar_number <= ts.bar_end {
            return (ts.numerator, ts.denominator);
        }
    }
    // Fallback to last entry
    if let Some(ts) = time_signatures.last() {
        (ts.numerator, ts.denominator)
    } else {
        (4, 4)
    }
}

/// Calculate the absolute tick position of the start of `bar_number`.
///
/// Accumulates ticks for bars 1..bar_number-1 using their time signatures.
pub fn calculate_bar_start_ticks(bar_number: i32, time_signatures: &[TimeSigEntry], ppq: i64) -> i64 {
    let mut total_ticks: i64 = 0;
    for b in 1..bar_number {
        let (num, den) = get_time_sig_for_bar(b, time_signatures);
        let bar_ticks = (num as i64) * (ppq * 4 / den as i64);
        total_ticks += bar_ticks;
    }
    total_ticks
}

/// Convert a (bar, beat, sub) position to an absolute tick.
///
/// - `bar_number`: 1-indexed
/// - `beat`: 1-indexed beat within the bar
/// - `sub`: subdivision offset (0.0 = on beat, 0.25 = 16th, 0.5 = 8th, 0.75 = dotted 8th)
pub fn position_to_ticks(
    bar_number: i32,
    beat: i32,
    sub: f64,
    time_signatures: &[TimeSigEntry],
    ppq: i64,
) -> i64 {
    let (_num, den) = get_time_sig_for_bar(bar_number, time_signatures);
    let bar_start = calculate_bar_start_ticks(bar_number, time_signatures, ppq);
    let beat_ticks = ppq * 4 / den as i64;
    let tick_offset = (sub * beat_ticks as f64) as i64;
    bar_start + (beat as i64 - 1) * beat_ticks + tick_offset
}

/// Calculate the total ticks for a range of bars using time signatures.
pub fn total_pattern_ticks(total_bars: i32, time_signatures: &[TimeSigEntry], ppq: i64) -> i64 {
    calculate_bar_start_ticks(total_bars + 1, time_signatures, ppq)
}

/// Get beat ticks for a given denominator.
pub fn beat_ticks_for_denominator(denominator: i32, ppq: i64) -> i64 {
    ppq * 4 / denominator as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts_4_4(bars: i32) -> Vec<TimeSigEntry> {
        vec![TimeSigEntry { bar_start: 1, bar_end: bars, numerator: 4, denominator: 4 }]
    }

    #[test]
    fn test_position_to_ticks_basic() {
        let ts = ts_4_4(4);
        // Bar 1, beat 1, on beat
        assert_eq!(position_to_ticks(1, 1, 0.0, &ts, PPQ), 0);
        // Bar 1, beat 2, on beat
        assert_eq!(position_to_ticks(1, 2, 0.0, &ts, PPQ), 480);
        // Bar 1, beat 1, eighth note
        assert_eq!(position_to_ticks(1, 1, 0.5, &ts, PPQ), 240);
        // Bar 2, beat 1
        assert_eq!(position_to_ticks(2, 1, 0.0, &ts, PPQ), 1920);
    }

    #[test]
    fn test_bar_start_ticks() {
        let ts = ts_4_4(4);
        assert_eq!(calculate_bar_start_ticks(1, &ts, PPQ), 0);
        assert_eq!(calculate_bar_start_ticks(2, &ts, PPQ), 1920);
        assert_eq!(calculate_bar_start_ticks(3, &ts, PPQ), 3840);
    }

    #[test]
    fn test_total_pattern_ticks() {
        let ts = ts_4_4(4);
        assert_eq!(total_pattern_ticks(4, &ts, PPQ), 7680);
    }

    #[test]
    fn test_mixed_meter() {
        let ts = vec![
            TimeSigEntry { bar_start: 1, bar_end: 2, numerator: 7, denominator: 8 },
            TimeSigEntry { bar_start: 3, bar_end: 4, numerator: 4, denominator: 4 },
        ];
        // 7/8: beat_ticks = 480*4/8 = 240, bar_ticks = 7*240 = 1680
        assert_eq!(calculate_bar_start_ticks(1, &ts, PPQ), 0);
        assert_eq!(calculate_bar_start_ticks(2, &ts, PPQ), 1680);
        assert_eq!(calculate_bar_start_ticks(3, &ts, PPQ), 3360);
        // Bar 3 is 4/4: 1920 ticks
        assert_eq!(calculate_bar_start_ticks(4, &ts, PPQ), 3360 + 1920);
    }
}
