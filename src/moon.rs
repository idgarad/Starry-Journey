//! The moon (Starry Journey extension; owner decisions 2026-11-03).
//!
//! - A 39-cell disk (diameter in virtual pixels), on the sky layer: drawn
//!   behind the city and street, so towers and trees pass in front of it.
//! - It crosses the sky in 5 minutes on a shallow arc: it rises from behind
//!   the skyline on the left, peaks in the open sky band, and sets behind the
//!   buildings on the right. Then the next lap begins.
//! - The phase is fixed for a whole lap and advances one step per lap
//!   through the 8-phase lunar cycle (new, waxing crescent, first quarter,
//!   waxing gibbous, full, waning gibbous, last quarter, waning crescent),
//!   so a full cycle takes 40 minutes. Waxing moons are lit on the right,
//!   waning moons on the left, as seen from the northern hemisphere.
//! - Colors come from the palette: the lit side in the star color with a few
//!   darker "seas", the unlit side faintly visible just above the sky color.
//!
//! Everything is a pure function of the grid, the palette, and the elapsed
//! time, so the moon is the same after a resize or palette change.

use crate::model::grid::Grid;
use crate::model::palette::{Palette, Rgb};
use crate::model::rng::js_round;
use crate::raster::mix;

/// Moon diameter in virtual pixels.
pub const MOON_DIAMETER: i32 = 39;
/// Cells from the center pixel to the edge.
pub const MOON_RADIUS: i32 = MOON_DIAMETER / 2;
/// Seconds for one lap across the sky.
pub const PASS_SECONDS: f64 = 300.0;
/// Phases in one lunar cycle; one step per lap.
pub const PHASE_STEPS: u64 = 8;
/// Phase step shown on the first lap: a waxing crescent, so the moon is
/// visible when the app opens.
pub const START_PHASE_STEP: u64 = 1;
/// Where in its first lap the moon starts, so it is already in view when
/// the app opens.
pub const START_FRACTION: f64 = 0.3;
/// Apex of the arc as a fraction of the screen height from the top.
pub const APEX_FRACTION: f64 = 0.22;
/// How far the seas move from the star color toward the sky.
pub const SEA_BLEND: f64 = 0.3;
/// How far the unlit side moves from the sky toward the star color.
pub const EARTHSHINE: f64 = 0.08;

/// Dark "seas", designed on a 19-cell disk as offsets from the center; on
/// the 39-cell disk each becomes a 2 x 2 block.
const SEAS: [(i32, i32); 20] = [
    (-4, -4),
    (-3, -4),
    (-4, -3),
    (-3, -3),
    (-2, -3),
    (-3, -2),
    (2, -2),
    (3, -2),
    (2, -1),
    (3, -1),
    (4, -1),
    (3, 0),
    (-2, 3),
    (-1, 3),
    (0, 3),
    (-1, 4),
    (0, 4),
    (5, 3),
    (5, 4),
    (4, 4),
];
/// Scale from the sea design grid to the disk.
const SEA_SCALE: i32 = 2;

/// The moon's position and phase at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoonState {
    /// Center column.
    pub x: i32,
    /// Center row (0 is the top).
    pub row: i32,
    /// Phase in the lunar cycle: 0 new, 0.25 first quarter, 0.5 full,
    /// 0.75 last quarter.
    pub phase: f64,
}

/// What one cell of the moon disk shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoonPixel {
    /// Outside the disk.
    Outside,
    /// The unlit part of the disk.
    Dark,
    /// The lit part of the disk.
    Lit,
    /// A darker sea on the lit part.
    Sea,
}

/// The moon at `seconds` since the app started. O(1).
pub fn moon_at(grid: &Grid, seconds: f64) -> MoonState {
    let laps = seconds.max(0.0) / PASS_SECONDS + START_FRACTION;
    let t = laps.fract();
    let step = (START_PHASE_STEP + laps.floor() as u64) % PHASE_STEPS;
    let start_x = -MOON_RADIUS;
    let end_x = grid.cols - 1 + MOON_RADIUS;
    let x = js_round(f64::from(start_x) + f64::from(end_x - start_x) * t) as i32;
    // At moonrise and moonset the whole disk sits below the road's top edge,
    // hidden behind the street.
    let horizon = grid.ground_row + MOON_RADIUS;
    let clear_sky = grid.ground_row - grid.max_h - MOON_RADIUS - 2;
    let apex = js_round(f64::from(grid.rows) * APEX_FRACTION)
        .min(f64::from(clear_sky))
        .max(f64::from(MOON_RADIUS + 1)) as i32;
    let lift = f64::from(horizon - apex) * (std::f64::consts::PI * t).sin();
    let row = js_round(f64::from(horizon) - lift) as i32;
    MoonState {
        x,
        row,
        phase: step as f64 / PHASE_STEPS as f64,
    }
}

/// What the cell at offset `(dx, dy)` from the center shows at `phase`.
///
/// A cell is on the disk when `dx² + dy² <= 19.5²`. Within each row, `u` is
/// the cell's position from -1 (left edge) to 1 (right edge). With
/// `θ = 2π · phase`, a waxing moon (`θ <= π`) is lit where `u > cos θ`; a
/// waning moon is lit where `u < -cos θ`. O(1).
pub fn moon_pixel(dx: i32, dy: i32, phase: f64) -> MoonPixel {
    let radius = f64::from(MOON_DIAMETER) / 2.0;
    let distance_sq = f64::from(dx * dx + dy * dy);
    if distance_sq > radius * radius {
        return MoonPixel::Outside;
    }
    let half_width = (radius * radius - f64::from(dy * dy)).sqrt().max(0.5);
    let u = f64::from(dx) / half_width;
    let theta = std::f64::consts::TAU * phase.rem_euclid(1.0);
    let lit = if theta <= std::f64::consts::PI {
        u > theta.cos()
    } else {
        u < -theta.cos()
    };
    if !lit {
        MoonPixel::Dark
    } else if SEAS.contains(&(dx.div_euclid(SEA_SCALE), dy.div_euclid(SEA_SCALE))) {
        MoonPixel::Sea
    } else {
        MoonPixel::Lit
    }
}

/// Cells to paint for the moon: `(buffer cell, color)`, clipped to the grid.
/// O(diameter²).
pub fn moon_cells(grid: &Grid, moon: &MoonState, palette: &Palette) -> Vec<(usize, Rgb)> {
    let lit = palette.star;
    let sea = mix(palette.star, palette.bg, SEA_BLEND);
    let dark = mix(palette.bg, palette.star, EARTHSHINE);
    let mut cells = Vec::new();
    for dy in -MOON_RADIUS..=MOON_RADIUS {
        for dx in -MOON_RADIUS..=MOON_RADIUS {
            let color = match moon_pixel(dx, dy, moon.phase) {
                MoonPixel::Outside => continue,
                MoonPixel::Dark => dark,
                MoonPixel::Lit => lit,
                MoonPixel::Sea => sea,
            };
            let (x, row) = (moon.x + dx, moon.row + dy);
            if (0..grid.cols).contains(&x) && (0..grid.rows).contains(&row) {
                cells.push((row as usize * grid.cols as usize + x as usize, color));
            }
        }
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::grid::CellScaling;
    use crate::model::palette::palette_by_key;

    const NEW: f64 = 0.0;
    const FIRST_QUARTER: f64 = 0.25;
    const FULL: f64 = 0.5;
    const LAST_QUARTER: f64 = 0.75;

    fn grid() -> Grid {
        Grid::with_scaling(1920, 1080, true, CellScaling::Fixed(3))
    }

    fn count(phase: f64, kind: &[MoonPixel]) -> usize {
        let r = MOON_RADIUS;
        (-r..=r)
            .flat_map(|dy| (-r..=r).map(move |dx| (dx, dy)))
            .filter(|&(dx, dy)| kind.contains(&moon_pixel(dx, dy, phase)))
            .count()
    }

    fn lit(phase: f64) -> usize {
        count(phase, &[MoonPixel::Lit, MoonPixel::Sea])
    }

    #[test]
    fn disk_is_thirty_nine_cells_across() {
        assert_eq!(MOON_DIAMETER, 39);
        let on_disk = |dx, dy| moon_pixel(dx, dy, FULL) != MoonPixel::Outside;
        assert!(on_disk(-19, 0) && on_disk(19, 0) && on_disk(0, -19) && on_disk(0, 19));
        assert!(!on_disk(-20, 0) && !on_disk(0, 20));
        assert!(!on_disk(19, 19), "corners are outside the circle");
        assert_eq!((-20..=20).filter(|&dx| on_disk(dx, 0)).count(), 39);
    }

    #[test]
    fn eight_phases_wax_on_the_right_and_wane_on_the_left() {
        let disk = count(FULL, &[MoonPixel::Lit, MoonPixel::Sea, MoonPixel::Dark]);
        assert_eq!(lit(NEW), 0, "new moon");
        assert_eq!(lit(FULL), disk, "full moon");
        for quarter in [FIRST_QUARTER, LAST_QUARTER] {
            let half = lit(quarter);
            assert!(
                half > disk * 2 / 5 && half < disk * 3 / 5,
                "{quarter}: {half}/{disk}"
            );
        }
        assert_ne!(
            moon_pixel(15, 0, FIRST_QUARTER),
            MoonPixel::Dark,
            "waxing lit right"
        );
        assert_eq!(moon_pixel(-15, 0, FIRST_QUARTER), MoonPixel::Dark);
        assert_ne!(
            moon_pixel(-15, 0, LAST_QUARTER),
            MoonPixel::Dark,
            "waning lit left"
        );
        assert_eq!(moon_pixel(15, 0, LAST_QUARTER), MoonPixel::Dark);
        let cycle: Vec<usize> = (0..8).map(|step| lit(f64::from(step) / 8.0)).collect();
        assert!(
            cycle[..5].windows(2).all(|p| p[0] < p[1]),
            "waxing grows: {cycle:?}"
        );
        assert!(
            cycle[4..].windows(2).all(|p| p[0] > p[1]),
            "waning shrinks: {cycle:?}"
        );
        assert_eq!(cycle[1], cycle[7], "crescents mirror each other");
        assert!(
            count(FULL, &[MoonPixel::Sea]) > 40,
            "seas show on a full moon"
        );
    }

    #[test]
    fn phase_is_fixed_for_a_lap_and_advances_one_step_per_lap() {
        let grid = grid();
        let lap_start = (1.0 - START_FRACTION) * PASS_SECONDS; // second lap begins
        let early = moon_at(&grid, lap_start + 1.0);
        let late = moon_at(&grid, lap_start + PASS_SECONDS - 1.0);
        assert_eq!(early.phase, late.phase, "fixed during the lap");
        assert_eq!(
            moon_at(&grid, 0.0).phase,
            1.0 / 8.0,
            "opens as a waxing crescent"
        );
        assert_eq!(early.phase, 2.0 / 8.0, "next lap: first quarter");
        let phases: Vec<f64> = (0..9)
            .map(|lap| moon_at(&grid, lap_start + 10.0 + f64::from(lap) * PASS_SECONDS).phase)
            .collect();
        assert_eq!(phases[0], 0.25);
        assert_eq!(phases[5], 0.875, "waning crescent");
        assert_eq!(phases[6], 0.0, "back to new after the waning crescent");
        assert_eq!(phases[8], phases[0], "the cycle repeats every 8 laps");
    }

    #[test]
    fn a_lap_rises_left_peaks_in_clear_sky_and_sets_right_in_five_minutes() {
        let grid = grid();
        let lap_start = (1.0 - START_FRACTION) * PASS_SECONDS;
        let rise = moon_at(&grid, lap_start);
        assert_eq!(rise.x, -MOON_RADIUS, "rises at the left edge");
        assert!(
            rise.row - MOON_RADIUS >= grid.ground_row,
            "hidden behind the street"
        );
        let top = moon_at(&grid, lap_start + PASS_SECONDS / 2.0);
        assert!(top.row - MOON_RADIUS >= 0, "inside the screen");
        assert!(
            top.row + MOON_RADIUS < grid.ground_row - grid.max_h,
            "above every tower"
        );
        let set = moon_at(&grid, lap_start + PASS_SECONDS - 0.3);
        assert!(set.x >= grid.cols - 1, "sets at the right edge");
    }

    #[test]
    fn the_app_opens_with_the_moon_in_view() {
        let grid = grid();
        let opening = moon_at(&grid, 0.0);
        assert!((0..grid.cols).contains(&opening.x));
        assert!(
            opening.row < grid.ground_row - grid.max_h,
            "center clear of the skyline"
        );
        assert_eq!(
            moon_at(&grid, -5.0),
            moon_at(&grid, 0.0),
            "negative time clamps"
        );
    }

    #[test]
    fn spec_scaling_keeps_the_moon_on_screen() {
        // With --pixel-size spec there are only about 216 rows, so the big
        // moon may dip behind a maximum-height tower at the top of its arc.
        // That is fine: the moon lives on the sky layer.
        let grid = Grid::new(1920, 1080, true);
        let top = moon_at(&grid, (1.5 - START_FRACTION) * PASS_SECONDS);
        assert!(top.row - MOON_RADIUS >= 0, "inside the screen");
        assert!(top.row + MOON_RADIUS <= grid.ground_row - grid.max_h + 2);
    }

    #[test]
    fn cells_use_palette_colors_and_clip_to_the_grid() {
        let grid = grid();
        let classic = palette_by_key("classic").unwrap();
        let full = MoonState {
            x: 100,
            row: 60,
            phase: FULL,
        };
        let cells = moon_cells(&grid, &full, classic);
        assert_eq!(
            cells.len(),
            count(FULL, &[MoonPixel::Lit, MoonPixel::Sea, MoonPixel::Dark])
        );
        assert!(cells.iter().any(|&(_, c)| c == classic.star));
        assert!(cells
            .iter()
            .any(|&(_, c)| c == mix(classic.star, classic.bg, SEA_BLEND)));
        let edge = MoonState {
            x: 0,
            row: 0,
            phase: FULL,
        };
        let clipped = moon_cells(&grid, &edge, classic);
        assert!(
            clipped.len() < cells.len() / 3,
            "only the in-screen quarter is drawn"
        );
        let total = (grid.cols * grid.rows) as usize;
        assert!(clipped.iter().all(|&(cell, _)| cell < total));
    }
}
