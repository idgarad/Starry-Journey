//! City glow on the clouds (owner decisions, 2026-11-04: cloud lighting
//! starts with the city's glow, "from the windows"). The lit windows below
//! a cloud tint its underside with their colors, stronger over dense or
//! tall parts of the city.
//!
//! This module turns a city image into a **glow row**: for every column,
//! the linear-light color of all the window cells in it, blurred sideways
//! so the glow spreads softly, and scaled so a busy column is about full
//! strength. The GPU reads the row under each cloud pixel every frame (see
//! `present.wgsl`), so the glow stays lined up with the city while Endless
//! scrolls; the row only needs rebuilding when the city image changes.

use crate::endless::decode_srgb;
use crate::raster::WINDOW_BODY;

/// Half-width of the sideways blur, in cells: the glow spreads this far
/// either side of its windows.
pub const GLOW_RADIUS: usize = 16;

/// Window cells per column (fully lit, after the blur) that give full
/// glow. Tuned with snapshots: a typical downtown column has 20-40 lit
/// window cells.
pub const GLOW_FULL: f64 = 30.0;

/// The glow row of a `width x height` city image: per column, linear RGB
/// in `0..1`. `rgba` is the sRGB image; `window_index` marks window cells
/// (anything above [`WINDOW_BODY`]); `shown` says whether a cell is drawn
/// (for example built yet). O(cells).
///
/// # Panics
///
/// Panics if `rgba` or `window_index` does not match `width x height`.
pub fn glow_row(
    rgba: &[u8],
    window_index: &[i32],
    width: usize,
    height: usize,
    shown: impl Fn(usize) -> bool,
) -> Vec<[f32; 3]> {
    let cells = width * height;
    assert_eq!(rgba.len(), cells * 4, "rgba size");
    assert_eq!(window_index.len(), cells, "window index size");
    let mut columns = vec![[0.0f64; 3]; width];
    for (cell, &window) in window_index.iter().enumerate() {
        if window <= WINDOW_BODY || !shown(cell) {
            continue;
        }
        let pixel = &rgba[cell * 4..cell * 4 + 4];
        if pixel[3] == 0 {
            continue;
        }
        let column = &mut columns[cell % width];
        for (sum, &channel) in column.iter_mut().zip(pixel) {
            *sum += decode_srgb(f64::from(channel) / 255.0);
        }
    }
    blur(&columns, GLOW_RADIUS)
        .into_iter()
        .map(|sum| sum.map(|c| (c / GLOW_FULL).clamp(0.0, 1.0) as f32))
        .collect()
}

/// A box blur of `columns` with half-width `radius`, as a mean over the
/// window (edges count missing columns as dark). O(n) with prefix sums.
fn blur(columns: &[[f64; 3]], radius: usize) -> Vec<[f64; 3]> {
    let n = columns.len();
    let mut prefix = vec![[0.0f64; 3]; n + 1];
    for (i, column) in columns.iter().enumerate() {
        prefix[i + 1] = std::array::from_fn(|c| prefix[i][c] + column[c]);
    }
    let span = (2 * radius + 1) as f64;
    (0..n)
        .map(|i| {
            let lo = i.saturating_sub(radius);
            let hi = (i + radius + 1).min(n);
            std::array::from_fn(|c| (prefix[hi][c] - prefix[lo][c]) / span)
        })
        .collect()
}

/// The glow row as RGBA bytes for a linear (not sRGB) texture.
pub fn glow_rgba(glow: &[[f32; 3]]) -> Vec<u8> {
    glow.iter()
        .flat_map(|rgb| {
            let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
            [byte(rgb[0]), byte(rgb[1]), byte(rgb[2]), 255]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `width x height` image of sky with lit windows at `windows`
    /// (column, row) in `color`.
    fn city(
        width: usize,
        height: usize,
        windows: &[(usize, usize)],
        color: [u8; 3],
    ) -> (Vec<u8>, Vec<i32>) {
        let mut rgba = vec![0u8; width * height * 4];
        let mut index = vec![-1; width * height];
        for (n, &(x, y)) in windows.iter().enumerate() {
            let cell = y * width + x;
            rgba[cell * 4..cell * 4 + 4].copy_from_slice(&[color[0], color[1], color[2], 255]);
            index[cell] = n as i32;
        }
        (rgba, index)
    }

    #[test]
    fn windows_glow_in_their_color_and_spread_softly() {
        let windows: Vec<(usize, usize)> = (0..20).map(|y| (50, y)).collect();
        let (rgba, index) = city(100, 20, &windows, [255, 200, 0]);
        let glow = glow_row(&rgba, &index, 100, 20, |_| true);
        let middle = glow[50];
        assert!(
            middle[0] > middle[1] && middle[1] > middle[2],
            "warm: {middle:?}"
        );
        assert_eq!(middle[2], 0.0);
        // Spread out evenly over the blur, and gone beyond it.
        assert_eq!(glow[50 + GLOW_RADIUS], middle);
        assert_eq!(glow[50 + GLOW_RADIUS + 1], [0.0; 3]);
        assert!(glow[40][0] > 0.0);
    }

    #[test]
    fn denser_columns_glow_brighter_and_never_past_full() {
        let few: Vec<(usize, usize)> = (0..5).map(|y| (20, y)).collect();
        let many: Vec<(usize, usize)> = (0..40).map(|y| (60, y)).collect();
        let (rgba, index) = city(100, 40, &[few, many].concat(), [255, 255, 255]);
        let glow = glow_row(&rgba, &index, 100, 40, |_| true);
        assert!(glow[60][0] > 2.0 * glow[20][0]);
        // A wall of windows saturates at full.
        let wall: Vec<(usize, usize)> = (0..100)
            .flat_map(|x| (0..200).map(move |y| (x, y)))
            .collect();
        let (rgba, index) = city(100, 200, &wall, [255, 255, 255]);
        let glow = glow_row(&rgba, &index, 100, 200, |_| true);
        assert!(glow.iter().all(|c| c.iter().all(|&v| v <= 1.0)));
        assert_eq!(glow[50], [1.0; 3]);
    }

    #[test]
    fn only_shown_window_cells_glow() {
        let (mut rgba, mut index) = city(60, 10, &[(30, 5)], [255, 255, 255]);
        // A body cell (not a window) and a hidden window glow nothing.
        index[2] = WINDOW_BODY;
        rgba[8..12].copy_from_slice(&[255, 255, 255, 255]);
        let none = glow_row(&rgba, &index, 60, 10, |cell| cell != 5 * 60 + 30);
        assert!(none.iter().all(|c| *c == [0.0; 3]));
        let empty = glow_row(&[], &[], 0, 0, |_| true);
        assert!(empty.is_empty());
    }

    #[test]
    #[should_panic(expected = "window index size")]
    fn mismatched_sizes_are_rejected() {
        let _ = glow_row(&[0; 8], &[0], 2, 1, |_| true);
    }

    #[test]
    fn glow_bytes_are_linear_rgba() {
        assert_eq!(
            glow_rgba(&[[1.0, 0.5, 0.0], [2.0, -1.0, 0.25]]),
            vec![255, 128, 0, 255, 255, 0, 64, 255]
        );
    }
}
