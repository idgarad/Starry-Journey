//! The street: road, curbs, lane dashes, and roadside trees drawn over the
//! city image (Rebuild Prompt, road and tree paragraphs; Starry Journey
//! extension: also in Normal mode).
//!
//! The road occupies the seven rows from the grid's ground row down. Draw
//! order follows the spec: road, background-verge trees (roots on the road's
//! top edge, in front of the city), traffic, then foreground-verge trees
//! (roots on the bottom row, in front of the traffic). The static parts are
//! precomputed per grid and palette as lists of cell colors, so applying
//! them each frame is O(street cells).

use crate::model::grid::{Grid, ROAD_ROWS};
use crate::model::palette::{Palette, Rgb};
use crate::model::trees::{tree_colors, trees_in, Tree, TREE_WIDTH};

/// Length of a center-line dash in cells.
pub const DASH_LENGTH: i32 = 4;
/// Repeat of the center-line dashes in cells.
pub const DASH_PERIOD: i32 = 8;

/// Cells to overwrite: buffer cell index and color.
pub type CellPaint = Vec<(usize, Rgb)>;

/// The strip of the world a street buffer covers: `width` columns starting
/// at world column `origin_x`, and the grid's rows. Normal mode uses the
/// screen ([`Span::screen`]); Endless uses a strip that follows the road
/// camera, slightly wider than the screen so the GPU can slide it by
/// fractions of a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// World column of buffer column 0.
    pub origin_x: i32,
    /// Columns.
    pub width: i32,
    /// Rows (the grid's rows).
    pub rows: i32,
}

impl Span {
    /// The screen: world columns `0..cols`.
    pub fn screen(grid: &Grid) -> Self {
        Self {
            origin_x: 0,
            width: grid.cols.max(1),
            rows: grid.rows.max(1),
        }
    }

    /// Buffer index of world column `x` and `row`, or `None` outside.
    pub fn index(&self, x: i32, row: i32) -> Option<usize> {
        let column = x - self.origin_x;
        let inside = (0..self.width).contains(&column) && (0..self.rows).contains(&row);
        inside.then(|| row as usize * self.width as usize + column as usize)
    }

    /// World column and row of buffer index `cell`.
    pub fn position(&self, cell: usize) -> (i32, i32) {
        let width = self.width.max(1) as usize;
        ((cell % width) as i32 + self.origin_x, (cell / width) as i32)
    }

    /// Cells in the buffer.
    pub fn cells(&self) -> usize {
        (self.width.max(0) * self.rows.max(0)) as usize
    }
}

/// The street's static layers for one grid and palette.
#[derive(Debug, Clone, PartialEq)]
pub struct StreetLayer {
    /// Road, curbs, and lane dashes, drawn before traffic.
    pub back: CellPaint,
    /// Background-verge trees, drawn after the road and before traffic.
    pub back_trees: CellPaint,
    /// Foreground-verge trees, drawn after traffic.
    pub front: CellPaint,
}

impl StreetLayer {
    /// Builds the road and the trees visible on `grid` for `seed`.
    /// O(road cells + visible trees).
    pub fn new(grid: &Grid, seed: u32, palette: &Palette) -> Self {
        Self::for_span(grid, seed, palette, Span::screen(grid))
    }

    /// Builds the road and trees over a strip of the world. Lane dashes and
    /// trees are placed by world column, so strips at different origins
    /// line up. O(road cells + trees in the strip).
    pub fn for_span(grid: &Grid, seed: u32, palette: &Palette, span: Span) -> Self {
        let mut back = CellPaint::new();
        let road_top = grid.ground_row;
        let (left, right) = (span.origin_x, span.origin_x + span.width);
        for offset in 0..ROAD_ROWS {
            let row = road_top + offset;
            for x in left..right {
                let color = road_color(offset, x, palette);
                push(&mut back, span, x, row, color);
            }
        }
        // Trees overlapping the strip, not just those wholly inside it, so
        // a tree straddling an edge is drawn (clipped) instead of popping in
        // as the strip moves.
        let (tree_left, tree_right) = (left - (TREE_WIDTH - 1), right + (TREE_WIDTH - 1));
        // Background trees stand on the road's top edge.
        let mut back_trees = CellPaint::new();
        for tree in trees_in(seed, tree_left, tree_right, false) {
            paint_tree(&mut back_trees, span, &tree, road_top, palette);
        }
        // Foreground trees stand on the bottom row of the screen.
        let mut front = CellPaint::new();
        for tree in trees_in(seed, tree_left, tree_right, true) {
            paint_tree(&mut front, span, &tree, grid.rows, palette);
        }
        Self {
            back,
            back_trees,
            front,
        }
    }

    /// Paints the layers drawn behind traffic (road, then background
    /// trees) into an RGBA buffer.
    pub fn apply_back(&self, rgba: &mut [u8]) {
        apply_paint(&self.back, rgba);
        apply_paint(&self.back_trees, rgba);
    }

    /// Paints the layer drawn in front of traffic into an RGBA buffer.
    pub fn apply_front(&self, rgba: &mut [u8]) {
        apply_paint(&self.front, rgba);
    }

    /// Like [`StreetLayer::apply_back`], but tints background trees toward
    /// `light` by `glow(x, row)` (0 = unlit, 1 = fully the light color).
    /// `glow` takes world columns; `span` says which strip `rgba` covers.
    pub fn apply_back_lit(
        &self,
        rgba: &mut [u8],
        span: Span,
        light: Rgb,
        glow: impl Fn(i32, i32) -> f64,
    ) {
        apply_paint(&self.back, rgba);
        apply_lit(&self.back_trees, rgba, span, light, &glow);
    }

    /// Like [`StreetLayer::apply_front`], with trees tinted by `glow`.
    pub fn apply_front_lit(
        &self,
        rgba: &mut [u8],
        span: Span,
        light: Rgb,
        glow: impl Fn(i32, i32) -> f64,
    ) {
        apply_lit(&self.front, rgba, span, light, &glow);
    }
}

/// Road color for a row `offset` (0 = top curb) and world column `x`.
pub fn road_color(offset: i32, x: i32, palette: &Palette) -> Rgb {
    let curb = offset == 0 || offset == ROAD_ROWS - 1;
    let center = offset == ROAD_ROWS / 2;
    if curb || (center && x.rem_euclid(DASH_PERIOD) < DASH_LENGTH) {
        palette.lane
    } else {
        palette.road
    }
}

/// Paints one tree whose trunk base sits just above row `ground` (the
/// sprite's last row is `ground - 1`).
fn paint_tree(paint: &mut CellPaint, span: Span, tree: &Tree, ground: i32, palette: &Palette) {
    let (leaf, shade, trunk) = tree_colors(tree, palette);
    let sprite = tree.sprite();
    let top = ground - sprite.len() as i32;
    for (dy, row) in sprite.iter().enumerate() {
        for (dx, ch) in row.chars().enumerate() {
            let color = match ch {
                'l' => leaf,
                's' => shade,
                't' => trunk,
                _ => continue,
            };
            push(paint, span, tree.x + dx as i32, top + dy as i32, color);
        }
    }
}

fn push(paint: &mut CellPaint, span: Span, x: i32, row: i32, color: Rgb) {
    if let Some(cell) = span.index(x, row) {
        paint.push((cell, color));
    }
}

/// Paints cell colors tinted toward `light` by `glow(x, row)`.
fn apply_lit(
    paint: &CellPaint,
    rgba: &mut [u8],
    span: Span,
    light: Rgb,
    glow: &impl Fn(i32, i32) -> f64,
) {
    for &(cell, color) in paint {
        let (x, row) = span.position(cell);
        let strength = glow(x, row);
        let color = if strength > 0.0 {
            crate::raster::mix(color, light, strength)
        } else {
            color
        };
        if let Some(pixel) = rgba.get_mut(cell * 4..cell * 4 + 3) {
            pixel.copy_from_slice(&color.0);
        }
    }
}

/// Sets alpha to opaque for every cell in `paint`, so a transparent overlay
/// shows exactly the painted cells. Cells outside the buffer are skipped.
pub fn mark_opaque(paint: &CellPaint, rgba: &mut [u8]) {
    for &(cell, _) in paint {
        if let Some(alpha) = rgba.get_mut(cell * 4 + 3) {
            *alpha = 255;
        }
    }
}

impl StreetLayer {
    /// Marks every street cell (road and both tree verges) opaque.
    pub fn mark_opaque(&self, rgba: &mut [u8]) {
        mark_opaque(&self.back, rgba);
        mark_opaque(&self.back_trees, rgba);
        mark_opaque(&self.front, rgba);
    }
}

/// Paints cell colors into an RGBA buffer, skipping cells outside it.
pub fn apply_paint(paint: &CellPaint, rgba: &mut [u8]) {
    for &(cell, color) in paint {
        if let Some(pixel) = rgba.get_mut(cell * 4..cell * 4 + 3) {
            pixel.copy_from_slice(&color.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::palette::palette_by_key;

    fn grid() -> Grid {
        // Normal mode with a road: ground row is rows - 7.
        Grid::new(1280, 720, true)
    }

    fn color_at(rgba: &[u8], grid: &Grid, x: i32, row: i32) -> Rgb {
        let i = (row * grid.cols + x) as usize * 4;
        Rgb([rgba[i], rgba[i + 1], rgba[i + 2]])
    }

    #[test]
    fn road_has_curbs_and_center_dashes() {
        let classic = palette_by_key("classic").unwrap();
        assert_eq!(road_color(0, 3, classic), classic.lane, "top curb");
        assert_eq!(road_color(6, 3, classic), classic.lane, "bottom curb");
        assert_eq!(road_color(1, 3, classic), classic.road);
        assert_eq!(road_color(3, 0, classic), classic.lane, "dash");
        assert_eq!(road_color(3, 3, classic), classic.lane, "dash end");
        assert_eq!(road_color(3, 4, classic), classic.road, "gap");
        assert_eq!(
            road_color(3, -5, classic),
            classic.lane,
            "negative x wraps correctly"
        );
    }

    #[test]
    fn street_covers_the_road_rows_and_stays_in_the_buffer() {
        let grid = grid();
        let classic = palette_by_key("classic").unwrap();
        let street = StreetLayer::new(&grid, 3, classic);
        let cells = (grid.cols * grid.rows) as usize;
        assert!(street
            .back
            .iter()
            .chain(&street.back_trees)
            .chain(&street.front)
            .all(|&(cell, _)| cell < cells));
        let mut rgba = vec![0u8; cells * 4];
        street.apply_back(&mut rgba);
        // Middle lane rows are road colored wherever no tree stands.
        assert!(
            (0..grid.cols).any(|x| color_at(&rgba, &grid, x, grid.ground_row + 2) == classic.road)
        );
        assert_eq!(color_at(&rgba, &grid, 0, grid.ground_row + 6), classic.lane);
    }

    #[test]
    fn trees_root_on_their_verges() {
        let grid = grid();
        let classic = palette_by_key("classic").unwrap();
        let street = StreetLayer::new(&grid, 8, classic);
        let rows = |paint: &CellPaint, colors: &[Rgb]| -> Vec<i32> {
            paint
                .iter()
                .filter(|(_, color)| colors.contains(color))
                .map(|&(cell, _)| cell as i32 / grid.cols)
                .collect()
        };
        let back_trees = trees_in(8, 0, grid.cols, false);
        let front_trees = trees_in(8, 0, grid.cols, true);
        assert!(!back_trees.is_empty() && !front_trees.is_empty());
        let trunk = tree_colors(&back_trees[0], classic).2;
        let back_rows = rows(&street.back_trees, &[trunk]);
        assert_eq!(
            back_rows.iter().max(),
            Some(&(grid.ground_row - 1)),
            "on the road edge"
        );
        let front_trunk = tree_colors(&front_trees[0], classic).2;
        let front_rows = rows(&street.front, &[front_trunk]);
        assert_eq!(
            front_rows.iter().max(),
            Some(&(grid.rows - 1)),
            "on the bottom row"
        );
    }

    #[test]
    fn strips_at_different_origins_agree_on_every_world_cell() {
        let grid = grid();
        let classic = palette_by_key("classic").unwrap();
        let draw = |origin_x: i32| {
            let span = Span {
                origin_x,
                width: grid.cols + 4,
                rows: grid.rows,
            };
            let street = StreetLayer::for_span(&grid, 6, classic, span);
            let mut rgba = vec![0u8; span.cells() * 4];
            street.apply_back(&mut rgba);
            street.apply_front(&mut rgba);
            street.mark_opaque(&mut rgba);
            (span, rgba)
        };
        let (a_span, a) = draw(-2);
        let (b_span, b) = draw(37);
        let mut compared = 0;
        for x in 37..a_span.origin_x + a_span.width {
            for row in 0..grid.rows {
                let (i, j) = (a_span.index(x, row).unwrap(), b_span.index(x, row).unwrap());
                assert_eq!(a[i * 4..i * 4 + 4], b[j * 4..j * 4 + 4], "({x}, {row})");
                compared += 1;
            }
        }
        assert!(compared > 0);
        // The screen span is the strip at origin 0.
        assert_eq!(Span::screen(&grid).origin_x, 0);
        assert_eq!(
            StreetLayer::new(&grid, 6, classic),
            StreetLayer::for_span(&grid, 6, classic, Span::screen(&grid))
        );
    }

    #[test]
    fn span_index_and_position_round_trip_and_reject_outside_cells() {
        let span = Span {
            origin_x: -3,
            width: 10,
            rows: 4,
        };
        assert_eq!(span.cells(), 40);
        for cell in 0..span.cells() {
            let (x, row) = span.position(cell);
            assert_eq!(span.index(x, row), Some(cell));
        }
        assert_eq!(span.index(-4, 0), None);
        assert_eq!(span.index(7, 0), None);
        assert_eq!(span.index(0, 4), None);
        assert_eq!(span.index(0, -1), None);
    }

    #[test]
    fn same_seed_and_palette_give_the_same_street() {
        let grid = grid();
        let artdeco = palette_by_key("artdeco").unwrap();
        assert_eq!(
            StreetLayer::new(&grid, 4, artdeco),
            StreetLayer::new(&grid, 4, artdeco)
        );
    }
}
