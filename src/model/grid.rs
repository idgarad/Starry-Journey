//! Logical cell grid derived from the device surface
//! (Rebuild Prompt, "Coordinates, Arithmetic, And Data").
//!
//! The grid replaces SKY-001's fixed 320 x 180 canvas: cell size follows the
//! device height (about 240 rows) and the column count follows the width, so
//! the scene fills the window. The grid may overrun the device size by less
//! than one cell; the rasterizer clips.

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
        let device_width = device_width.max(1);
        let device_height = device_height.max(1);
        let cell = (js_round(f64::from(device_height) / TARGET_ROWS) as u32).max(MIN_CELL);
        let cols = device_width.div_ceil(cell) as i32;
        let rows = device_height.div_ceil(cell) as i32;
        let ground_row = if endless {
            (rows - ROAD_ROWS).max(0)
        } else {
            rows
        };
        let max_h =
            ((f64::from(ground_row) * MAX_HEIGHT_FRACTION).floor() as i32).max(MIN_MAX_HEIGHT);
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
    fn row_and_screen_conversions() {
        let grid = Grid::new(1920, 1080, false);
        assert_eq!(grid.row_for_height(0), 215);
        assert_eq!(grid.row_for_height(10), 205);
        assert_eq!(grid.screen_x(12.5, 10.0), 12.5);
        assert_eq!(grid.screen_x(9.0, 10.0), -5.0);
    }
}
