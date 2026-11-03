//! The moon (Starry Journey extension; owner decisions 2026-11-03).
//!
//! - A 19-cell disk (diameter in virtual pixels), drawn behind the city and
//!   street so towers and trees pass in front of it.
//! - It crosses the sky in 5 minutes on a shallow arc: it rises from behind
//!   the skyline on the left, peaks in the open sky band, and sets behind the
//!   buildings on the right. Then the next pass begins.
//! - Each pass waxes from new moon at moonrise to full moon at moonset; the
//!   lit side faces right (waxing, as seen from the northern hemisphere).
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
pub const MOON_DIAMETER: i32 = 19;
/// Cells from the center pixel to the edge.
pub const MOON_RADIUS: i32 = MOON_DIAMETER / 2;
/// Seconds for one pass across the sky.
pub const PASS_SECONDS: f64 = 300.0;
/// Where in its first pass the moon starts, so it is already in view (and
/// already a crescent) when the app opens.
pub const START_FRACTION: f64 = 0.3;
/// Apex of the arc as a fraction of the screen height from the top.
pub const APEX_FRACTION: f64 = 0.22;
/// How far the seas move from the star color toward the sky.
pub const SEA_BLEND: f64 = 0.3;
/// How far the unlit side moves from the sky toward the star color.
pub const EARTHSHINE: f64 = 0.08;

/// Dark "seas" on the lit side, as offsets from the center pixel.
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

/// The moon's position and phase at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoonState {
    /// Center column.
    pub x: i32,
    /// Center row (0 is the top).
    pub row: i32,
    /// Phase from 0 (new) to 1 (full).
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
    let passes = seconds.max(0.0) / PASS_SECONDS + START_FRACTION;
    let t = passes.fract();
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
    MoonState { x, row, phase: t }
}

/// What the cell at offset `(dx, dy)` from the center shows at `phase`.
///
/// A cell is on the disk when `dx² + dy² <= 9.5²`. Within each row the
/// terminator sits at `u = cos(π · phase)`, where `u` is the cell's position
/// across that row from -1 (left edge) to 1 (right edge); cells right of it
/// are lit. O(1).
pub fn moon_pixel(dx: i32, dy: i32, phase: f64) -> MoonPixel {
    let radius = f64::from(MOON_DIAMETER) / 2.0;
    let distance_sq = f64::from(dx * dx + dy * dy);
    if distance_sq > radius * radius {
        return MoonPixel::Outside;
    }
    let half_width = (radius * radius - f64::from(dy * dy)).sqrt().max(0.5);
    let u = f64::from(dx) / half_width;
    let terminator = (std::f64::consts::PI * phase.clamp(0.0, 1.0)).cos();
    if u <= terminator {
        MoonPixel::Dark
    } else if SEAS.contains(&(dx, dy)) {
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

    #[test]
    fn disk_is_nineteen_cells_across() {
        let full = |dx, dy| moon_pixel(dx, dy, 1.0) != MoonPixel::Outside;
        assert!(full(-9, 0) && full(9, 0) && full(0, -9) && full(0, 9));
        assert!(!full(-10, 0) && !full(0, 10));
        assert!(!full(9, 9), "corners are outside the circle");
        let width_at_center = (-10..=10).filter(|&dx| full(dx, 0)).count();
        assert_eq!(width_at_center, 19);
    }

    #[test]
    fn phases_wax_from_new_to_full_with_the_lit_side_on_the_right() {
        let disk = count(1.0, &[MoonPixel::Lit, MoonPixel::Sea, MoonPixel::Dark]);
        assert_eq!(count(0.0, &[MoonPixel::Lit, MoonPixel::Sea]), 0, "new moon");
        assert_eq!(
            count(1.0, &[MoonPixel::Lit, MoonPixel::Sea]),
            disk,
            "full moon"
        );
        let quarter = count(0.5, &[MoonPixel::Lit, MoonPixel::Sea]);
        assert!(
            quarter > disk / 3 && quarter < disk * 2 / 3,
            "first quarter: {quarter}/{disk}"
        );
        assert_ne!(moon_pixel(8, 0, 0.5), MoonPixel::Dark, "right side lit");
        assert_eq!(moon_pixel(-8, 0, 0.5), MoonPixel::Dark, "left side dark");
        let lit: Vec<usize> = (0..=10)
            .map(|step| count(f64::from(step) / 10.0, &[MoonPixel::Lit, MoonPixel::Sea]))
            .collect();
        assert!(
            lit.windows(2).all(|pair| pair[0] <= pair[1]),
            "always waxing: {lit:?}"
        );
        assert!(
            count(1.0, &[MoonPixel::Sea]) > 10,
            "seas show on a full moon"
        );
    }

    #[test]
    fn a_pass_rises_left_peaks_in_clear_sky_and_sets_right_in_five_minutes() {
        let grid = grid();
        let start = moon_at(&grid, (1.0 - START_FRACTION) * PASS_SECONDS);
        assert_eq!(start.x, -MOON_RADIUS, "rises at the left edge");
        assert!(
            start.row - MOON_RADIUS >= grid.ground_row,
            "hidden behind the street"
        );
        assert!(start.phase < 1e-9, "new at moonrise");
        let middle = moon_at(&grid, (1.5 - START_FRACTION) * PASS_SECONDS);
        assert!(
            middle.row + MOON_RADIUS < grid.ground_row - grid.max_h,
            "above every tower"
        );
        assert!(
            (middle.phase - 0.5).abs() < 1e-9,
            "first quarter at the top"
        );
        let late = moon_at(&grid, (1.999 - START_FRACTION) * PASS_SECONDS);
        assert!(late.x >= grid.cols - 1, "sets at the right edge");
        assert!(late.phase > 0.99, "nearly full at moonset");
    }

    #[test]
    fn the_app_opens_with_a_visible_crescent_and_passes_repeat() {
        let grid = grid();
        let opening = moon_at(&grid, 0.0);
        assert!((opening.phase - START_FRACTION).abs() < 1e-9);
        assert!((0..grid.cols).contains(&opening.x));
        assert!(
            opening.row < grid.ground_row - grid.max_h,
            "clear of the skyline"
        );
        let (now, next) = (moon_at(&grid, 42.0), moon_at(&grid, 42.0 + PASS_SECONDS));
        assert_eq!((now.x, now.row), (next.x, next.row));
        assert!((now.phase - next.phase).abs() < 1e-9);
        assert_eq!(
            moon_at(&grid, -5.0),
            moon_at(&grid, 0.0),
            "negative time clamps"
        );
    }

    #[test]
    fn spec_scaling_still_fits_the_arc_above_the_city() {
        let grid = Grid::new(1920, 1080, true); // about 240 rows
        let top = moon_at(&grid, (1.5 - START_FRACTION) * PASS_SECONDS);
        assert!(top.row - MOON_RADIUS >= 1, "inside the screen");
        assert!(top.row + MOON_RADIUS < grid.ground_row - grid.max_h);
    }

    #[test]
    fn cells_use_palette_colors_and_clip_to_the_grid() {
        let grid = grid();
        let classic = palette_by_key("classic").unwrap();
        let full = MoonState {
            x: 100,
            row: 60,
            phase: 1.0,
        };
        let cells = moon_cells(&grid, &full, classic);
        assert_eq!(
            cells.len(),
            count(1.0, &[MoonPixel::Lit, MoonPixel::Sea, MoonPixel::Dark])
        );
        assert!(cells.iter().any(|&(_, c)| c == classic.star));
        assert!(cells
            .iter()
            .any(|&(_, c)| c == mix(classic.star, classic.bg, SEA_BLEND)));
        let edge = MoonState {
            x: 0,
            row: 0,
            phase: 1.0,
        };
        let clipped = moon_cells(&grid, &edge, classic);
        assert!(
            clipped.len() < cells.len() / 3,
            "only the in-screen quarter is drawn"
        );
        let cells_total = (grid.cols * grid.rows) as usize;
        assert!(clipped.iter().all(|&(cell, _)| cell < cells_total));
    }
}
