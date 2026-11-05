//! City ambience after construction (Rebuild Prompt: "after completion
//! windows flicker and landmark lights animate"; twinkles and shooting
//! stars). Owner decision, 2026-11-05: in both modes, not Normal only.
//!
//! - **Window flicker:** Poisson toggles at [`FLICKER_RATE`] per building per
//!   second times speed, each steered toward the building's target lit
//!   count (Rebuild Prompt).
//! - **Beacons:** each mast's beacon is lit for [`BEACON_ON`] of its own
//!   seeded period.
//! - **Landmark lights:** `Cycle` outlines shift hue together; `Chase`
//!   outlines carry a hue that travels along the outline.
//! - **Twinkles:** [`TWINKLE_SHARE`] of the stars now and then brighten and
//!   dim.
//! - **Shooting stars:** the first after 4..12 s, then every 8..28 s
//!   (Rebuild Prompt), each a short streak with a fading trail.
//!
//! This module decides states and colors; the renderer patches the cells
//! that change, and the shader draws shooting stars.

use crate::model::building::{Building, Decoration, LightMode, Mast};
use crate::model::rng::Mulberry32;

/// Window toggles per building per second at 1x.
pub const FLICKER_RATE: f64 = 0.02;
/// How strongly a toggle steers toward the target lit count: the chance a
/// building with too few lit windows turns one on (and with too many, off).
pub const FLICKER_STEER: f64 = 0.8;
/// Share of each beacon's period it is lit.
pub const BEACON_ON: f64 = 0.35;
/// Degrees per second a `Cycle` outline's hue turns, and a `Chase`
/// outline's hue travels.
pub const CYCLE_SPEED: f64 = 36.0;
/// Degrees of hue spread along a `Chase` outline.
pub const CHASE_SPREAD: f64 = 360.0;
/// Share of stars that twinkle (Rebuild Prompt).
pub const TWINKLE_SHARE: f64 = 0.04;
/// Shooting-star waits in seconds: before the first, and between later
/// ones (Rebuild Prompt).
pub const FIRST_METEOR_WAIT: (f64, f64) = (4.0, 12.0);
pub const METEOR_WAIT: (f64, f64) = (8.0, 28.0);
/// Seconds a shooting star takes to cross.
pub const METEOR_SECONDS: f64 = 0.9;

/// Whether `mast`'s beacon is lit at `seconds`.
pub fn beacon_lit(mast: &Mast, seconds: f64) -> bool {
    let period = mast.beacon_period.max(0.1);
    ((seconds + mast.beacon_phase) / period).rem_euclid(1.0) < BEACON_ON
}

/// The hue (degrees, 0..360) of point `index` of `decoration` at
/// `seconds`.
pub fn decoration_hue(decoration: &Decoration, index: usize, seconds: f64) -> f64 {
    let turn = CYCLE_SPEED * seconds;
    let hue = match decoration.mode {
        LightMode::Cycle => decoration.hue + turn,
        LightMode::Chase => {
            let points = (decoration.points.len() / 2).max(1) as f64;
            decoration.hue + CHASE_SPREAD * index as f64 / points - turn
        }
    };
    hue.rem_euclid(360.0)
}

/// Whether star `index` of the run `seed` twinkles.
pub fn twinkles(seed: u32, index: usize) -> bool {
    unit_hash(seed ^ 0x7a1e_5ea5, index as u32) < TWINKLE_SHARE
}

/// Star `index`'s fade toward the background at `seconds` (0 brightest):
/// its own `fade`, or for a twinkling star a slow swell between its own
/// fade and nearly gone, with its own seeded pace. O(1).
pub fn star_fade(seed: u32, index: usize, fade: f64, seconds: f64) -> f64 {
    if !twinkles(seed, index) {
        return fade;
    }
    let pace = 1.5 + 2.5 * unit_hash(seed ^ 0x51ab, index as u32);
    let phase = unit_hash(seed ^ 0xfade, index as u32) * std::f64::consts::TAU;
    let swell = 0.5 + 0.5 * (std::f64::consts::TAU * seconds / pace + phase).sin();
    fade + (0.85 - fade).max(0.0) * swell * swell
}

fn unit_hash(seed: u32, n: u32) -> f64 {
    let mut h = seed ^ n.wrapping_mul(0x9e37_79b9);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    f64::from(h) / 4_294_967_296.0
}

/// Window flicker: seeded Poisson toggles steered toward each building's
/// target lit count.
#[derive(Debug, Clone)]
pub struct Flicker {
    rng: Mulberry32,
}

impl Flicker {
    /// A flicker stream for the run `seed`.
    pub fn new(seed: u32) -> Self {
        Self {
            rng: Mulberry32::new(seed ^ 0xf11c_6e12),
        }
    }

    /// Runs `dt` seconds at `speed` over `buildings`, toggling windows.
    /// Returns the `(building, window)` pairs that changed. Each building
    /// toggles with chance `FLICKER_RATE × speed × dt`; a building with
    /// fewer lit windows than its target turns one on [`FLICKER_STEER`] of
    /// the time (with more, off), otherwise either way. O(buildings).
    pub fn step(&mut self, buildings: &mut [Building], dt: f64, speed: f64) -> Vec<(usize, usize)> {
        let chance = FLICKER_RATE * speed.max(0.0) * dt.max(0.0);
        let mut changed = Vec::new();
        if chance.is_nan() || chance <= 0.0 {
            return changed;
        }
        for (b, building) in buildings.iter_mut().enumerate() {
            if building.windows.is_empty() || self.rng.next_f64() >= chance {
                continue;
            }
            let windows = &mut building.windows;
            let lit = windows.lit.iter().filter(|&&on| on).count();
            let target = windows.target.iter().filter(|&&on| on).count();
            let up = match lit.cmp(&target) {
                std::cmp::Ordering::Less => FLICKER_STEER,
                std::cmp::Ordering::Greater => 1.0 - FLICKER_STEER,
                std::cmp::Ordering::Equal => 0.5,
            };
            let turn_on = self.rng.next_f64() < up;
            let candidates: Vec<usize> = (0..windows.len())
                .filter(|&w| windows.lit[w] != turn_on)
                .collect();
            if candidates.is_empty() {
                continue;
            }
            let w = candidates
                [(self.rng.next_f64() * candidates.len() as f64) as usize % candidates.len()];
            windows.lit[w] = turn_on;
            changed.push((b, w));
        }
        changed
    }
}

/// A shooting star in flight: where it starts and heads (shares of the
/// screen width and the sky's height), and how far along it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Meteor {
    pub from: (f64, f64),
    pub to: (f64, f64),
    /// 0 at the start, 1 at the end of its flight.
    pub progress: f64,
}

/// The `n`th shooting star of the run `seed` (0 first): when it starts and
/// where it goes.
fn meteor(seed: u32, n: u32, start: f64) -> (f64, Meteor) {
    let mut rng = Mulberry32::new(seed ^ 0x03e7_e0e5 ^ n.wrapping_mul(0x85eb_ca6b));
    let mut next = || rng.next_f64();
    // Anywhere across the upper sky, either way, from a shallow streak to
    // a steep fall (owner, 2026-11-05: "from random locations and
    // directions").
    let from = (0.05 + 0.9 * next(), 0.02 + 0.45 * next());
    let side = if next() < 0.5 { -1.0 } else { 1.0 };
    let length = 0.12 + 0.18 * next();
    let angle = (10.0 + 60.0 * next()).to_radians();
    let to = (
        from.0 + side * length * angle.cos(),
        from.1 + length * angle.sin() * 1.8,
    );
    (
        start,
        Meteor {
            from,
            to,
            progress: 0.0,
        },
    )
}

/// The shooting star in flight at `seconds`, if any. `forced` is the start
/// of one asked for by hand (Shift+S), which takes precedence. O(stars so
/// far): the schedule is walked from the start of the run.
pub fn meteor_at(seed: u32, seconds: f64, forced: Option<f64>) -> Option<Meteor> {
    if !seconds.is_finite() {
        return None;
    }
    let within = |start: f64, m: Meteor| {
        let t = (seconds - start) / METEOR_SECONDS;
        (0.0..1.0)
            .contains(&t)
            .then_some(Meteor { progress: t, ..m })
    };
    if let Some(start) = forced {
        // Each forced star draws its own place and direction from the
        // moment it was asked for.
        let n = (start.to_bits() ^ (start.to_bits() >> 32)) as u32;
        if let Some(m) = within(start, meteor(seed ^ 0xf0ce_d000, n, start).1) {
            return Some(m);
        }
    }
    let mut rng = Mulberry32::new(seed ^ 0x5707_ca11);
    let mut wait = |range: (f64, f64)| range.0 + (range.1 - range.0) * rng.next_f64();
    let mut start = wait(FIRST_METEOR_WAIT);
    let mut n = 0;
    while start <= seconds {
        if let Some(m) = within(start, meteor(seed, n, start).1) {
            return Some(m);
        }
        start += METEOR_SECONDS + wait(METEOR_WAIT);
        n += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::building::{BuildingKind, DecorationKind, Tier, WindowSet};

    fn building(windows: usize, lit: usize, target: usize) -> Building {
        let mut set = WindowSet::default();
        for w in 0..windows {
            set.push(w as i32, 1, 0, 0, w < target);
        }
        for w in 0..lit {
            set.lit[w] = true;
        }
        Building {
            layer: 0,
            x0: 0,
            x1: 10,
            kind: BuildingKind::Box,
            tiers: vec![Tier::new(0, 10, 0, 10).unwrap()],
            body_index: 0,
            tone: 0,
            accent_roll: 0.0,
            windows: set,
            mast: None,
            decoration: None,
        }
    }

    #[test]
    fn windows_flicker_at_the_spec_rate_and_steer_toward_their_target() {
        let mut buildings: Vec<Building> = (0..100).map(|_| building(20, 0, 12)).collect();
        let mut flicker = Flicker::new(3);
        let mut toggles = 0;
        for _ in 0..(600 * 30) {
            toggles += flicker.step(&mut buildings, 1.0 / 30.0, 1.0).len();
        }
        // 100 buildings x 0.02/s x 600 s = 1200 expected.
        assert!((1050..1350).contains(&toggles), "{toggles}");
        // Starting all dark, they climb toward their target of 12 and then
        // stay around it (about 12 toggles a building per 10 minutes).
        for _ in 0..(3000 * 30) {
            flicker.step(&mut buildings, 1.0 / 30.0, 1.0);
        }
        let mean = buildings
            .iter()
            .map(|b| b.windows.lit.iter().filter(|&&on| on).count())
            .sum::<usize>() as f64
            / 100.0;
        assert!((10.0..14.0).contains(&mean), "{mean}");
        // Faster speed, more toggles; paused, none.
        let mut flicker = Flicker::new(3);
        assert!(flicker.step(&mut buildings, 1.0, 0.0).is_empty());
        let fast: usize = (0..300)
            .map(|_| flicker.step(&mut buildings, 1.0 / 30.0, 4.0).len())
            .sum();
        assert!(fast > 50, "{fast}");
    }

    #[test]
    fn beacons_blink_on_their_own_period() {
        let mast = Mast {
            x: 0,
            h0: 0,
            h1: 5,
            beacon_period: 2.0,
            beacon_phase: 0.5,
        };
        let lit: Vec<bool> = (0..200)
            .map(|i| beacon_lit(&mast, f64::from(i) * 0.01))
            .collect();
        let share = lit.iter().filter(|&&on| on).count() as f64 / 200.0;
        assert!((share - BEACON_ON).abs() < 0.02);
        assert_eq!(beacon_lit(&mast, 0.0), beacon_lit(&mast, 2.0), "periodic");
    }

    #[test]
    fn landmark_lights_cycle_and_chase() {
        let mut decoration = Decoration {
            kind: DecorationKind::Crown,
            mode: LightMode::Cycle,
            hue: 100.0,
            points: (0..20).collect(),
        };
        assert_eq!(decoration_hue(&decoration, 0, 0.0), 100.0);
        assert_eq!(
            decoration_hue(&decoration, 0, 1.0),
            decoration_hue(&decoration, 9, 1.0),
            "together"
        );
        assert!((decoration_hue(&decoration, 0, 1.0) - 136.0).abs() < 1e-9);
        decoration.mode = LightMode::Chase;
        assert!(
            decoration_hue(&decoration, 0, 0.0) != decoration_hue(&decoration, 5, 0.0),
            "spread along"
        );
        // The pattern travels: point 1 at t matches point 0 a little later.
        let step = CHASE_SPREAD / 10.0 / CYCLE_SPEED;
        assert!(
            (decoration_hue(&decoration, 1, 1.0 + step) - decoration_hue(&decoration, 0, 1.0))
                .abs()
                < 1e-9
        );
        assert!((0.0..360.0).contains(&decoration_hue(&decoration, 3, 1e6)));
    }

    #[test]
    fn about_four_percent_of_stars_twinkle_smoothly() {
        let count = (0..10_000).filter(|&i| twinkles(5, i)).count();
        assert!((330..470).contains(&count), "{count}");
        let star = (0..10_000).find(|&i| twinkles(5, i)).unwrap();
        let steady = (0..10_000).find(|&i| !twinkles(5, i)).unwrap();
        assert_eq!(star_fade(5, steady, 0.2, 3.0), 0.2);
        let fades: Vec<f64> = (0..600)
            .map(|i| star_fade(5, star, 0.1, f64::from(i) / 60.0))
            .collect();
        assert!(fades.iter().all(|f| (0.1..=0.85).contains(f)));
        assert!(
            fades.windows(2).all(|w| (w[1] - w[0]).abs() < 0.05),
            "smooth"
        );
        assert!(
            fades.iter().copied().fold(0.0, f64::max) > 0.6,
            "dims a lot"
        );
    }

    #[test]
    fn shooting_stars_follow_the_spec_waits_and_can_be_forced() {
        // Each time a shooting star appears (none the frame before).
        let mut firsts = Vec::new();
        let mut was = false;
        for i in 0..40_000 {
            let t = f64::from(i) * 0.025;
            let now = meteor_at(2, t, None).is_some();
            if now && !was {
                firsts.push(t);
            }
            was = now;
        }
        // First within 4..12 s, then 8..28 s apart (plus the flight).
        assert!((4.0..12.1).contains(&firsts[0]), "{}", firsts[0]);
        assert!(firsts.len() > 20);
        for w in firsts.windows(2) {
            let gap = w[1] - w[0];
            assert!(
                (8.0 + METEOR_SECONDS - 0.05..28.0 + METEOR_SECONDS + 0.05).contains(&gap),
                "{gap}"
            );
        }
        assert!(meteor_at(2, 1.0, None).is_none());
        let forced = meteor_at(2, 1.3, Some(1.0)).expect("a forced shooting star");
        assert!((forced.progress - 0.3 / METEOR_SECONDS).abs() < 1e-9);
        assert!(forced.to.1 > forced.from.1, "falls");
        // Forced stars at different moments start and head differently.
        let paths: Vec<Meteor> = (0..20)
            .map(|i| {
                let start = 100.0 + 3.7 * f64::from(i);
                meteor_at(2, start + 0.1, Some(start)).unwrap()
            })
            .collect();
        let distinct = |f: fn(&Meteor) -> f64| {
            let mut values: Vec<f64> = paths.iter().map(f).collect();
            values.sort_by(f64::total_cmp);
            values.dedup();
            values.len()
        };
        assert_eq!(distinct(|m| m.from.0), 20, "different places");
        let lefts = paths.iter().filter(|m| m.to.0 < m.from.0).count();
        assert!(
            (3..=17).contains(&lefts),
            "both directions: {lefts} of 20 go left"
        );
    }
}
