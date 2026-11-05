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
use crate::lightning::LightningMode;
use crate::message::parse_message;
use crate::model::building::BuildingProfile;
use crate::model::grid::CellScaling;
use crate::model::palette::{palette_by_key, palettes};
use crate::model::rng::js_round;
use crate::moon::{parse_moon_phase, START_PHASE_STEP};
use crate::searchlights::SearchlightMode;
use crate::snapshot::DEFAULT_SNAPSHOT_DIR;
use crate::station::StationFate;
use crate::weather::PrecipMode;

/// Allowed fixed `--pixel-size` values, in logical (unscaled) pixels.
pub const PIXEL_SIZE_RANGE: (u32, u32) = (1, 16);
/// Default virtual pixel size, chosen by the owner after measuring on both
/// of their systems (2026-11-03).
pub const DEFAULT_PIXEL_SIZE: u32 = 3;
/// Snapshot size when `--size` is not given: the 1080p test screen.
pub const DEFAULT_SNAPSHOT_SIZE: (u32, u32) = (1920, 1080);
/// Snapshot sizes allowed by `--size`, per side.
pub const SNAPSHOT_SIZE_RANGE: (u32, u32) = (16, 8192);
/// The most moments one `--snapshot` run may save.
pub const MAX_SNAPSHOTS: usize = 200;

/// Command-line help text.
pub const USAGE: &str = "\
Usage: starry-journey [options]

Options:
  --buildings B      Building kinds: all (default), or baseline: only the 19
                     kinds shared with the Python version, at its widths, for
                     comparing the two.
  --clouds C         Cloud cover: clear, puffs, scattered (default), broken,
                     overcast, or storm. Press C to cycle it.
  --endless          Endless mode: the city is built, then scrolls forever in
                     four parallax layers. Press E to switch modes any time.
  --palette KEY      Start in palette KEY: classic (default), artdeco,
                     blackonwhite, cga, cga2, ega, vga, c64, appleii,
                     gameboy, amber, green. Press P to cycle.
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
  --lightning L      Lightning in storms: auto (default) or off. Flashes are
                     soft and capped well under three a second.
  --message TEXT     Show TEXT in a box over the city (at most 240 UTF-16
                     units; quote it). Blank text shows no box. Changing the
                     palette shows its name there for 2 seconds.
  --precip P         What falls in showers from overcast (10% chance every
                     10 minutes) and storm clouds (60%): rain (default),
                     snow, or off. Press W to cycle it.
  --moon-phase P     Phase the moon opens on: new, waxing-crescent,
                     first-quarter, waxing-gibbous, full, waning-gibbous,
                     last-quarter, waning-crescent (or 0-7). It then advances
                     one phase per 10-minute lap. Default: waxing-crescent.
  --station-fate F   On the new-moon lap a battle station is under attack:
                     chance (30% it is destroyed, default), destroy (blows up
                     about 15 s after opening on that lap), or survive.
  --searchlights S   Ground searchlights: auto (now and then a show of two or
                     three sweeping beams for up to 2 minutes, very rarely in
                     Endless; default), always, or off.
  --events E@T,...   Fire test events at moments T (durations as for
                     --timer): meteor, police, utility, strike, bolt,
                     flicker, shower. For example
                     --events police@30s,meteor@45s. The same as the Shift
                     test keys, for snapshots.
  --seed N           Build city number N (0-4294967295) instead of a random
                     one. The same seed gives the same city and clouds.
  --snapshot T,...   Open no window: run the app in simulated time and save
                     a PNG of each moment T (durations as for --timer, or
                     0), for example --snapshot 0,30s,12m. For checking
                     visual work; the run waits for every cloud keyframe so
                     the same seed gives the same pictures.
  --size WxH         Snapshot size in pixels (default 1920x1080).
  --snapshot-dir D   Folder for snapshot runs (default target/snapshots);
                     each run gets its own YYYYMMDD-HHMMSS folder.
  -h, --help         Show this help.

Keys: P palette, C clouds, W rain/snow; tests: Shift+S shooting star,
Shift+E police car, Shift+U utility truck, Shift+L lightning strike, Shift+B
cloud-to-cloud bolt, Shift+I in-cloud flicker, Shift+R shower; R new city, +/- speed, E Normal/Endless, Space pause,
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
    /// What falls from heavy cloud (`--precip`).
    pub precip: PrecipMode,
    /// Whether storms have lightning (`--lightning`).
    pub lightning: LightningMode,
    /// The palette to start in (`--palette`); `None` is the default.
    pub palette: Option<String>,
    /// Which building kinds and footprints (`--buildings`).
    pub buildings: BuildingProfile,
    /// When ground searchlights appear (`--searchlights`).
    pub searchlights: SearchlightMode,
    /// Test events to fire at moments of the run (`--events`), in order.
    pub events: Vec<(TestEvent, f64)>,
    /// The city seed (`--seed`); `None` picks a random one.
    pub seed: Option<u32>,
    /// Snapshot mode (`--snapshot`): the moments to save, in seconds,
    /// ascending; `None` opens the window as usual.
    pub snapshot: Option<Vec<f64>>,
    /// Snapshot size in pixels (`--size`).
    pub size: (u32, u32),
    /// Folder for snapshot runs (`--snapshot-dir`).
    pub snapshot_dir: String,
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
            buildings: BuildingProfile::default(),
            palette: None,
            lightning: LightningMode::default(),
            precip: PrecipMode::default(),
            searchlights: SearchlightMode::default(),
            events: Vec::new(),
            seed: None,
            snapshot: None,
            size: DEFAULT_SNAPSHOT_SIZE,
            snapshot_dir: DEFAULT_SNAPSHOT_DIR.to_string(),
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
    let mut size_given = false;
    let mut dir_given = false;
    let mut searchlights_given = false;
    let mut buildings_given = false;
    let mut lightning_given = false;
    let mut precip_given = false;
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
            "--precip" => {
                if precip_given {
                    return Err("--precip was given more than once".to_string());
                }
                precip_given = true;
                let value = inline
                    .or_else(|| args.next())
                    .ok_or("--precip needs rain, snow, or off")?;
                options.precip =
                    PrecipMode::parse(&value).map_err(|message| format!("--precip {message}"))?;
            }
            "--lightning" => {
                if lightning_given {
                    return Err("--lightning was given more than once".to_string());
                }
                lightning_given = true;
                let value = inline
                    .or_else(|| args.next())
                    .ok_or("--lightning needs auto or off")?;
                options.lightning = LightningMode::parse(&value)
                    .map_err(|message| format!("--lightning {message}"))?;
            }
            "--buildings" => {
                if buildings_given {
                    return Err("--buildings was given more than once".to_string());
                }
                buildings_given = true;
                let value = inline
                    .or_else(|| args.next())
                    .ok_or("--buildings needs all or baseline")?;
                options.buildings = BuildingProfile::parse(&value)
                    .map_err(|message| format!("--buildings {message}"))?;
            }
            "--searchlights" => {
                if searchlights_given {
                    return Err("--searchlights was given more than once".to_string());
                }
                searchlights_given = true;
                let value = inline
                    .or_else(|| args.next())
                    .ok_or("--searchlights needs auto, always, or off")?;
                options.searchlights = SearchlightMode::parse(&value)
                    .map_err(|message| format!("--searchlights {message}"))?;
            }
            "--events" => {
                if !options.events.is_empty() {
                    return Err("--events was given more than once".to_string());
                }
                let value = inline
                    .or_else(|| args.next())
                    .ok_or("--events needs events, for example --events police@30s")?;
                options.events = parse_events(&value)?;
            }
            "--palette" => {
                if options.palette.is_some() {
                    return Err("--palette was given more than once".to_string());
                }
                let value = inline
                    .or_else(|| args.next())
                    .ok_or("--palette needs a palette, for example --palette vga")?;
                let key = value.trim().to_ascii_lowercase();
                if palette_by_key(&key).is_none() {
                    let keys: Vec<&str> = palettes().iter().map(|p| p.key).collect();
                    return Err(format!(
                        "--palette must be one of {}, got '{value}'",
                        keys.join(", ")
                    ));
                }
                options.palette = Some(key);
            }
            "--seed" => {
                if options.seed.is_some() {
                    return Err("--seed was given more than once".to_string());
                }
                let value = inline
                    .or_else(|| args.next())
                    .ok_or("--seed needs a number, for example --seed 42")?;
                options.seed = Some(value.trim().parse::<u32>().map_err(|_| {
                    format!("--seed must be a whole number from 0 to 4294967295, got '{value}'")
                })?);
            }
            "--snapshot" => {
                if options.snapshot.is_some() {
                    return Err("--snapshot was given more than once".to_string());
                }
                let value = inline
                    .or_else(|| args.next())
                    .ok_or("--snapshot needs moments, for example --snapshot 0,30s,12m")?;
                options.snapshot = Some(parse_snapshot_times(&value)?);
            }
            "--size" => {
                if size_given {
                    return Err("--size was given more than once".to_string());
                }
                size_given = true;
                let value = inline
                    .or_else(|| args.next())
                    .ok_or("--size needs a size, for example --size 1920x1080")?;
                options.size = parse_size(&value)?;
            }
            "--snapshot-dir" => {
                if dir_given {
                    return Err("--snapshot-dir was given more than once".to_string());
                }
                dir_given = true;
                let value = inline
                    .or_else(|| args.next())
                    .ok_or("--snapshot-dir needs a folder")?;
                if value.trim().is_empty() {
                    return Err("--snapshot-dir needs a folder".to_string());
                }
                options.snapshot_dir = value;
            }
            _ => return Err(format!("unknown option '{arg}'")),
        }
    }
    if options.snapshot.is_none() && (size_given || dir_given) {
        return Err("--size and --snapshot-dir only apply with --snapshot".to_string());
    }
    Ok(options)
}

/// A test event (the Shift test keys, or `--events`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestEvent {
    /// A shooting star (Shift+S).
    Meteor,
    /// A police car (Shift+E).
    Police,
    /// A utility truck (Shift+U).
    Utility,
    /// A lightning strike on the tallest point (Shift+L).
    Strike,
    /// A cloud-to-cloud bolt (Shift+B).
    Bolt,
    /// An in-cloud flicker (Shift+I).
    Flicker,
    /// A shower, easing in at once (Shift+R).
    Shower,
}

/// Parses a moment of a run for `--snapshot` and `--events`: `0`, a
/// duration as for `--timer`, or decimal seconds such as `30.1s` (a flash
/// lasts well under a second, so frames just after an event need them).
fn parse_moment(text: &str) -> Result<f64, String> {
    let text = text.trim();
    if text == "0" {
        return Ok(0.0);
    }
    if let Some(number) = text.strip_suffix('s').filter(|n| n.contains('.')) {
        return number
            .parse::<f64>()
            .ok()
            .filter(|seconds| seconds.is_finite() && *seconds >= 0.0 && *seconds <= 86_400.0)
            .ok_or_else(|| format!("'{text}' is not a moment; use 30s, 1m30s, or 30.5s"));
    }
    parse_duration(text)
}

/// Parses `--events`: comma-separated `name@moment`, sorted by moment.
fn parse_events(value: &str) -> Result<Vec<(TestEvent, f64)>, String> {
    let mut events = Vec::new();
    for part in value.split(',') {
        let (name, at) = part.trim().split_once('@').ok_or_else(|| {
            format!("--events: '{part}' should be name@moment, for example police@30s")
        })?;
        let event = match name.trim().to_ascii_lowercase().as_str() {
            "meteor" => TestEvent::Meteor,
            "police" => TestEvent::Police,
            "utility" => TestEvent::Utility,
            "strike" => TestEvent::Strike,
            "bolt" => TestEvent::Bolt,
            "flicker" => TestEvent::Flicker,
            "shower" => TestEvent::Shower,
            _ => {
                return Err(format!(
                    "--events: unknown event '{name}' (meteor, police, utility, strike, bolt, flicker, shower)"
                ))
            }
        };
        let at = at.trim();
        let seconds = parse_moment(at).map_err(|message| format!("--events: {message}"))?;
        events.push((event, seconds));
    }
    events.sort_by(|a, b| a.1.total_cmp(&b.1));
    Ok(events)
}

/// Parses `--snapshot` moments: comma-separated durations (as for
/// `--timer`) or `0`, returned in seconds, sorted, without repeats.
fn parse_snapshot_times(value: &str) -> Result<Vec<f64>, String> {
    let mut times = Vec::new();
    for part in value.split(',') {
        let part = part.trim();
        let seconds = parse_moment(part).map_err(|message| format!("--snapshot: {message}"))?;
        times.push(seconds);
    }
    times.sort_by(f64::total_cmp);
    times.dedup();
    if times.len() > MAX_SNAPSHOTS {
        return Err(format!(
            "--snapshot takes at most {MAX_SNAPSHOTS} moments, got {}",
            times.len()
        ));
    }
    Ok(times)
}

/// Parses `--size WxH`, each side within [`SNAPSHOT_SIZE_RANGE`].
fn parse_size(value: &str) -> Result<(u32, u32), String> {
    let (lo, hi) = SNAPSHOT_SIZE_RANGE;
    let invalid =
        || format!("--size must be WIDTHxHEIGHT with each side {lo} to {hi}, got '{value}'");
    let (w, h) = value
        .trim()
        .to_ascii_lowercase()
        .split_once('x')
        .map(|(w, h)| (w.to_string(), h.to_string()))
        .ok_or_else(invalid)?;
    let side = |text: &str| {
        text.trim()
            .parse::<u32>()
            .ok()
            .filter(|n| (lo..=hi).contains(n))
            .ok_or_else(invalid)
    };
    Ok((side(&w)?, side(&h)?))
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

    #[test]
    fn palette_option_picks_a_known_palette() {
        assert_eq!(parse(&[]).unwrap().palette, None);
        assert_eq!(
            parse(&["--palette", "BlackOnWhite"])
                .unwrap()
                .palette
                .as_deref(),
            Some("blackonwhite")
        );
        assert!(parse(&["--palette", "sepia"]).is_err());
        assert!(parse(&["--palette"]).is_err());
        assert!(parse(&["--palette", "vga", "--palette", "ega"]).is_err());
    }

    #[test]
    fn events_parse_in_time_order_and_reject_bad_ones() {
        let fractions = parse(&["--snapshot", "30.1s,0,2m", "--events", "strike@29.95s"]).unwrap();
        assert_eq!(fractions.snapshot, Some(vec![0.0, 30.1, 120.0]));
        assert_eq!(fractions.events, vec![(TestEvent::Strike, 29.95)]);
        assert!(parse(&["--snapshot", "-1.5s"]).is_err());
        assert!(parse(&["--snapshot", "1.5.5s"]).is_err());
        let lightning = parse(&["--events", "strike@1s,bolt@2s,flicker@3s,shower@4s"]).unwrap();
        assert_eq!(
            lightning.events.iter().map(|e| e.0).collect::<Vec<_>>(),
            vec![
                TestEvent::Strike,
                TestEvent::Bolt,
                TestEvent::Flicker,
                TestEvent::Shower
            ]
        );
        let options = parse(&["--events", "meteor@45s, police@30s,utility@1m"]).unwrap();
        assert_eq!(
            options.events,
            vec![
                (TestEvent::Police, 30.0),
                (TestEvent::Meteor, 45.0),
                (TestEvent::Utility, 60.0)
            ]
        );
        assert!(parse(&[]).unwrap().events.is_empty());
        for bad in [
            &["--events"][..],
            &["--events", "police"],
            &["--events", "parade@30s"],
            &["--events", "police@soon"],
            &["--events", "police@1m", "--events", "meteor@2m"],
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn precip_option_parses_and_defaults_to_rain() {
        assert_eq!(parse(&[]).unwrap().precip, PrecipMode::Rain);
        assert_eq!(
            parse(&["--precip", "snow"]).unwrap().precip,
            PrecipMode::Snow
        );
        assert_eq!(parse(&["--precip=off"]).unwrap().precip, PrecipMode::Off);
        assert!(parse(&["--precip"]).is_err());
        assert!(parse(&["--precip", "sleet"]).is_err());
        assert!(parse(&["--precip", "rain", "--precip", "snow"]).is_err());
    }

    #[test]
    fn lightning_option_parses_and_defaults_to_auto() {
        assert_eq!(parse(&[]).unwrap().lightning, LightningMode::Auto);
        assert_eq!(
            parse(&["--lightning", "off"]).unwrap().lightning,
            LightningMode::Off
        );
        assert!(parse(&["--lightning"]).is_err());
        assert!(parse(&["--lightning", "on"]).is_err());
        assert!(parse(&["--lightning", "off", "--lightning=off"]).is_err());
    }

    #[test]
    fn buildings_option_parses_and_defaults_to_all() {
        assert_eq!(parse(&[]).unwrap().buildings, BuildingProfile::All);
        assert_eq!(
            parse(&["--buildings", "baseline"]).unwrap().buildings,
            BuildingProfile::Baseline
        );
        assert!(parse(&["--buildings"]).is_err());
        assert!(parse(&["--buildings", "python"]).is_err());
        assert!(parse(&["--buildings", "all", "--buildings", "all"]).is_err());
    }

    #[test]
    fn searchlight_option_parses_and_defaults_to_auto() {
        assert_eq!(parse(&[]).unwrap().searchlights, SearchlightMode::Auto);
        assert_eq!(
            parse(&["--searchlights", "always"]).unwrap().searchlights,
            SearchlightMode::Always
        );
        assert_eq!(
            parse(&["--searchlights=off"]).unwrap().searchlights,
            SearchlightMode::Off
        );
        assert!(parse(&["--searchlights"]).is_err());
        assert!(parse(&["--searchlights", "loud"]).is_err());
        assert!(parse(&["--searchlights", "off", "--searchlights", "auto"]).is_err());
    }

    #[test]
    fn snapshot_options_parse_moments_size_and_seed() {
        let options = parse(&[
            "--snapshot",
            "12m,0,30s,30s",
            "--size",
            "640x360",
            "--seed",
            "42",
        ])
        .unwrap();
        assert_eq!(options.snapshot, Some(vec![0.0, 30.0, 720.0]));
        assert_eq!(options.size, (640, 360));
        assert_eq!(options.seed, Some(42));
        assert_eq!(options.snapshot_dir, DEFAULT_SNAPSHOT_DIR);
        let default = parse(&[]).unwrap();
        assert_eq!(default.snapshot, None);
        assert_eq!(default.seed, None);
        assert_eq!(default.size, DEFAULT_SNAPSHOT_SIZE);
        let dir = parse(&["--snapshot=1m", "--snapshot-dir", "frames"]).unwrap();
        assert_eq!(dir.snapshot_dir, "frames");
    }

    #[test]
    fn snapshot_options_reject_bad_values() {
        for bad in [
            &["--snapshot"][..],
            &["--snapshot", "soon"],
            &["--snapshot", "1m,,2m"],
            &["--snapshot", "1m", "--snapshot", "2m"],
            &["--size", "640x360"],
            &["--snapshot-dir", "x"],
            &["--snapshot", "1m", "--size", "640"],
            &["--snapshot", "1m", "--size", "8x8"],
            &["--snapshot", "1m", "--size", "9000x100"],
            &["--snapshot", "1m", "--snapshot-dir", " "],
            &["--seed", "-1"],
            &["--seed", "4294967296"],
            &["--seed", "1", "--seed", "2"],
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        let many: Vec<String> = (1..=201).map(|s| format!("{s}s")).collect();
        assert!(parse(&["--snapshot", &many.join(",")]).is_err());
    }

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
