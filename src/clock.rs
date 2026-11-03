//! Construction clocks (Rebuild Prompt, "Time, Reveal, And Endless Motion",
//! three construction clocks).
//!
//! - **Untimed:** progress grows by `dt * speed / 240` each frame.
//! - **Duration** (`--timer`, alias `--deadline`): progress is monotonic
//!   elapsed time over the requested duration, independent of speed, so the
//!   city completes exactly when the timer runs out.
//!
//! The absolute local-clock deadline (`--ends-at`) arrives with the full
//! launch configuration. Progress never decreases; [`crate::reveal::Reveal`]
//! enforces that.

use crate::reveal::untimed_step;

/// Shortest allowed duration in seconds.
pub const MIN_DURATION: f64 = 1.0;
/// Longest allowed duration in seconds (24 hours).
pub const MAX_DURATION: f64 = 24.0 * 60.0 * 60.0;

/// How construction progress advances.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ConstructionClock {
    /// 240 seconds at 1x, scaled by the speed setting.
    #[default]
    Untimed,
    /// Completes after exactly this many seconds, whatever the speed.
    Duration {
        /// Total construction time in seconds.
        seconds: f64,
    },
}

impl ConstructionClock {
    /// Progress for this frame.
    ///
    /// `previous` is the current progress, `dt` the frame time, `speed` the
    /// speed setting, and `elapsed` the monotonic seconds since construction
    /// (re)started. The result is clamped to `0..=1`.
    pub fn progress(&self, previous: f64, dt: f64, speed: f64, elapsed: f64) -> f64 {
        let progress = match *self {
            ConstructionClock::Untimed => untimed_step(previous, dt, speed),
            ConstructionClock::Duration { seconds } => elapsed.max(0.0) / seconds.max(MIN_DURATION),
        };
        progress.clamp(0.0, 1.0)
    }

    /// Seconds left for a duration clock, or `None` when untimed.
    pub fn remaining(&self, elapsed: f64) -> Option<f64> {
        match *self {
            ConstructionClock::Untimed => None,
            ConstructionClock::Duration { seconds } => Some((seconds - elapsed).max(0.0)),
        }
    }

    /// True if the speed setting affects this clock.
    pub fn uses_speed(&self) -> bool {
        matches!(self, ConstructionClock::Untimed)
    }
}

/// Parses a duration: a bare number means minutes; otherwise one or more
/// `<number><unit>` parts with units `h`, `m`, `s` in that order, for
/// example `25`, `25m`, `90s`, `1h30m`. Accepts 1 second to 24 hours.
///
/// # Errors
///
/// Returns a message for empty, malformed, zero, or out-of-range input.
pub fn parse_duration(text: &str) -> Result<f64, String> {
    let text = text.trim().to_ascii_lowercase();
    let invalid =
        || format!("'{text}' is not a duration; use minutes (10) or units like 90s, 25m, 1h30m");
    if text.is_empty() {
        return Err(invalid());
    }
    let seconds = if text.chars().all(|c| c.is_ascii_digit()) {
        text.parse::<f64>().map_err(|_| invalid())? * 60.0
    } else {
        let mut total = 0.0;
        let mut number = String::new();
        let mut last_unit = 0; // 3 = h, 2 = m, 1 = s; units must descend
        for c in text.chars() {
            if c.is_ascii_digit() {
                number.push(c);
                continue;
            }
            let (rank, scale) = match c {
                'h' => (3, 3600.0),
                'm' => (2, 60.0),
                's' => (1, 1.0),
                _ => return Err(invalid()),
            };
            if number.is_empty() || (last_unit != 0 && rank >= last_unit) {
                return Err(invalid());
            }
            total += number.parse::<f64>().map_err(|_| invalid())? * scale;
            number.clear();
            last_unit = rank;
        }
        if !number.is_empty() {
            return Err(invalid());
        }
        total
    };
    if !(MIN_DURATION..=MAX_DURATION).contains(&seconds) {
        return Err(format!(
            "the timer must be between 1 second and 24 hours, got '{text}'"
        ));
    }
    Ok(seconds)
}

/// Formats seconds left as `m:ss` or `h:mm:ss`, rounding up so the display
/// reaches 0:00 only when time is up.
pub fn format_remaining(seconds: f64) -> String {
    let total = seconds.max(0.0).ceil() as u64;
    let (hours, minutes, secs) = (total / 3600, (total / 60) % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_numbers_are_minutes_and_units_combine() {
        assert_eq!(parse_duration("10"), Ok(600.0));
        assert_eq!(parse_duration("25m"), Ok(1500.0));
        assert_eq!(parse_duration("90s"), Ok(90.0));
        assert_eq!(parse_duration("10s"), Ok(10.0));
        assert_eq!(parse_duration("1h30m"), Ok(5400.0));
        assert_eq!(parse_duration("1h2m3s"), Ok(3723.0));
        assert_eq!(parse_duration(" 2H "), Ok(7200.0));
        assert_eq!(parse_duration("24h"), Ok(MAX_DURATION));
        assert_eq!(parse_duration("1s"), Ok(1.0));
    }

    #[test]
    fn malformed_and_out_of_range_durations_are_rejected() {
        for text in [
            "", "0", "0s", "24h1s", "25h", "ten", "10x", "m", "10m5", "5s10m", "1m1m", "-5", "1.5m",
        ] {
            assert!(parse_duration(text).is_err(), "{text:?} should fail");
        }
    }

    #[test]
    fn duration_progress_ignores_speed_and_completes_on_time() {
        let clock = ConstructionClock::Duration { seconds: 10.0 };
        assert_eq!(clock.progress(0.0, 0.016, 4.0, 5.0), 0.5);
        assert_eq!(clock.progress(0.0, 0.016, 0.25, 5.0), 0.5);
        assert_eq!(clock.progress(0.0, 0.016, 1.0, 10.0), 1.0);
        assert_eq!(clock.progress(0.0, 0.016, 1.0, 99.0), 1.0, "clamped");
        assert_eq!(clock.progress(0.0, 0.016, 1.0, -1.0), 0.0);
        assert_eq!(clock.remaining(3.5), Some(6.5));
        assert_eq!(clock.remaining(20.0), Some(0.0));
        assert!(!clock.uses_speed());
    }

    #[test]
    fn untimed_progress_uses_speed_and_frame_time() {
        let clock = ConstructionClock::default();
        assert_eq!(clock, ConstructionClock::Untimed);
        assert_eq!(clock.progress(0.0, 0.25, 4.0, 999.0), 1.0 / 240.0);
        assert_eq!(clock.remaining(5.0), None);
        assert!(clock.uses_speed());
    }

    #[test]
    fn remaining_time_formats_round_up() {
        assert_eq!(format_remaining(0.0), "0:00");
        assert_eq!(format_remaining(0.2), "0:01");
        assert_eq!(format_remaining(59.0), "0:59");
        assert_eq!(format_remaining(600.0), "10:00");
        assert_eq!(format_remaining(3723.0), "1:02:03");
        assert_eq!(format_remaining(-4.0), "0:00");
    }
}
