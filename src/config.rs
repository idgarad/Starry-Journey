//! Launch options from the command line (Rebuild Prompt, "Configuration And
//! Launcher", native equivalent).
//!
//! For now this covers the Starry Journey extension `--pixel-size N` plus
//! `--help`. The spec's options (`--speed`, `--palette`, `--timer`, ...)
//! join this parser in the launch-configuration feature. Invalid input is
//! reported as an error message; the binary exits with code 2.

use crate::model::grid::CellScaling;
use crate::model::rng::js_round;

/// Allowed `--pixel-size` values, in logical (unscaled) pixels.
pub const PIXEL_SIZE_RANGE: (u32, u32) = (1, 16);

/// Command-line help text.
pub const USAGE: &str = "\
Usage: starry-journey [options]

Options:
  --pixel-size N   Draw each virtual pixel as N screen pixels (1-16), times the
                   display scaling, so bigger screens show more city and sky.
                   Default: about 240 virtual rows on every screen.
  -h, --help       Show this help.

Keys: P palette, R new city, +/- speed, F fullscreen, Tab next display, Esc quit.";

/// Parsed launch options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LaunchOptions {
    /// Fixed virtual pixel size in logical pixels, if requested.
    pub pixel_size: Option<u32>,
    /// True if help was requested.
    pub help: bool,
}

impl LaunchOptions {
    /// The grid cell scaling for a window with this display scale factor.
    /// A fixed size is multiplied by the scale factor and rounded, never
    /// below one device pixel.
    pub fn cell_scaling(&self, scale_factor: f64) -> CellScaling {
        match self.pixel_size {
            None => CellScaling::Spec,
            Some(size) => {
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
/// Returns a message for unknown options, a missing or non-numeric value,
/// a value outside [`PIXEL_SIZE_RANGE`], or a repeated option.
pub fn parse_args<I>(args: I) -> Result<LaunchOptions, String>
where
    I: IntoIterator<Item = String>,
{
    let mut options = LaunchOptions::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) => (name.to_string(), Some(value.to_string())),
            None => (arg.clone(), None),
        };
        match name.as_str() {
            "-h" | "--help" if inline.is_none() => options.help = true,
            "--pixel-size" => {
                if options.pixel_size.is_some() {
                    return Err("--pixel-size was given more than once".to_string());
                }
                let value = inline
                    .or_else(|| args.next())
                    .ok_or("--pixel-size needs a value, for example --pixel-size 4")?;
                options.pixel_size = Some(parse_pixel_size(&value)?);
            }
            _ => return Err(format!("unknown option '{arg}'")),
        }
    }
    Ok(options)
}

fn parse_pixel_size(value: &str) -> Result<u32, String> {
    let (lo, hi) = PIXEL_SIZE_RANGE;
    value
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|size| (lo..=hi).contains(size))
        .ok_or_else(|| {
            format!("--pixel-size must be a whole number from {lo} to {hi}, got '{value}'")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<LaunchOptions, String> {
        parse_args(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn no_arguments_mean_spec_scaling() {
        let options = parse(&[]).unwrap();
        assert_eq!(options, LaunchOptions::default());
        assert_eq!(options.cell_scaling(1.5), CellScaling::Spec);
    }

    #[test]
    fn pixel_size_accepts_separate_and_inline_values() {
        assert_eq!(parse(&["--pixel-size", "4"]).unwrap().pixel_size, Some(4));
        assert_eq!(parse(&["--pixel-size=3"]).unwrap().pixel_size, Some(3));
        assert_eq!(parse(&["--pixel-size", "16"]).unwrap().pixel_size, Some(16));
        assert_eq!(parse(&["--pixel-size", "1"]).unwrap().pixel_size, Some(1));
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
        ] {
            assert!(parse(args).is_err(), "{args:?} should fail");
        }
        assert!(parse(&["--pixel-size", "0"])
            .unwrap_err()
            .contains("1 to 16"));
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
