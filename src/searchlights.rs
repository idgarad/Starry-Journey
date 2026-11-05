//! Ground searchlights (owner decisions, 2026-11-04: "Beams + cloud
//! spots", from street level between buildings, "like a grand opening").
//!
//! Now and then a **show** starts: two or three searchlights at street
//! level sweep back and forth, beams crossing, for a minute or two (never
//! more than two; owner, 2026-11-05), then switch off. Whether a show runs
//! is seeded per 10-minute stretch of the run: often enough to notice in
//! Normal mode (an assumption the owner reviewed), and very rarely in
//! Endless (owner, 2026-11-05). `--searchlights always` keeps one running
//! and `off` disables them.
//!
//! When weather arrives: searchlights must never be on while it rains
//! (owner, 2026-11-05: rain and beams together would be too much hassle).
//!
//! This module only decides where the beams are; the GPU draws them (see
//! `present.wgsl`): a faint beam through the air, behind the city so the
//! buildings hide its foot, blocked by thick cloud, and a bright pool where
//! it meets a cloud.

use crate::model::rng::Mulberry32;

/// Seconds in each stretch of the run that may hold one show.
pub const SHOW_SLOT: f64 = 600.0;
/// Chance a stretch holds a show in Normal mode.
pub const SHOW_CHANCE: f64 = 0.6;
/// Chance a stretch holds a show in Endless mode: very rare, about one
/// show every hour and forty minutes.
pub const SHOW_CHANCE_ENDLESS: f64 = 0.1;
/// Shortest and longest show, in seconds, switching on and off included:
/// never more than two minutes.
pub const SHOW_SECONDS: (f64, f64) = (70.0, 120.0);
/// Seconds a light takes to switch fully on or off.
pub const SWITCH_SECONDS: f64 = 3.0;
/// The most searchlights in one show (the shader has this many slots).
pub const MAX_LIGHTS: usize = 3;

/// When searchlights appear (`--searchlights`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchlightMode {
    /// Occasional shows (default); in Endless mode see
    /// [`SearchlightMode::for_scene`].
    #[default]
    Auto,
    /// `Auto` as Endless mode uses it: very rare shows. Not a launch
    /// option.
    Rare,
    /// A show is always running.
    Always,
    /// Never.
    Off,
}

impl SearchlightMode {
    /// The mode to use for a scene: in Endless, `Auto` becomes `Rare`.
    pub fn for_scene(self, endless: bool) -> Self {
        if endless && self == Self::Auto {
            Self::Rare
        } else {
            self
        }
    }

    /// Parses `auto`, `always`, or `off` (any case).
    ///
    /// # Errors
    ///
    /// Returns a message for anything else.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "always" => Ok(Self::Always),
            "off" => Ok(Self::Off),
            _ => Err(format!("must be auto, always, or off, got '{value}'")),
        }
    }
}

/// One beam right now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Beam {
    /// World column of its foot (street level).
    pub x: f64,
    /// Lean from straight up, in radians (positive leans right).
    pub angle: f64,
    /// 0 (off) to 1 (fully on).
    pub power: f64,
}

/// A show: its time span, its lights' places (as shares of the screen
/// width) and sweeps, and the camera when it started.
#[derive(Debug, Clone, PartialEq)]
struct Show {
    start: f64,
    end: f64,
    lights: Vec<Sweep>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Sweep {
    /// Where along the screen it stands when the show starts, 0..1.
    place: f64,
    /// Largest lean, radians.
    reach: f64,
    /// Seconds per full sweep there and back.
    period: f64,
    /// Starting point of the sweep, radians.
    phase: f64,
}

/// The show stretch `slot` holds (if any) for the run `seed`. In
/// `Always` mode there is one endless show (use slot 0).
fn show_in(seed: u32, slot: i64, mode: SearchlightMode) -> Option<Show> {
    let mut rng = Mulberry32::new(seed ^ 0x5ea7_c417 ^ (slot as u32).wrapping_mul(0x9e37_79b9));
    let mut next = || rng.next_f64();
    let chance = next();
    let slot_start = slot as f64 * SHOW_SLOT;
    let (start, end) = match mode {
        SearchlightMode::Off => return None,
        SearchlightMode::Always => (-SWITCH_SECONDS, f64::INFINITY),
        SearchlightMode::Auto | SearchlightMode::Rare => {
            let threshold = if mode == SearchlightMode::Rare {
                SHOW_CHANCE_ENDLESS
            } else {
                SHOW_CHANCE
            };
            if chance >= threshold {
                return None;
            }
            let length = SHOW_SECONDS.0 + (SHOW_SECONDS.1 - SHOW_SECONDS.0) * next();
            let start = slot_start + (SHOW_SLOT - length) * next();
            (start, start + length)
        }
    };
    let count = if next() < 0.5 { 2 } else { 3 };
    // Spread across the screen, with a little jitter.
    let lights = (0..count)
        .map(|i| Sweep {
            place: (f64::from(i) + 0.3 + 0.4 * next()) / f64::from(count),
            reach: (15.0 + 20.0 * next()).to_radians(),
            period: 9.0 + 6.0 * next(),
            // Neighbours start half a sweep apart, so their beams cross.
            phase: f64::from(i) * std::f64::consts::PI + 0.6 * next(),
        })
        .collect();
    Some(Show { start, end, lights })
}

/// The show running at `seconds`, if any.
fn show_at(seed: u32, mode: SearchlightMode, seconds: f64) -> Option<Show> {
    if !seconds.is_finite() {
        return None;
    }
    let slot = match mode {
        SearchlightMode::Always => 0,
        _ => (seconds / SHOW_SLOT).floor() as i64,
    };
    show_in(seed, slot, mode).filter(|show| (show.start..=show.end).contains(&seconds))
}

/// When the show running at `seconds` started, or `None` if none is. The
/// caller remembers the view at that moment and passes it to
/// [`beams_at`] as `anchor`, so the show's lights stay put in the world.
pub fn show_start(seed: u32, mode: SearchlightMode, seconds: f64) -> Option<f64> {
    show_at(seed, mode, seconds).map(|show| show.start)
}

/// The searchlights at `seconds` into the run: their world columns, leans,
/// and power. `cols` is the screen width in cells, `view` the world column
/// at the screen's left edge now, and `anchor` the view when the show
/// started: the lights stand across the screen as it was then, so they
/// scroll with the street in Endless. In `Always` mode a light that
/// scrolls off the left comes back in on the right. O(lights).
pub fn beams_at(
    seed: u32,
    mode: SearchlightMode,
    seconds: f64,
    cols: f64,
    anchor: f64,
    view: f64,
) -> Vec<Beam> {
    let Some(show) = show_at(seed, mode, seconds) else {
        return Vec::new();
    };
    let power = smooth(
        ((seconds - show.start) / SWITCH_SECONDS).min((show.end - seconds) / SWITCH_SECONDS),
    );
    let cols = cols.max(1.0);
    show.lights
        .iter()
        .take(MAX_LIGHTS)
        .map(|light| {
            let x = anchor + light.place * cols;
            Beam {
                x: if mode == SearchlightMode::Always {
                    view + (x - view).rem_euclid(cols)
                } else {
                    x
                },
                angle: light.reach
                    * (std::f64::consts::TAU * seconds / light.period + light.phase).sin(),
                power,
            }
        })
        .collect()
}

fn smooth(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seed: u32, mode: SearchlightMode, seconds: f64) -> Vec<Beam> {
        beams_at(seed, mode, seconds, 600.0, 0.0, 0.0)
    }

    #[test]
    fn endless_shows_are_very_rare_and_never_longer_than_two_minutes() {
        let count = |mode: SearchlightMode| {
            (0..1000)
                .filter(|&seed| show_in(seed, 0, mode).is_some())
                .count()
        };
        let (normal, endless) = (count(SearchlightMode::Auto), count(SearchlightMode::Rare));
        assert!((60..=140).contains(&endless), "{endless} of 1000 stretches");
        assert!(endless * 4 < normal, "{endless} vs {normal}");
        for seed in 0..1000 {
            for mode in [SearchlightMode::Auto, SearchlightMode::Rare] {
                if let Some(show) = show_in(seed, 3, mode) {
                    assert!(show.end - show.start <= 120.0, "at most two minutes");
                }
            }
        }
        assert_eq!(SearchlightMode::Auto.for_scene(true), SearchlightMode::Rare);
        assert_eq!(
            SearchlightMode::Auto.for_scene(false),
            SearchlightMode::Auto
        );
        assert_eq!(
            SearchlightMode::Always.for_scene(true),
            SearchlightMode::Always
        );
        assert!(SearchlightMode::parse("rare").is_err());
    }

    #[test]
    fn shows_come_now_and_then_and_last_a_minute_or_two() {
        // Over many runs, about SHOW_CHANCE of the stretches hold a show,
        // each lasting 70-120 s.
        let mut shows = 0;
        for seed in 0..200 {
            let lit: Vec<bool> = (0..600)
                .map(|s| !at(seed, SearchlightMode::Auto, f64::from(s)).is_empty())
                .collect();
            let seconds = lit.iter().filter(|&&on| on).count();
            if seconds > 0 {
                shows += 1;
                assert!((69..=121).contains(&seconds), "seed {seed}: {seconds} s");
            }
        }
        assert!((90..=150).contains(&shows), "{shows} of 200 stretches");
    }

    #[test]
    fn lights_switch_on_and_off_smoothly() {
        let seed = (0..100)
            .find(|&s| show_in(s, 0, SearchlightMode::Auto).is_some())
            .unwrap();
        let show = show_in(seed, 0, SearchlightMode::Auto).unwrap();
        let power = |t: f64| {
            at(seed, SearchlightMode::Auto, t)
                .first()
                .map_or(0.0, |b| b.power)
        };
        assert!(power(show.start + 0.5) < 0.2);
        assert_eq!(power(show.start + SWITCH_SECONDS + 1.0), 1.0);
        assert!(power(show.end - 0.5) < 0.2);
        let mut previous = 0.0;
        for i in 0..30 {
            let p = power(show.start + f64::from(i) * 0.1);
            assert!(p >= previous && p - previous < 0.1, "no jumps");
            previous = p;
        }
    }

    #[test]
    fn beams_sweep_and_neighbours_cross() {
        let beams = |t: f64| at(1, SearchlightMode::Always, t);
        let first = beams(100.0);
        assert!((2..=MAX_LIGHTS).contains(&first.len()));
        assert!(first
            .iter()
            .all(|b| b.angle.abs() <= 35f64.to_radians() + 1e-9));
        // Over a sweep each beam leans both ways, and the first two lean
        // opposite ways at some moment.
        let mut crossed = false;
        let mut lean = [false; 2];
        for i in 0..300 {
            let b = beams(100.0 + f64::from(i) * 0.05);
            lean[0] |= b[0].angle > 0.1;
            lean[1] |= b[0].angle < -0.1;
            crossed |= b[0].angle * b[1].angle < -0.01;
        }
        assert!(lean[0] && lean[1] && crossed);
        // Spread across the screen.
        assert!(first.windows(2).all(|w| w[1].x > w[0].x));
        assert!(first.iter().all(|b| (0.0..600.0).contains(&b.x)));
    }

    #[test]
    fn lights_stay_put_in_the_world_while_the_view_scrolls() {
        let seed = (0..100)
            .find(|&s| show_in(s, 0, SearchlightMode::Auto).is_some())
            .unwrap();
        let start = show_start(
            seed,
            SearchlightMode::Auto,
            show_in(seed, 0, SearchlightMode::Auto).unwrap().start + 10.0,
        )
        .unwrap();
        let anchor = 2.0 * start;
        let a = beams_at(
            seed,
            SearchlightMode::Auto,
            start + 10.0,
            600.0,
            anchor,
            2.0 * (start + 10.0),
        );
        let b = beams_at(
            seed,
            SearchlightMode::Auto,
            start + 40.0,
            600.0,
            anchor,
            2.0 * (start + 40.0),
        );
        assert_eq!(a[0].x, b[0].x, "same world column");
        assert!(a
            .iter()
            .all(|beam| (anchor..anchor + 600.0).contains(&beam.x)));
        // Always: a light scrolled off the left comes back on the right.
        let far = beams_at(1, SearchlightMode::Always, 5000.0, 600.0, 0.0, 10_000.0);
        assert!(far
            .iter()
            .all(|beam| (10_000.0..10_600.0).contains(&beam.x)));
        assert_eq!(
            show_start(1, SearchlightMode::Always, 5000.0),
            Some(-SWITCH_SECONDS)
        );
        assert_eq!(show_start(1, SearchlightMode::Off, 5000.0), None);
    }

    #[test]
    fn off_never_lights_and_bad_times_are_dark() {
        assert!((0..2000).all(|s| at(3, SearchlightMode::Off, f64::from(s)).is_empty()));
        assert!(at(3, SearchlightMode::Always, f64::NAN).is_empty());
        assert!(!at(3, SearchlightMode::Always, 0.0).is_empty());
    }

    #[test]
    fn modes_parse() {
        assert_eq!(
            SearchlightMode::parse("Always"),
            Ok(SearchlightMode::Always)
        );
        assert_eq!(SearchlightMode::parse(" off "), Ok(SearchlightMode::Off));
        assert_eq!(SearchlightMode::parse("auto"), Ok(SearchlightMode::Auto));
        assert!(SearchlightMode::parse("sometimes").is_err());
    }
}
