//! Endless mode's scrolling city (Rebuild Prompt, "Time, Reveal, And Endless
//! Motion").
//!
//! Four independent layer cities, each from its own seeded generator, move
//! under one root camera. The camera runs at `2 * speed` cells per second and
//! each layer follows it at `[0.2, 0.4, 0.65, 1]` of that, so far layers
//! drift slowly (parallax). Each layer keeps a buffer `2 * cols + 64` wide;
//! when its view nears the buffer's right edge, the buffer is rebased to
//! `floor(viewX) - 4`, generation continues ahead to `origin + bufW + 32`,
//! buildings behind `origin - 4` are dropped, and the buffer is redrawn.
//! Memory stays bounded however long it runs.
//!
//! Starry Journey extension (owner, 2026-11-03): an Endless city is first
//! built cell by cell, like Normal mode, and only then starts moving.
//! [`EndlessCity::initial_buildings`] gives the buildings on screen at camera
//! 0 for that construction phase, so the finished construction image and the
//! first scrolling frame are the same.
//!
//! For now layers are drawn at whole-cell positions. Smooth fractional
//! reconstruction arrives with the Endless compositor.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::model::building::Building;
use crate::model::city::{endless_layer_generator, CityGenerator};
use crate::model::grid::Grid;
use crate::model::palette::Palette;
use crate::raster::{CityRaster, RasterOptions, WindowLighting};

/// Layer speeds relative to the root camera, farthest to nearest.
pub const LAYER_SPEEDS: [f64; 4] = [0.2, 0.4, 0.65, 1.0];
/// Root camera speed in cells per second at 1x.
pub const CAMERA_SPEED: f64 = 2.0;
/// Longest simulated motion step per frame, in seconds.
pub const MAX_MOTION_STEP: f64 = 1.0 / 30.0;
/// A new layer buffer's world origin.
pub const ORIGIN_START: i32 = -4;
/// Columns kept behind the view when rebasing, and pruned behind the origin.
pub const ORIGIN_MARGIN: i32 = 4;
/// Columns generated past the buffer's right edge.
pub const GENERATE_AHEAD: i32 = 32;

/// A fresh image revision, unique across every city in the process, so a
/// rebuilt city never looks unchanged to the uploader.
fn next_revision() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// sRGB decode of one encoded channel in `0..=1` (Rebuild Prompt).
pub fn decode_srgb(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Reconstructs one screen row of a layer at a fractional position: the CPU
/// mirror of the presentation shader, used to test its math.
///
/// The layer is treated as a sharp device-resolution image in which every
/// cell is `cell` pixels wide. Screen pixel `p` covers source pixels
/// `[p + offset, p + offset + 1)`, so it takes the two source pixels it
/// overlaps, decoded to linear light and premultiplied by alpha, weighted by
/// their overlap. Cells outside the row are transparent. Returns linear,
/// premultiplied RGBA per screen pixel. O(width).
pub fn reconstruct_row(cells: &[[u8; 4]], cell: u32, offset: f64, width: usize) -> Vec<[f64; 4]> {
    let cell = f64::from(cell.max(1));
    let texel = |pixel: f64| -> [f64; 4] {
        let index = (pixel / cell).floor();
        if index < 0.0 || index as usize >= cells.len() {
            return [0.0; 4];
        }
        let [r, g, b, a] = cells[index as usize];
        let alpha = f64::from(a) / 255.0;
        let linear = |v: u8| decode_srgb(f64::from(v) / 255.0) * alpha;
        [linear(r), linear(g), linear(b), alpha]
    };
    (0..width)
        .map(|p| {
            let u = p as f64 + offset;
            let left = u.floor();
            let t = u - left;
            let (a, b) = (texel(left), texel(left + 1.0));
            [0, 1, 2, 3].map(|i| a[i] * (1.0 - t) + b[i] * t)
        })
        .collect()
}

/// A layer buffer's width for a grid: `2 * cols + 64`.
pub fn buffer_width(cols: i32) -> i32 {
    2 * cols.max(1) + 64
}

/// One parallax layer: its generator, its buffer origin, and the drawn buffer.
#[derive(Debug, Clone)]
struct EndlessLayer {
    generator: CityGenerator,
    origin_x: i32,
    raster: CityRaster,
    /// Increases every time the buffer is redrawn (rebase or palette).
    revision: u64,
}

/// One layer's drawn buffer, for uploading to the GPU.
#[derive(Debug, Clone, Copy)]
pub struct LayerImage<'a> {
    /// RGBA bytes, row-major, sRGB-encoded; alpha is 0 on sky, 255 on
    /// buildings.
    pub rgba: &'a [u8],
    /// Columns.
    pub width: usize,
    /// Rows.
    pub height: usize,
    /// World x of column 0.
    pub origin_x: i32,
    /// Changes whenever the image changes.
    pub revision: u64,
}

impl EndlessLayer {
    fn new(seed: u32, layer: u8, grid: &Grid, palette: &Palette) -> Self {
        let mut generator = endless_layer_generator(seed, layer, grid.max_h);
        generator.extend_to(ORIGIN_START + buffer_width(grid.cols) + GENERATE_AHEAD);
        let raster = Self::draw(&generator, ORIGIN_START, grid, palette);
        Self {
            generator,
            origin_x: ORIGIN_START,
            raster,
            revision: next_revision(),
        }
    }

    /// Draws the buffer; sky cells are made transparent so far layers and
    /// the sky show through.
    fn draw(
        generator: &CityGenerator,
        origin_x: i32,
        grid: &Grid,
        palette: &Palette,
    ) -> CityRaster {
        let mut raster = Self::render(generator, origin_x, grid, palette);
        for (cell, owner) in raster.owner.iter().enumerate() {
            if *owner < 0 {
                raster.rgba[cell * 4 + 3] = 0;
            }
        }
        raster
    }

    fn render(
        generator: &CityGenerator,
        origin_x: i32,
        grid: &Grid,
        palette: &Palette,
    ) -> CityRaster {
        CityRaster::render_options(
            grid,
            generator.buildings(),
            &[],
            palette,
            RasterOptions {
                lighting: WindowLighting::Target,
                origin_x,
                width: Some(buffer_width(grid.cols) as usize),
                endless_tint: true,
            },
        )
    }

    /// Rebases and redraws the buffer if the view at `view_x` would run past
    /// it. Returns true if it rebased. O(1) unless rebasing, then
    /// O(buffer cells + new buildings).
    fn ensure_covers(&mut self, view_x: f64, grid: &Grid, palette: &Palette) -> bool {
        let left = view_x.floor() as i32;
        let width = buffer_width(grid.cols);
        // One spare column on the right for fractional positions later.
        let covered = left >= self.origin_x && left + grid.cols < self.origin_x + width;
        if covered {
            return false;
        }
        self.origin_x = left - ORIGIN_MARGIN;
        self.generator
            .extend_to(self.origin_x + width + GENERATE_AHEAD);
        self.generator.prune_before(self.origin_x - ORIGIN_MARGIN);
        self.raster = Self::draw(&self.generator, self.origin_x, grid, palette);
        self.revision = next_revision();
        true
    }
}

/// The four scrolling layer cities under one camera.
#[derive(Debug, Clone)]
pub struct EndlessCity {
    grid: Grid,
    camera: f64,
    layers: Vec<EndlessLayer>,
}

impl EndlessCity {
    /// A new city with the camera at 0. Layer `L` is seeded with
    /// `seed XOR (0x85ebca6b + L)`.
    pub fn new(seed: u32, grid: &Grid, palette: &Palette) -> Self {
        Self::with_camera(seed, grid, palette, 0.0)
    }

    /// A city with the camera at `camera` cells, as if it had scrolled there
    /// (used after a resize). Generation is deterministic, so the buildings
    /// match a city that scrolled the whole way.
    pub fn with_camera(seed: u32, grid: &Grid, palette: &Palette, camera: f64) -> Self {
        let mut city = Self {
            grid: *grid,
            camera: camera.max(0.0),
            layers: (0..4)
                .map(|layer| EndlessLayer::new(seed, layer, grid, palette))
                .collect(),
        };
        city.ensure_covered(palette);
        city
    }

    /// The root camera position in cells.
    pub fn camera(&self) -> f64 {
        self.camera
    }

    /// The world x at the left edge of the screen for `layer`.
    pub fn view_x(&self, layer: usize) -> f64 {
        self.camera * LAYER_SPEEDS[layer]
    }

    /// The world origin of `layer`'s buffer.
    pub fn origin_x(&self, layer: usize) -> i32 {
        self.layers[layer].origin_x
    }

    /// Buildings currently held, over all layers.
    pub fn building_count(&self) -> usize {
        self.layers
            .iter()
            .map(|layer| layer.generator.buildings().len())
            .sum()
    }

    /// Every building of every layer, farthest layer first, with windows lit
    /// to their targets. At camera 0 these are exactly the buildings on
    /// screen, for the construction phase.
    pub fn initial_buildings(&self) -> Vec<Building> {
        self.layers
            .iter()
            .flat_map(|layer| layer.generator.buildings().iter().cloned())
            .collect()
    }

    /// Moves the camera by one frame: at most [`MAX_MOTION_STEP`] seconds
    /// of motion at `CAMERA_SPEED * speed` cells per second. Returns true if
    /// any layer moved to a new whole column.
    pub fn advance(&mut self, dt: f64, speed: f64, palette: &Palette) -> bool {
        let step = if dt.is_finite() {
            dt.clamp(0.0, MAX_MOTION_STEP)
        } else {
            0.0
        };
        let before: Vec<i32> = (0..4).map(|l| self.view_x(l).floor() as i32).collect();
        self.camera += CAMERA_SPEED * speed.max(0.0) * step;
        self.ensure_covered(palette);
        (0..4).any(|l| self.view_x(l).floor() as i32 != before[l])
    }

    /// Redraws every layer buffer in another palette.
    pub fn set_palette(&mut self, palette: &Palette) {
        for layer in &mut self.layers {
            layer.raster =
                EndlessLayer::draw(&layer.generator, layer.origin_x, &self.grid, palette);
            layer.revision = next_revision();
        }
    }

    /// The drawn buffer of `layer` (0 farthest to 3 nearest).
    pub fn layer_image(&self, layer: usize) -> LayerImage<'_> {
        let layer = &self.layers[layer];
        LayerImage {
            rgba: &layer.raster.rgba,
            width: layer.raster.width,
            height: layer.raster.height,
            origin_x: layer.origin_x,
            revision: layer.revision,
        }
    }

    /// How far into `layer`'s buffer the screen's left edge is, in device
    /// pixels: `(viewX - originX) * cell`. Fractional; computed in f64 so it
    /// stays exact however far the camera has gone.
    pub fn source_offset_px(&self, layer: usize) -> f64 {
        (self.view_x(layer) - f64::from(self.layers[layer].origin_x)) * f64::from(self.grid.cell)
    }

    fn ensure_covered(&mut self, palette: &Palette) {
        for index in 0..self.layers.len() {
            let view_x = self.view_x(index);
            self.layers[index].ensure_covers(view_x, &self.grid, palette);
        }
    }

    /// Composes the screen: `sky` (the background and stars, grid-sized
    /// RGBA) with each layer drawn over it far to near at its whole-cell
    /// view position. `buildings` marks the cells a building covers.
    /// O(4 × cols × max_h).
    pub fn compose(&self, sky: &[u8], frame: &mut Vec<u8>, buildings: &mut Vec<bool>) {
        let cols = self.grid.cols.max(1) as usize;
        let rows = self.grid.rows.max(1) as usize;
        frame.clear();
        frame.extend_from_slice(sky);
        buildings.clear();
        buildings.resize(cols * rows, false);
        // Buildings never rise above max_h, so the sky rows above are skipped.
        let top = (self.grid.ground_row - self.grid.max_h).max(0) as usize;
        let bottom = (self.grid.ground_row.max(0) as usize).min(rows);
        for (index, layer) in self.layers.iter().enumerate() {
            let raster = &layer.raster;
            let offset = self.view_x(index).floor() as i32 - layer.origin_x;
            if offset < 0 || offset as usize + cols > raster.width {
                continue; // unreachable: ensure_covered keeps the view inside
            }
            let offset = offset as usize;
            for row in top..bottom {
                for col in 0..cols {
                    let source = row * raster.width + col + offset;
                    if raster.owner[source] < 0 {
                        continue;
                    }
                    let target = row * cols + col;
                    frame[target * 4..target * 4 + 4]
                        .copy_from_slice(&raster.rgba[source * 4..source * 4 + 4]);
                    buildings[target] = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::grid::CellScaling;
    use crate::model::palette::palette_by_key;
    use crate::raster::generate_stars;

    fn grid() -> Grid {
        Grid::with_scaling(480, 270, true, CellScaling::Fixed(3))
    }

    fn classic() -> &'static Palette {
        palette_by_key("classic").unwrap()
    }

    fn sky(grid: &Grid, seed: u32, palette: &Palette) -> Vec<u8> {
        CityRaster::render(grid, &[], &generate_stars(seed, grid), palette).rgba
    }

    fn composed(city: &EndlessCity, sky: &[u8]) -> (Vec<u8>, Vec<bool>) {
        let (mut frame, mut mask) = (Vec::new(), Vec::new());
        city.compose(sky, &mut frame, &mut mask);
        (frame, mask)
    }

    #[test]
    fn buffer_is_two_screens_plus_64() {
        assert_eq!(buffer_width(640), 1344);
        assert_eq!(buffer_width(0), 66);
    }

    #[test]
    fn camera_moves_two_cells_per_second_times_speed_with_parallax() {
        let grid = grid();
        let mut city = EndlessCity::new(1, &grid, classic());
        for _ in 0..300 {
            city.advance(1.0 / 30.0, 1.0, classic());
        }
        assert!((city.camera() - 20.0).abs() < 1e-9);
        for (layer, speed) in LAYER_SPEEDS.iter().enumerate() {
            assert!((city.view_x(layer) - 20.0 * speed).abs() < 1e-9);
        }
        let mut fast = EndlessCity::new(1, &grid, classic());
        for _ in 0..300 {
            fast.advance(1.0 / 30.0, 4.0, classic());
        }
        assert!((fast.camera() - 80.0).abs() < 1e-9);
    }

    #[test]
    fn long_frames_move_at_most_one_thirtieth_of_a_second() {
        let mut city = EndlessCity::new(1, &grid(), classic());
        city.advance(0.25, 1.0, classic());
        assert!((city.camera() - CAMERA_SPEED / 30.0).abs() < 1e-12);
        city.advance(-1.0, 1.0, classic());
        city.advance(f64::NAN, 1.0, classic());
        assert!((city.camera() - CAMERA_SPEED / 30.0).abs() < 1e-12);
        // Small frames use their real dt.
        city.advance(0.01, 1.0, classic());
        assert!((city.camera() - CAMERA_SPEED * (1.0 / 30.0 + 0.01)).abs() < 1e-12);
    }

    #[test]
    fn advance_reports_whole_column_changes() {
        let mut city = EndlessCity::new(1, &grid(), classic());
        // 0.4 cells/s on the nearest layer at 0.1x is under a column a frame.
        assert!(!city.advance(1.0 / 30.0, 0.1, classic()));
        let mut moved = false;
        for _ in 0..30 {
            moved |= city.advance(1.0 / 30.0, 1.0, classic());
        }
        assert!(moved);
    }

    #[test]
    fn buffers_always_cover_the_view_and_memory_stays_bounded() {
        let grid = grid();
        let mut city = EndlessCity::new(5, &grid, classic());
        let start = city.building_count();
        let mut most = start;
        // Two hours at 4x: the nearest layer travels 57,600 cells.
        for frame in 0..(2 * 3600 * 30) {
            city.advance(1.0 / 30.0, 4.0, classic());
            if frame % 3000 == 0 {
                for layer in 0..4 {
                    let left = city.view_x(layer).floor() as i32;
                    let origin = city.origin_x(layer);
                    assert!(left >= origin);
                    assert!(left + grid.cols < origin + buffer_width(grid.cols));
                }
                most = most.max(city.building_count());
            }
        }
        assert!(city.camera() > 57_000.0);
        assert!(most < start * 3, "{most} vs {start}");
    }

    #[test]
    fn a_long_scroll_matches_a_city_built_at_that_camera() {
        let grid = grid();
        let palette = classic();
        let sky = sky(&grid, 9, palette);
        let mut scrolled = EndlessCity::new(9, &grid, palette);
        for _ in 0..(20 * 60 * 30) {
            scrolled.advance(1.0 / 30.0, 4.0, palette);
        }
        let fresh = EndlessCity::with_camera(9, &grid, palette, scrolled.camera());
        assert_eq!(composed(&scrolled, &sky), composed(&fresh, &sky));
    }

    #[test]
    fn construction_image_equals_the_first_scrolling_frame() {
        let grid = grid();
        let palette = palette_by_key("artdeco").unwrap();
        let stars = generate_stars(3, &grid);
        let city = EndlessCity::new(3, &grid, palette);
        let built = CityRaster::render_options(
            &grid,
            &city.initial_buildings(),
            &stars,
            palette,
            RasterOptions {
                lighting: WindowLighting::Target,
                endless_tint: true,
                ..RasterOptions::default()
            },
        );
        let sky = CityRaster::render(&grid, &[], &stars, palette).rgba;
        let (frame, mask) = composed(&city, &sky);
        assert_eq!(frame, built.rgba);
        let owned: Vec<bool> = built.owner.iter().map(|&o| o >= 0).collect();
        assert_eq!(mask, owned);
    }

    #[test]
    fn layers_scroll_at_their_own_speeds() {
        let grid = grid();
        let palette = classic();
        let sky = sky(&grid, 2, palette);
        let mut city = EndlessCity::new(2, &grid, palette);
        let (before, _) = composed(&city, &sky);
        // 25 s at 1x: the nearest layer moves 50 cells, the farthest 10.
        for _ in 0..(25 * 30) {
            city.advance(1.0 / 30.0, 1.0, palette);
        }
        assert_eq!(city.view_x(3).floor() as i32, 50);
        assert_eq!(city.view_x(0).floor() as i32, 10);
        let (after, _) = composed(&city, &sky);
        assert_ne!(before, after);
    }

    #[test]
    fn palette_change_recolors_without_moving() {
        let grid = grid();
        let mut city = EndlessCity::new(4, &grid, classic());
        let ega = palette_by_key("ega").unwrap();
        city.set_palette(ega);
        let fresh = EndlessCity::new(4, &grid, ega);
        let sky = sky(&grid, 4, ega);
        assert_eq!(composed(&city, &sky), composed(&fresh, &sky));
    }

    fn luminance_sum(pixels: &[[f64; 4]]) -> f64 {
        pixels.iter().map(|p| p[0] + p[1] + p[2]).sum()
    }

    #[test]
    fn whole_pixel_offsets_reproduce_the_sharp_image() {
        let cells = [[0, 0, 0, 255], [200, 100, 50, 255], [10, 20, 30, 0]];
        let row = reconstruct_row(&cells, 3, 3.0, 6);
        // Offset 3 px = one cell: pixels 0..3 show cell 1, 3..6 cell 2.
        let expected = [
            decode_srgb(200.0 / 255.0),
            decode_srgb(100.0 / 255.0),
            decode_srgb(50.0 / 255.0),
            1.0,
        ];
        for pixel in &row[0..3] {
            for (got, want) in pixel.iter().zip(expected) {
                assert!((got - want).abs() < 1e-12);
            }
        }
        // A transparent cell is premultiplied to nothing.
        assert!(row[3..6].iter().all(|p| *p == [0.0; 4]));
    }

    #[test]
    fn subpixel_motion_keeps_brightness_constant_in_linear_light() {
        // A mid-grey cell moving across black, a quarter pixel at a time.
        let grey = [128, 128, 128, 255];
        let cells = [
            [0, 0, 0, 255],
            [0, 0, 0, 255],
            grey,
            [0, 0, 0, 255],
            [0, 0, 0, 255],
        ];
        let reference = luminance_sum(&reconstruct_row(&cells, 3, 0.0, 12));
        let mut encoded_sums = Vec::new();
        for step in 0..=12 {
            let offset = f64::from(step) * 0.25;
            let row = reconstruct_row(&cells, 3, offset, 12);
            let sum = luminance_sum(&row);
            assert!((sum - reference).abs() < 1e-9, "offset {offset}: {sum}");
            // What interpolating the encoded values would give: the edge
            // pixels come out too dark, so the sum dips between pixels.
            let encoded: f64 = (0..12)
                .map(|p| {
                    let u = p as f64 + offset;
                    let t = u - u.floor();
                    let at = |x: f64| {
                        let i = (x / 3.0).floor() as usize;
                        f64::from(cells.get(i).map_or(0, |c| c[0])) / 255.0
                    };
                    3.0 * decode_srgb(at(u.floor()) * (1.0 - t) + at(u.floor() + 1.0) * t)
                })
                .sum();
            encoded_sums.push(encoded);
        }
        let dip = encoded_sums.iter().cloned().fold(f64::MAX, f64::min);
        assert!(
            dip < reference * 0.95,
            "encoded-space blending dims a moving edge (flicker): {dip} vs {reference}"
        );
    }

    #[test]
    fn edges_outside_the_buffer_are_transparent() {
        let cells = [[255, 255, 255, 255]];
        // A 2-pixel cell shifted 1.5 px right covers pixels 1..=3 as
        // 1/2, 1, 1/2; nothing outside the buffer adds coverage.
        let row = reconstruct_row(&cells, 2, -1.5, 5);
        let alphas: Vec<f64> = row.iter().map(|p| p[3]).collect();
        assert_eq!(alphas, vec![0.0, 0.5, 1.0, 0.5, 0.0]);
    }

    #[test]
    fn layer_images_are_transparent_on_sky_and_track_revisions() {
        let grid = grid();
        let palette = classic();
        let mut city = EndlessCity::new(8, &grid, palette);
        let image = city.layer_image(3);
        assert_eq!(image.width, buffer_width(grid.cols) as usize);
        assert_eq!(image.origin_x, ORIGIN_START);
        let alphas: Vec<u8> = image.rgba.chunks(4).map(|p| p[3]).collect();
        assert!(alphas.contains(&0) && alphas.contains(&255));
        assert!(alphas.iter().all(|a| *a == 0 || *a == 255));
        let first = image.revision;
        city.set_palette(palette_by_key("vga").unwrap());
        let recolored = city.layer_image(3).revision;
        assert_ne!(recolored, first);
        // Scroll until the nearest layer rebases.
        while city.origin_x(3) == ORIGIN_START {
            city.advance(1.0 / 30.0, 4.0, palette);
        }
        assert_ne!(city.layer_image(3).revision, recolored);
        // A new city never reuses a revision.
        let other = EndlessCity::new(8, &grid, palette);
        assert_ne!(other.layer_image(3).revision, city.layer_image(3).revision);
    }

    #[test]
    fn source_offset_is_fractional_device_pixels_into_the_buffer() {
        let grid = grid();
        let mut city = EndlessCity::new(8, &grid, classic());
        assert_eq!(city.source_offset_px(0), f64::from(-ORIGIN_START * 3));
        city.advance(0.02, 1.0, classic());
        // Camera 0.04 cells; the farthest layer is at 0.008 cells.
        let expected = (0.04 * 0.2 + 4.0) * 3.0;
        assert!((city.source_offset_px(0) - expected).abs() < 1e-12);
    }

    #[test]
    fn endless_bodies_blend_toward_the_layer_tint() {
        let grid = grid();
        let city = EndlessCity::new(6, &grid, classic());
        let building = &city.initial_buildings()[0];
        let plain = crate::raster::body_color(building, classic());
        let tinted = crate::raster::endless_body_color(building, classic());
        let target = classic().layer_tint_target(usize::from(building.layer));
        assert_eq!(tinted, crate::raster::mix(plain, target, 0.1));
    }
}
