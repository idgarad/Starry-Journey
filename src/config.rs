//! Launch options from the command line (Rebuild Prompt, "Configuration And
//! Launcher", native equivalent).
//!
//! For now this covers `--timer` (alias `--deadline`), `--message`, the
//! Starry Journey extensions `--pixel-size`, `--moon-phase`, and
//! `--station-fate`, and `--help`. The spec's other options
//! (`--speed`, `--palette`, `--ends-at`, ...) join this parser in the
//! launch-configuration feature. Invalid input is
//! reported as an error message; the binary exits with code 2.

use crate::clock::{parse_duration, ConstructionClock};
use crate::clouds::Cover;
use crate::message::parse_message;
use crate::model::grid::CellScaling;
use crate::model::rng::js_round;
use crate::moon::{parse_moon_phase, START_PHASE_STEP};
use crate::station::StationFate;

/// Allowed fixed `--pixel-size` values, in logical (unscaled) pixels.
pub const PIXEL_SIZE_RANGE: (u32, u32) = (1, 16);
/// Default virtual pixel size, chosen by the owner after measuring on both
/// of their systems (2026-11-03).
pub const DEFAULT_PIXEL_SIZE: u32 = 3;

/// Command-line help text.
pub const USAGE: &str = "\
Usage: starry-journey [options]

Options:
  --clouds C         Cloud cover: clear, puffs, scattered (default), broken,
                     overcast, or storm. Press C to cycle it.
  --endless          Endless mode: the city is built, then scrolls forever in
                     four parallax layers. Press E to switch modes any time.
  --pixel-size N    Draw each virtual pixel as N screen pixels (1-16), times
                     the display scaling. Bigger screens show more city and
                     sky. Default: 3.
  --pixel-size spec  Scale virtual pixels with the screen height instead, so
                     every screen shows about 240 virtual rows.
  --timer D          Finish building the city in exactly D, whatever the
                     speed. A bare number is minutes; also 90s, 25m, 1h30m.
                     1 second to 24 hours. Example: --timer 10 for a meeting
                     that starts in 10 minutes, --timer 10s for a quick test.
  --deadline D       Same as --timer.
  --message TEXT     Show TEXT in a box over the city (at most 240 UTF-16
                     units; quote it). Blank text shows no box. Changing the
                     palette shows its name there for 2 seconds.
  --moon-phase P     Phase the moon opens on: new, waxing-crescent,
                     first-quarter, waxing-gibbous, full, waning-gibbous,
                     last-quarter, waning-crescent (or 0-7). It then advances
                     one phase per 10-minute lap. Default: waxing-crescent.
  --station-fate F   On the new-moon lap a battle station is under attack:
                     chance (30% it is destroyed, default), destroy (blows up
                     about 15 s after opening on that lap), or survive.
  -h, --help         Show this help.

Keys: P palette, C clouds, R new city, +/- speed, E Normal/Endless, Space pause,
F fullscreen, Tab next display, Esc quit.";

/// How big a virtual pixel is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelSize {
    /// Rebuild Prompt formula: about 240 rows on every screen.
    Spec,
    /// A fixed size in logical pixels, multiplied by the display scale.
    Fixed(u32),
}

impl Default for PixelSize {
    fn default() -> Self {
        PixelSize::Fixed(DEFAULT_PIXEL_SIZE)
    }
}

/// Parsed launch options.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchOptions {
    /// Virtual pixel size.
    pub pixel_size: PixelSize,
    /// How construction progress advances.
    pub clock: ConstructionClock,
    /// Moon phase step on the first lap (0 new ... 7 waning crescent).
    pub moon_start_step: u64,
    /// What happens to the station on its laps.
    pub station_fate: StationFate,
    /// Text for the message box (`--message`); `None` hides the box.
    pub message: Option<String>,
    /// Start in Endless mode (`--endless`). Unlike the spec, a timer is
    /// allowed: it times the construction that precedes scrolling.
    pub endless: bool,
    /// Cloud cover preset (`--clouds`).
    pub clouds: Cover,
    /// True if help was requested.
    pub help: bool,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            pixel_size: PixelSize::default(),
            clock: ConstructionClock::default(),
            moon_start_step: START_PHASE_STEP,
            station_fate: StationFate::default(),
            message: None,
            endless: false,
            clouds: Cover::default(),
            help: false,
        }
    }
}

impl LaunchOptions {
    /// The grid cell scaling for a window with this display scale factor.
    /// A fixed size is multiplied by the scale factor and rounded, never
    /// below one device pixel.
    pub fn cell_scaling(&self, scale_factor: f64) -> CellScaling {
        match self.pixel_size {
            PixelSize::Spec => CellScaling::Spec,
            PixelSize::Fixed(size) => {
                let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
                    scale_factor
                } else {
                    1.0
                };
                CellScaling::Fixed((js_round(f64::from(size) * scale) as u32).max(1))
            }
        }
    }
}

/// Parses arguments (without the program name).
///
/// # Errors
///
/// Returns a message for unknown options, a missing or invalid value, a
/// size outside [`PIXEL_SIZE_RANGE`], or a repeated option.
pub fn parse_args<I>(args: I) -> Result<LaunchOptions, String>
where
    I: IntoIterator<Item = String>,
{
    let mut options = LaunchOptions::default();
    let mut pixel_size_given = false;
    let mut timer_given = false;
    let mut moon_given = false;
    let mut fate_given = false;
    let mut message_given = false;
    let mut clouds_given = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) => (name.to_string(), Some(value.to_string())),
            None => (arg.clone(), None),
        };
        match name.as_str() {
            "-h" | "--help" if inline.is_none() => options.help = true,
            "--endless" if inline.is_none() => {
                if options.endless {
                    return Err("--endless was given more than once".to_string());
                }
                options.endless = true;
            }
            "--pixel-size" => {
                if pixel_size_given {
                    return Err("--pixel-size was given more than once".to_string());
                }
                pixel_size_given = true;
                let value = inline
                    .or_else(|| args.next())
                    .ok_or("--pixel-size needs a value, for example --pixel-size 4")?;
                options.pixel_size = parse_pixel_size(&value)?;
            }
            "--timer" | "--deadline" => {
                if timer_given {
                    return Err("--timer/--deadline was given more than once".to_string());
                }
                timer_given = true;
                let value = inline
                    .or_else(|| args.next())
                    .ok_or_else(|| format!("{name} needs a duration, for example {name} 10m"))?;
                let seconds =
                    parse_duration(&value).map_err(|message| format!("{name}: {message}"))?;
                options.clock = ConstructionClock::Duration { seconds };
            }
            "--moon-phase" => {
                if moon_given {
                    return Err("--moon-phase was given more than once".to_string());
                }
                moon_given = true;
                let value = inline
                    .or_else(|| args.next())
                    .ok_or("--moon-phase needs a phase, for example --moon-phase full")?;
                options.moon_start_step = parse_moon_phase(&value)
                    .map_err(|message| format!("--moon-phase: {message}"))?;
            }
            "--station-fate" => {
                if fate_given {
                    return Err("--station-fate was given more than once".to_string());
                }
                fate_given = true;
                let value = inline
                    .or_else(|| args.next())
                    .ok_or("--station-fate needs chance, destroy, or survive")?;
                options.station_fate = match value.trim().to_ascii_lowercase().as_str() {
                    "chance" => StationFate::Chance,
                    "destroy" => StationFate::Destroy,
                    "survive" => StationFate::Survive,
                    _ => {
                        return Err(format!(
                            "--station-fate must be chance, destroy, or survive, got '{value}'"
                        ))
                    }
                };
            }
            "--clouds" => {
                if clouds_given {
                    return Err("--clouds was given more than once".to_string());
                }
                clouds_given = true;
                let value = inline
                    .or_else(|| args.next())
                    .ok_or("--clouds needs a cover, for example --clouds storm")?;
                options.clouds =
                    Cover::parse(&value).map_err(|message| format!("--clouds {message}"))?;
            }
            "--message" => {
                if message_given {
                    return Err("--message was given more than once".to_string());
                }
                message_given = true;
                let value = inline
                    .or_else(|| args.next())
                    .ok_or("--message needs text, for example --message \"Back soon\"")?;
                options.message = parse_message(&value)?;
            }
            _ => return Err(format!("unknown option '{arg}'")),
        }
    }
    Ok(options)
}

fn parse_pixel_size(value: &str) -> Result<PixelSize, String> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("spec") {
        return Ok(PixelSize::Spec);
    }
    let (lo, hi) = PIXEL_SIZE_RANGE;
    value
        .parse::<u32>()
        .ok()
        .filter(|size| (lo..=hi).contains(size))
        .map(PixelSize::Fixed)
        .ok_or_else(|| {
            format!(
                "--pixel-size must be 'spec' or a whole number from {lo} to {hi}, got '{value}'"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<LaunchOptions, String> {
        parse_args(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn default_is_fixed_three_pixels() {
        let options = parse(&[]).unwrap();
        assert_eq!(options, LaunchOptions::default());
        assert_eq!(options.pixel_size, PixelSize::Fixed(3));
        assert_eq!(options.cell_scaling(1.0), CellScaling::Fixed(3));
        assert_eq!(options.cell_scaling(2.0), CellScaling::Fixed(6));
    }

    #[test]
    fn spec_value_restores_screen_height_scaling() {
        for value in ["spec", "SPEC", " spec "] {
            let options = parse(&["--pixel-size", value]).unwrap();
            assert_eq!(options.pixel_size, PixelSize::Spec);
            assert_eq!(options.cell_scaling(1.5), CellScaling::Spec);
        }
        assert_eq!(
            parse(&["--pixel-size=spec"]).unwrap().pixel_size,
            PixelSize::Spec
        );
    }

    #[test]
    fn pixel_size_accepts_separate_and_inline_values() {
        assert_eq!(
            parse(&["--pixel-size", "4"]).unwrap().pixel_size,
            PixelSize::Fixed(4)
        );
        assert_eq!(
            parse(&["--pixel-size=5"]).unwrap().pixel_size,
            PixelSize::Fixed(5)
        );
        assert_eq!(
            parse(&["--pixel-size", "16"]).unwrap().pixel_size,
            PixelSize::Fixed(16)
        );
        assert_eq!(
            parse(&["--pixel-size", "1"]).unwrap().pixel_size,
            PixelSize::Fixed(1)
        );
    }

    #[test]
    fn invalid_pixel_sizes_are_rejected_with_a_message() {
        for args in [
            &["--pixel-size"][..],
            &["--pixel-size", "0"],
            &["--pixel-size", "17"],
            &["--pixel-size", "four"],
            &["--pixel-size", "-2"],
            &["--pixel-size=2.5"],
            &["--pixel-size", "3", "--pixel-size", "4"],
            &["--pixel-size", "spec", "--pixel-size", "spec"],
        ] {
            assert!(parse(args).is_err(), "{args:?} should fail");
        }
        assert!(parse(&["--pixel-size", "0"])
            .unwrap_err()
            .contains("'spec' or a whole number"));
    }

    #[test]
    fn timer_and_deadline_alias_set_a_duration_clock() {
        assert_eq!(parse(&[]).unwrap().clock, ConstructionClock::Untimed);
        let minutes = parse(&["--timer", "10"]).unwrap();
        assert_eq!(
            minutes.clock,
            ConstructionClock::Duration { seconds: 600.0 }
        );
        let seconds = parse(&["--deadline=10s"]).unwrap();
        assert_eq!(seconds.clock, ConstructionClock::Duration { seconds: 10.0 });
        let both = parse(&["--deadline", "1h30m", "--pixel-size", "4"]).unwrap();
        assert_eq!(both.clock, ConstructionClock::Duration { seconds: 5400.0 });
        assert_eq!(both.pixel_size, PixelSize::Fixed(4));
    }

    #[test]
    fn bad_or_repeated_timers_are_rejected() {
        for args in [
            &["--timer"][..],
            &["--timer", "0"],
            &["--deadline", "soon"],
            &["--timer", "25h"],
            &["--timer", "5", "--deadline", "6"],
        ] {
            assert!(parse(args).is_err(), "{args:?} should fail");
        }
        assert!(parse(&["--deadline", "soon"])
            .unwrap_err()
            .starts_with("--deadline:"));
    }

    #[test]
    fn moon_phase_option_sets_the_opening_phase() {
        assert_eq!(parse(&[]).unwrap().moon_start_step, START_PHASE_STEP);
        assert_eq!(parse(&["--moon-phase", "new"]).unwrap().moon_start_step, 0);
        assert_eq!(parse(&["--moon-phase=full"]).unwrap().moon_start_step, 4);
        assert_eq!(parse(&["--moon-phase", "6"]).unwrap().moon_start_step, 6);
        assert!(parse(&["--moon-phase"]).is_err());
        assert!(parse(&["--moon-phase", "blue"])
            .unwrap_err()
            .starts_with("--moon-phase:"));
        assert!(parse(&["--moon-phase", "new", "--moon-phase", "full"]).is_err());
    }

    #[test]
    fn station_fate_option() {
        assert_eq!(parse(&[]).unwrap().station_fate, StationFate::Chance);
        assert_eq!(
            parse(&["--station-fate", "destroy"]).unwrap().station_fate,
            StationFate::Destroy
        );
        assert_eq!(
            parse(&["--station-fate=Survive"]).unwrap().station_fate,
            StationFate::Survive
        );
        assert!(parse(&["--station-fate", "maybe"]).is_err());
        assert!(parse(&["--station-fate"]).is_err());
        assert!(parse(&["--station-fate", "chance", "--station-fate", "destroy"]).is_err());
    }

    #[test]
    fn message_option_is_trimmed_literal_text() {
        assert_eq!(parse(&[]).unwrap().message, None);
        assert_eq!(
            parse(&["--message", "  Back in 5 <b>min</b>  "])
                .unwrap()
                .message
                .as_deref(),
            Some("Back in 5 <b>min</b>")
        );
        assert_eq!(
            parse(&["--message=a=b"]).unwrap().message.as_deref(),
            Some("a=b")
        );
        assert_eq!(parse(&["--message", "   "]).unwrap().message, None);
    }

    #[test]
    fn bad_or_repeated_messages_are_rejected() {
        assert!(parse(&["--message"]).is_err());
        assert!(parse(&["--message", "a", "--message", "b"]).is_err());
        let long = "x".repeat(241);
        assert!(parse(&["--message", &long]).unwrap_err().contains("240"));
    }

    #[test]
    fn endless_flag_and_timer_may_be_combined() {
        assert!(!parse(&[]).unwrap().endless);
        let options = parse(&["--endless", "--timer", "10m"]).unwrap();
        assert!(options.endless);
        assert_eq!(
            options.clock,
            ConstructionClock::Duration { seconds: 600.0 }
        );
        assert!(parse(&["--endless", "--endless"]).is_err());
        assert!(parse(&["--endless=yes"]).is_err());
    }

    #[test]
    fn clouds_option_picks_a_cover() {
        assert_eq!(parse(&[]).unwrap().clouds, Cover::Scattered);
        assert_eq!(parse(&["--clouds", "storm"]).unwrap().clouds, Cover::Storm);
        assert_eq!(
            parse(&["--clouds=Overcast"]).unwrap().clouds,
            Cover::Overcast
        );
        assert!(parse(&["--clouds", "drizzle"])
            .unwrap_err()
            .starts_with("--clouds"));
        assert!(parse(&["--clouds"]).is_err());
        assert!(parse(&["--clouds", "clear", "--clouds", "storm"]).is_err());
    }

    #[test]
    fn unknown_options_and_help() {
        assert!(parse(&["--speed", "2"])
            .unwrap_err()
            .contains("unknown option"));
        assert!(parse(&["--help=yes"]).is_err());
        assert!(parse(&["-h"]).unwrap().help);
        assert!(parse(&["--help", "--pixel-size", "2"]).unwrap().help);
    }

    #[test]
    fn fixed_size_follows_display_scaling() {
        let options = parse(&["--pixel-size", "4"]).unwrap();
        assert_eq!(options.cell_scaling(1.0), CellScaling::Fixed(4));
        assert_eq!(options.cell_scaling(1.5), CellScaling::Fixed(6));
        assert_eq!(options.cell_scaling(1.25), CellScaling::Fixed(5));
        assert_eq!(options.cell_scaling(f64::NAN), CellScaling::Fixed(4));
        let one = parse(&["--pixel-size", "1"]).unwrap();
        assert_eq!(
            one.cell_scaling(0.4),
            CellScaling::Fixed(1),
            "never below one pixel"
        );
    }
}
