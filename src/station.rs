//! The battle around the station on the new-moon lap (Starry Journey easter
//! egg; owner idea 2026-11-03).
//!
//! - About eight one-cell fighters swirl around the station on tilted
//!   orbits. Each lives 3 to 10 seconds, then explodes in a tiny burst, and a
//!   new fighter takes its slot on a fresh orbit. Fighters on the far side of
//!   an orbit are hidden behind the station.
//! - Each station lap has a [`DESTROY_CHANCE`] chance (or a forced fate from
//!   `--station-fate`) that the station is destroyed: a full-disk flash, then
//!   a fireball, an expanding shockwave ring, and debris, fading over about
//!   four seconds. After that the sky is empty for the rest of the lap.
//! - Credit where credit is due (owner request): on a destruction lap, a
//!   single red ship, always pure red whatever the palette, flies the length
//!   of the equatorial trench, climbs away, and a few seconds later the
//!   station explodes. No trench run, no explosion.
//!
//! Everything is a pure function of the lap number and the time within the
//! lap, seeded per lap, so a resize or palette change mid-battle changes
//! nothing.

use crate::model::palette::{Palette, Rgb};
use crate::model::rng::{js_round, Mulberry32};
use crate::raster::mix;

/// Number of fighter slots.
pub const FIGHTERS: u32 = 8;
/// Fighter lifetime range in seconds before it explodes.
pub const FIGHTER_LIFE: (f64, f64) = (3.0, 10.0);
/// Length of a fighter explosion in seconds.
pub const BURST_SECONDS: f64 = 0.6;
/// Orbit distance from the station's edge, in cells.
pub const ORBIT_GAP: (f64, f64) = (3.0, 14.0);
/// Orbit angular speed range in radians per second.
pub const ORBIT_SPEED: (f64, f64) = (0.6, 1.6);
/// Vertical squash of the orbits, so they look tilted.
pub const ORBIT_TILT: f64 = 0.45;
/// Chance the station is destroyed on a lap with fate [`StationFate::Chance`].
pub const DESTROY_CHANCE: f64 = 0.3;
/// Random destruction time as a fraction of the lap (the station is high).
pub const DESTROY_WINDOW: (f64, f64) = (0.35, 0.65);
/// Forced destruction time as a fraction of the lap: 12 seconds after the
/// app opens when it starts on the station lap.
pub const FORCED_DESTROY_AT: f64 = 0.32;
/// Length of the full-disk flash in seconds.
pub const FLASH_SECONDS: f64 = 0.25;
/// Length of the whole destruction sequence in seconds.
pub const EXPLOSION_SECONDS: f64 = 4.0;
/// Shockwave growth in cells per second.
pub const SHOCKWAVE_SPEED: f64 = 12.0;
/// Number of debris particles.
pub const DEBRIS: u32 = 30;
/// The hero ship's color: pure red in every palette.
pub const HERO_RED: Rgb = Rgb([0xff, 0x20, 0x20]);
/// Seconds the hero takes to fly the trench, edge to edge.
pub const TRENCH_RUN_SECONDS: f64 = 2.5;
/// Seconds the hero takes to climb away after the trench.
pub const ESCAPE_SECONDS: f64 = 1.5;
/// Quiet seconds between the hero leaving and the explosion.
pub const FUSE_SECONDS: f64 = 3.0;
/// Cells beyond the station's edge where the trench run starts and ends.
pub const RUN_MARGIN: f64 = 6.0;
/// How far the hero climbs away (cells right, cells up).
pub const ESCAPE_DISTANCE: (f64, f64) = (24.0, 25.0);

/// Seconds before the explosion when the trench run begins.
pub fn run_lead() -> f64 {
    TRENCH_RUN_SECONDS + ESCAPE_SECONDS + FUSE_SECONDS
}

/// The hero ship's nose position (offset from the station center) at `age`
/// seconds after the trench run began, or `None` once it has gone. The ship
/// flies left to right along the trench (row offset 0), then climbs away to
/// the upper right.
pub fn hero_at(age: f64, radius: f64) -> Option<(i32, i32)> {
    let enter = -radius - RUN_MARGIN;
    let exit = radius + RUN_MARGIN;
    if age < 0.0 {
        None
    } else if age < TRENCH_RUN_SECONDS {
        let x = enter + (exit - enter) * age / TRENCH_RUN_SECONDS;
        Some((js_round(x) as i32, 0))
    } else if age < TRENCH_RUN_SECONDS + ESCAPE_SECONDS {
        let k = (age - TRENCH_RUN_SECONDS) / ESCAPE_SECONDS;
        let x = exit + ESCAPE_DISTANCE.0 * k;
        // Ease into the climb so it pulls up out of the trench.
        let y = -ESCAPE_DISTANCE.1 * k * k;
        Some((js_round(x) as i32, js_round(y) as i32))
    } else {
        None
    }
}

/// What happens to the station on its laps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StationFate {
    /// Destroyed with [`DESTROY_CHANCE`] probability each lap.
    #[default]
    Chance,
    /// Always destroyed (for testing).
    Destroy,
    /// Never destroyed.
    Survive,
}

/// The station's state at one moment of its lap.
#[derive(Debug, Clone, PartialEq)]
pub struct StationEffects {
    /// True if the station disk should be drawn.
    pub station_visible: bool,
    /// True during the destruction flash: draw the disk in the flash color.
    pub flash: bool,
    /// Extra cells (fighters, bursts, fireball, shockwave, debris) as offsets
    /// from the station center with colors, drawn over the disk.
    pub cells: Vec<(i32, i32, Rgb)>,
}

/// Seed for one lap's battle.
fn lap_seed(lap: u64, salt: u32) -> u32 {
    (lap as u32).wrapping_mul(0x85eb_ca6b) ^ (lap >> 32) as u32 ^ salt
}

fn between(rng: &mut Mulberry32, range: (f64, f64)) -> f64 {
    range.0 + (range.1 - range.0) * rng.next_f64()
}

/// When (seconds into the lap) the station is destroyed on `lap`, if it is.
pub fn destruction_time(lap: u64, fate: StationFate, pass_seconds: f64) -> Option<f64> {
    let mut rng = Mulberry32::new(lap_seed(lap, 0x2f6b_1d3c));
    let roll = rng.next_f64();
    let window = between(&mut rng, DESTROY_WINDOW);
    match fate {
        StationFate::Survive => None,
        StationFate::Destroy => Some(FORCED_DESTROY_AT * pass_seconds),
        StationFate::Chance => (roll < DESTROY_CHANCE).then_some(window * pass_seconds),
    }
}

/// A fighter's state at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fighter {
    /// Flying: offset from the station center, and whether it is on the near
    /// side of its orbit.
    Flying { dx: i32, dy: i32, near: bool },
    /// Exploding at an offset, `age` seconds into the burst.
    Bursting { dx: i32, dy: i32, age: f64 },
}

/// Fighter `slot` at `t` seconds into `lap`, for a station of `radius`.
/// O(t / shortest lifetime).
pub fn fighter_at(lap: u64, slot: u32, t: f64, radius: f64) -> Fighter {
    let mut rng = Mulberry32::new(lap_seed(lap, slot.wrapping_mul(0x9e37_79b9) ^ 0x51ed_270b));
    // Stagger the first lifetimes so the slots do not all start together.
    let mut start = -between(&mut rng, (0.0, FIGHTER_LIFE.1));
    loop {
        let life = between(&mut rng, FIGHTER_LIFE);
        let distance = radius + between(&mut rng, ORBIT_GAP);
        let speed = between(&mut rng, ORBIT_SPEED) * if rng.next_f64() < 0.5 { -1.0 } else { 1.0 };
        let phase = rng.next_f64() * std::f64::consts::TAU;
        let place = |at: f64| {
            let angle = phase + speed * (at - start);
            let dx = js_round(distance * angle.cos()) as i32;
            let dy = js_round(distance * ORBIT_TILT * angle.sin()) as i32;
            (dx, dy, angle.sin() >= 0.0)
        };
        let death = start + life;
        if t < death {
            let (dx, dy, near) = place(t.max(start));
            return Fighter::Flying { dx, dy, near };
        }
        if t < death + BURST_SECONDS {
            let (dx, dy, _) = place(death);
            return Fighter::Bursting {
                dx,
                dy,
                age: t - death,
            };
        }
        start = death + BURST_SECONDS;
    }
}

/// Burst cells for a fighter explosion `age` seconds old: a flash, a `+` of
/// sparks, then fading diagonal embers.
pub fn burst_cells(age: f64, palette: &Palette) -> Vec<(i32, i32, Rgb)> {
    let third = BURST_SECONDS / 3.0;
    if age < third {
        vec![(0, 0, palette.star)]
    } else if age < 2.0 * third {
        let spark = palette.beacon;
        vec![
            (0, 0, palette.star),
            (1, 0, spark),
            (-1, 0, spark),
            (0, 1, spark),
            (0, -1, spark),
        ]
    } else {
        let ember = mix(palette.beacon, palette.bg, 0.5);
        vec![
            (1, 1, ember),
            (-1, 1, ember),
            (1, -1, ember),
            (-1, -1, ember),
        ]
    }
}

/// Everything around the station at `t` seconds into `lap`. O(fighters ×
/// lifetimes + explosion cells).
pub fn station_effects(
    lap: u64,
    t: f64,
    radius: i32,
    fate: StationFate,
    pass_seconds: f64,
    palette: &Palette,
) -> StationEffects {
    let r = f64::from(radius);
    let destroyed_at = destruction_time(lap, fate, pass_seconds);
    let since = destroyed_at.map(|at| t - at);
    let mut cells = Vec::new();

    // The hero's trench run, ending a few seconds before the explosion. A
    // two-cell dart: nose plus the cell behind it.
    if let Some(at) = destroyed_at {
        if let Some((dx, dy)) = hero_at(t - (at - run_lead()), r) {
            cells.push((dx - 1, dy, HERO_RED));
            cells.push((dx, dy, HERO_RED));
        }
    }

    // Fighters fight until the explosion is over.
    let fighting = since.is_none_or(|age| age < EXPLOSION_SECONDS);
    let station_there = since.is_none_or(|age| age < FLASH_SECONDS);
    if fighting {
        for slot in 0..FIGHTERS {
            match fighter_at(lap, slot, t, r) {
                Fighter::Flying { dx, dy, near } => {
                    let behind = !near && station_there && dx * dx + dy * dy <= radius * radius;
                    if !behind {
                        cells.push((dx, dy, palette.star));
                    }
                }
                Fighter::Bursting { dx, dy, age } => {
                    for (bx, by, color) in burst_cells(age, palette) {
                        cells.push((dx + bx, dy + by, color));
                    }
                }
            }
        }
    }

    if let Some(age) = since.filter(|age| (FLASH_SECONDS..EXPLOSION_SECONDS).contains(age)) {
        let progress = (age - FLASH_SECONDS) / (EXPLOSION_SECONDS - FLASH_SECONDS);
        // Shrinking fireball.
        let core = r * 0.6 * (1.0 - progress * 2.5);
        if core >= 1.0 {
            let c = core as i32;
            for dy in -c..=c {
                for dx in -c..=c {
                    if f64::from(dx * dx + dy * dy) <= core * core {
                        cells.push((dx, dy, mix(palette.beacon, palette.star, 0.3)));
                    }
                }
            }
        }
        // Expanding shockwave ring, fading.
        let ring = r + SHOCKWAVE_SPEED * (age - FLASH_SECONDS);
        let ring_color = mix(palette.star, palette.bg, progress);
        let steps = (ring * std::f64::consts::TAU).ceil() as i32;
        for step in 0..steps {
            let angle = f64::from(step) / f64::from(steps) * std::f64::consts::TAU;
            let dx = js_round(ring * angle.cos()) as i32;
            let dy = js_round(ring * ORBIT_TILT.max(0.6) * angle.sin()) as i32;
            cells.push((dx, dy, ring_color));
        }
        // Debris flying outward, fading.
        let mut rng = Mulberry32::new(lap_seed(lap, 0x7f4a_7c15));
        let debris_color = mix(palette.beacon, palette.bg, progress);
        for _ in 0..DEBRIS {
            let angle = rng.next_f64() * std::f64::consts::TAU;
            let speed = between(&mut rng, (4.0, 14.0));
            let distance = speed * (age - FLASH_SECONDS);
            let dx = js_round(distance * angle.cos()) as i32;
            let dy = js_round(distance * angle.sin()) as i32;
            cells.push((dx, dy, debris_color));
        }
    }

    StationEffects {
        station_visible: station_there,
        flash: since.is_some_and(|age| (0.0..FLASH_SECONDS).contains(&age)),
        cells,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::palette::palette_by_key;

    const PASS: f64 = 600.0;
    const R: i32 = 19;

    fn classic() -> &'static Palette {
        palette_by_key("classic").unwrap()
    }

    #[test]
    fn fighters_orbit_near_the_station_and_eventually_explode() {
        let mut burst_seen = false;
        for slot in 0..FIGHTERS {
            for step in 0..600 {
                let t = f64::from(step) * 0.1;
                match fighter_at(3, slot, t, f64::from(R)) {
                    Fighter::Flying { dx, dy, .. } | Fighter::Bursting { dx, dy, .. } => {
                        let max = f64::from(R) + ORBIT_GAP.1 + 1.0;
                        assert!(f64::from(dx.abs()) <= max && f64::from(dy.abs()) <= max);
                    }
                }
                if matches!(
                    fighter_at(3, slot, t, f64::from(R)),
                    Fighter::Bursting { .. }
                ) {
                    burst_seen = true;
                }
            }
        }
        assert!(burst_seen, "fighters explode within a minute");
    }

    #[test]
    fn fighters_are_deterministic_per_lap() {
        assert_eq!(fighter_at(5, 2, 33.3, 19.0), fighter_at(5, 2, 33.3, 19.0));
        let differs = (0..FIGHTERS)
            .any(|slot| fighter_at(5, slot, 20.0, 19.0) != fighter_at(6, slot, 20.0, 19.0));
        assert!(differs, "each lap has its own battle");
    }

    #[test]
    fn bursts_flash_then_spark_then_fade() {
        let classic = classic();
        assert_eq!(burst_cells(0.05, classic), vec![(0, 0, classic.star)]);
        assert_eq!(burst_cells(0.3, classic).len(), 5);
        let embers = burst_cells(0.55, classic);
        assert_eq!(embers.len(), 4);
        assert!(embers.iter().all(|&(dx, dy, _)| dx != 0 && dy != 0));
    }

    #[test]
    fn fate_controls_destruction() {
        assert_eq!(destruction_time(1, StationFate::Survive, PASS), None);
        assert_eq!(
            destruction_time(1, StationFate::Destroy, PASS),
            Some(0.32 * PASS)
        );
        let destroyed = (0..10_000)
            .filter(|&lap| destruction_time(lap, StationFate::Chance, PASS).is_some())
            .count();
        let share = destroyed as f64 / 10_000.0;
        assert!((0.27..0.33).contains(&share), "destroy share {share}");
        for lap in 0..200 {
            if let Some(at) = destruction_time(lap, StationFate::Chance, PASS) {
                assert!((0.35 * PASS..0.65 * PASS).contains(&at));
            }
        }
    }

    #[test]
    fn destruction_flashes_then_explodes_then_leaves_an_empty_sky() {
        let classic = classic();
        let at = FORCED_DESTROY_AT * PASS;
        let effect = |t| station_effects(0, t, R, StationFate::Destroy, PASS, classic);

        let before = effect(at - 1.0);
        assert!(before.station_visible && !before.flash);
        assert!(!before.cells.is_empty(), "fighters are attacking");

        let flash = effect(at + 0.1);
        assert!(flash.station_visible && flash.flash);

        let boom = effect(at + 1.0);
        assert!(!boom.station_visible && !boom.flash);
        let reach = boom.cells.iter().map(|&(dx, _, _)| dx.abs()).max().unwrap();
        assert!(reach > R, "the shockwave spreads past the station");

        let after = effect(at + EXPLOSION_SECONDS + 1.0);
        assert!(!after.station_visible);
        assert!(after.cells.is_empty(), "the sky is empty afterwards");
    }

    #[test]
    fn the_hero_runs_the_trench_climbs_away_then_the_station_blows() {
        let r = f64::from(R);
        // Along the trench, left to right.
        let start = hero_at(0.0, r).unwrap();
        let middle = hero_at(TRENCH_RUN_SECONDS / 2.0, r).unwrap();
        let end = hero_at(TRENCH_RUN_SECONDS - 0.01, r).unwrap();
        assert_eq!((start.1, middle.1, end.1), (0, 0, 0), "in the trench row");
        assert!(
            start.0 < -R && end.0 > R && middle.0.abs() <= 1,
            "edge to edge"
        );
        // Then climbs away up and to the right, and is gone before the fuse.
        let climb = hero_at(TRENCH_RUN_SECONDS + ESCAPE_SECONDS * 0.9, r).unwrap();
        assert!(climb.0 > end.0 && climb.1 < -10, "climbing away: {climb:?}");
        assert_eq!(hero_at(TRENCH_RUN_SECONDS + ESCAPE_SECONDS + 0.01, r), None);
        assert_eq!(hero_at(-0.1, r), None);

        // Wired into the effects: red in every palette, only before the blast.
        for key in ["classic", "gameboy", "amber", "green", "blackonwhite"] {
            let palette = palette_by_key(key).unwrap();
            let at = FORCED_DESTROY_AT * PASS;
            let run_mid = at - run_lead() + TRENCH_RUN_SECONDS / 2.0;
            let during = station_effects(0, run_mid, R, StationFate::Destroy, PASS, palette);
            let red: Vec<_> = during.cells.iter().filter(|c| c.2 == HERO_RED).collect();
            assert_eq!(red.len(), 2, "{key}: one two-cell red ship");
            assert!(red.iter().all(|c| c.1 == 0), "{key}: in the trench");
            let fuse = station_effects(0, at - 1.0, R, StationFate::Destroy, PASS, palette);
            assert!(
                fuse.cells.iter().all(|c| c.2 != HERO_RED),
                "{key}: gone before the blast"
            );
        }
    }

    #[test]
    fn no_trench_run_without_destruction() {
        for t in [100.0, 200.0, 300.0, 400.0] {
            let effects = station_effects(0, t, R, StationFate::Survive, PASS, classic());
            assert!(effects.cells.iter().all(|c| c.2 != HERO_RED));
        }
    }

    #[test]
    fn a_surviving_station_stays_under_attack() {
        let effects = station_effects(0, 300.0, R, StationFate::Survive, PASS, classic());
        assert!(effects.station_visible && !effects.flash);
        assert!(!effects.cells.is_empty());
    }
}
