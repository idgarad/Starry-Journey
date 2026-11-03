//! Logical cell grid derived from the device surface
//! (Rebuild Prompt, "Coordinates, Arithmetic, And Data").
//!
//! By default ([`CellScaling::Spec`]) the cell size follows the device height
//! (about 240 rows) and the column count follows the width, so the scene
//! fills the window with the same look on every screen. With
//! [`CellScaling::Fixed`] (the `--pixel-size` launch option) the cell size is
//! fixed, so bigger screens get more city and more sky; building heights then
//! come from the 240-row reference so towers keep their proportions. The grid
//! may overrun the device size by less than one cell; the rasterizer clips.

use super::rng::js_round;

/// Rows reserved for the endless-mode road below the city.
pub const ROAD_ROWS: i32 = 7;
/// Target logical row count used to derive the cell size.
pub const TARGET_ROWS: f64 = 240.0;
/// Smallest allowed cell size in device pixels.
pub const MIN_CELL: u32 = 2;
/// Smallest allowed building height limit in cells.
pub const MIN_MAX_HEIGHT: i32 = 8;
/// Fraction of the ground row available to building heights.
pub const MAX_HEIGHT_FRACTION: f64 = 0.82;

/// How the cell size is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellScaling {
    /// Rebuild Prompt formula: `max(2, round(deviceHeight / 240))`.
    #[default]
    Spec,
    /// A fixed number of device pixels per cell (at least 1).
    Fixed(u32),
}

/// Grid dimensions for one device surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    /// Device (physical) surface width in pixels, at least 1.
    pub device_width: u32,
    /// Device (physical) surface height in pixels, at least 1.
    pub device_height: u32,
    /// Device pixels per logical cell edge.
    pub cell: u32,
    /// Logical columns: `ceil(device_width / cell)`.
    pub cols: i32,
    /// Logical rows: `ceil(device_height / cell)`.
    pub rows: i32,
    /// First row below the city: `rows` in normal mode, `rows - 7` in endless.
    pub ground_row: i32,
    /// Building height limit in cells: `max(8, floor(ground_row * 0.82))`.
    pub max_h: i32,
}

impl Grid {
    /// Builds the grid for a device surface. O(1).
    ///
    /// Zero dimensions are treated as 1, as in the original's
    /// `max(1, round(viewport * devicePixelRatio))`. A native window already
    /// reports physical pixels, so no device-pixel-ratio scaling happens here.
    /// When an endless surface is too short for the road, `ground_row` is
    /// clamped at 0 rather than going negative (assumption AS-R001).
    pub fn new(device_width: u32, device_height: u32, endless: bool) -> Self {
        Self::with_scaling(device_width, device_height, endless, CellScaling::Spec)
    }

    /// Builds the grid with a chosen [`CellScaling`]. O(1).
    ///
    /// With `Fixed`, `max_h` is computed from the smaller of the real ground
    /// row and the 240-row reference ground row, so a taller screen adds sky
    /// rather than stretching every tower.
    pub fn with_scaling(
        device_width: u32,
        device_height: u32,
        endless: bool,
        scaling: CellScaling,
    ) -> Self {
        let device_width = device_width.max(1);
        let device_height = device_height.max(1);
        let cell = match scaling {
            CellScaling::Spec => {
                (js_round(f64::from(device_height) / TARGET_ROWS) as u32).max(MIN_CELL)
            }
            CellScaling::Fixed(cell) => cell.max(1),
        };
        let cols = device_width.div_ceil(cell) as i32;
        let rows = device_height.div_ceil(cell) as i32;
        let ground_row = if endless {
            (rows - ROAD_ROWS).max(0)
        } else {
            rows
        };
        let height_ground = match scaling {
            CellScaling::Spec => ground_row,
            CellScaling::Fixed(_) => {
                let reference = TARGET_ROWS as i32 - if endless { ROAD_ROWS } else { 0 };
                ground_row.min(reference)
            }
        };
        let max_h =
            ((f64::from(height_ground) * MAX_HEIGHT_FRACTION).floor() as i32).max(MIN_MAX_HEIGHT);
        Self {
            device_width,
            device_height,
            cell,
            cols,
            rows,
            ground_row,
            max_h,
        }
    }

    /// Converts a building height coordinate to a logical row:
    /// `ground_row - 1 - height`.
    pub fn row_for_height(&self, height: i32) -> i32 {
        self.ground_row - 1 - height
    }

    /// Converts a continuous world x to a device-pixel x for a camera at
    /// `view_x`: `(world_x - view_x) * cell`.
    pub fn screen_x(&self, world_x: f64, view_x: f64) -> f64 {
        (world_x - view_x) * f64::from(self.cell)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_1080p_normal_has_240_rows_and_full_width() {
        let grid = Grid::new(1920, 1080, false);
        assert_eq!(grid.cell, 5); // round(4.5) rounds half up
        assert_eq!((grid.cols, grid.rows), (384, 216));
        assert_eq!(grid.ground_row, 216);
        assert_eq!(grid.max_h, 177); // floor(216 * 0.82)
    }

    #[test]
    fn grid_endless_reserves_seven_road_rows() {
        let grid = Grid::new(1280, 720, true);
        assert_eq!(grid.cell, 3);
        assert_eq!((grid.cols, grid.rows), (427, 240));
        assert_eq!(grid.ground_row, 233);
        assert_eq!(grid.max_h, 191);
    }

    #[test]
    fn grid_overruns_device_by_less_than_one_cell() {
        let grid = Grid::new(1366, 768, false);
        assert_eq!(grid.cell, 3);
        assert_eq!(grid.cols, 456); // ceil(1366 / 3)
        assert!(grid.cols as u32 * grid.cell >= 1366);
        assert!((grid.cols as u32 - 1) * grid.cell < 1366);
    }

    #[test]
    fn grid_small_and_zero_surfaces_use_minimums() {
        let grid = Grid::new(0, 0, false);
        assert_eq!((grid.device_width, grid.device_height), (1, 1));
        assert_eq!(grid.cell, MIN_CELL);
        assert_eq!((grid.cols, grid.rows), (1, 1));
        assert_eq!(grid.max_h, MIN_MAX_HEIGHT);

        let tiny_endless = Grid::new(10, 10, true);
        assert_eq!(tiny_endless.rows, 5);
        assert_eq!(tiny_endless.ground_row, 0);
    }

    #[test]
    fn grid_4k_scales_cells_not_rows() {
        let grid = Grid::new(3840, 2160, false);
        assert_eq!(grid.cell, 9);
        assert_eq!((grid.cols, grid.rows), (427, 240));
    }

    #[test]
    fn fixed_cells_add_city_and_sky_without_stretching_towers() {
        let fixed = |w, h| Grid::with_scaling(w, h, false, CellScaling::Fixed(4));
        let hd = fixed(1920, 1080);
        assert_eq!((hd.cell, hd.cols, hd.rows), (4, 480, 270));
        let uhd = fixed(3840, 2160);
        assert_eq!((uhd.cell, uhd.cols, uhd.rows), (4, 960, 540));
        // Height limit comes from the 240-row reference: floor(240 * 0.82).
        assert_eq!((hd.max_h, uhd.max_h), (196, 196));
        // A short window uses its own (smaller) ground row.
        let small = fixed(1280, 720);
        assert_eq!((small.rows, small.max_h), (180, 147));
        let endless = Grid::with_scaling(3840, 2160, true, CellScaling::Fixed(4));
        assert_eq!((endless.ground_row, endless.max_h), (533, 191)); // floor(233 * 0.82)
        assert_eq!(
            Grid::with_scaling(100, 100, false, CellScaling::Fixed(0)).cell,
            1
        );
    }

    #[test]
    fn spec_scaling_is_the_default() {
        assert_eq!(CellScaling::default(), CellScaling::Spec);
        assert_eq!(
            Grid::new(1920, 1080, false),
            Grid::with_scaling(1920, 1080, false, CellScaling::Spec)
        );
    }

    #[test]
    fn row_and_screen_conversions() {
        let grid = Grid::new(1920, 1080, false);
        assert_eq!(grid.row_for_height(0), 215);
        assert_eq!(grid.row_for_height(10), 205);
        assert_eq!(grid.screen_x(12.5, 10.0), 12.5);
        assert_eq!(grid.screen_x(9.0, 10.0), -5.0);
    }
}
