//! CPU rasterizer: turns a generated city into an RGBA buffer on the logical
//! grid, one texel per cell (Rebuild Prompt, "Rasterization And Color Space").
//!
//! Ownership is resolved for the whole city before any pixel is painted, so
//! nearer layers hide farther ones and a window hidden behind a nearer
//! building is never painted through it. Painting then follows the
//! ownership arrays. The GPU only scales the finished buffer up by the cell
//! size with nearest-neighbor sampling.
//!
//! This first version covers Normal mode with fully built cities and steady
//! lights. Reveal, flicker, beacon blinking, landmark color cycling,
//! twinkles, and the palette materials come in later features.

use crate::model::building::Building;
use crate::model::grid::Grid;
use crate::model::palette::{
    Palette, Rgb, BODY_FADE_MULTIPLIER, DEPTH_FADES, ENDLESS_LAYER_TINT_BLEND,
};
use crate::model::rng::{ambient_seed, js_round, Mulberry32};
use crate::model::windows::shade_from_level;

/// Stars per sky cell: `round(cols * groundRow * 0.0035)` stars in total.
pub const STAR_DENSITY: f64 = 0.0035;
/// Largest fade of a star toward the background.
pub const STAR_MAX_FADE: f64 = 0.6;
/// How far an accent building's body moves toward the palette accent
/// (assumption AS-R005).
pub const ACCENT_MIX: f64 = 0.35;
/// HSL saturation for generated landmark colors (assumption AS-R006).
pub const DECOR_SATURATION: f64 = 0.8;
/// `owner` value for sky cells.
pub const OWNER_SKY: i32 = -1;
/// `window_index` value for building body cells.
pub const WINDOW_BODY: i32 = -1;
/// `window_index` value for mast cells.
pub const WINDOW_MAST: i32 = -2;

/// Which window state to paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowLighting {
    /// Each window's current `lit` flag.
    Current,
    /// Each window's `target` flag: the fully built city's look.
    Target,
}

/// One star on the sky.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Star {
    /// Logical column.
    pub x: i32,
    /// Logical row (0 is the top).
    pub row: i32,
    /// Fade toward the background, `0..0.6`.
    pub fade: f64,
}

/// Places the seeded stars above the ground row, using the root ambient
/// stream (`seed XOR 0x9e3779b9`). O(stars).
pub fn generate_stars(seed: u32, grid: &Grid) -> Vec<Star> {
    let mut rng = Mulberry32::new(ambient_seed(seed));
    let count = js_round(f64::from(grid.cols) * f64::from(grid.ground_row) * STAR_DENSITY) as usize;
    if grid.cols < 1 || grid.ground_row < 1 {
        return Vec::new();
    }
    (0..count)
        .map(|_| Star {
            x: rng.rand_int(0, i64::from(grid.cols) - 1) as i32,
            row: rng.rand_int(0, i64::from(grid.ground_row) - 1) as i32,
            fade: rng.next_f64() * STAR_MAX_FADE,
        })
        .collect()
}

/// Linear blend of two colors in encoded space: `a + (b - a) * t`.
pub fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let channel = |x: u8, y: u8| js_round(f64::from(x) + (f64::from(y) - f64::from(x)) * t) as u8;
    Rgb([
        channel(a.0[0], b.0[0]),
        channel(a.0[1], b.0[1]),
        channel(a.0[2], b.0[2]),
    ])
}

/// Converts HSL (hue in degrees, saturation and lightness in `0..=1`) to RGB.
pub fn hsl(hue: f64, saturation: f64, lightness: f64) -> Rgb {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = hue.rem_euclid(360.0) / 60.0;
    let second = chroma * (1.0 - (sector % 2.0 - 1.0).abs());
    let (r, g, b) = match sector as u32 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let base = lightness - chroma / 2.0;
    let to_byte = |v: f64| js_round((v + base).clamp(0.0, 1.0) * 255.0) as u8;
    Rgb([to_byte(r), to_byte(g), to_byte(b)])
}

/// A building's body color: its palette body (optionally nudged toward the
/// accent), blended toward the background by 1.5 times its layer's fade.
pub fn body_color(building: &Building, palette: &Palette) -> Rgb {
    let mut color = palette.bodies[usize::from(building.body_index)];
    if let Some(accent) = palette.accent {
        if building.accent_roll < palette.accent_chance {
            color = mix(color, accent, ACCENT_MIX);
        }
    }
    let fade = DEPTH_FADES[usize::from(building.layer.min(3))] * BODY_FADE_MULTIPLIER;
    mix(color, palette.bg, fade)
}

/// An Endless building's body color: [`body_color`], then blended 10%
/// toward its layer's tint target (Rebuild Prompt).
pub fn endless_body_color(building: &Building, palette: &Palette) -> Rgb {
    let target = palette.layer_tint_target(usize::from(building.layer.min(3)));
    mix(
        body_color(building, palette),
        target,
        ENDLESS_LAYER_TINT_BLEND,
    )
}

/// How [`CityRaster::render_options`] draws a city.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RasterOptions {
    /// Which window state to paint.
    pub lighting: WindowLighting,
    /// World x of buffer column 0.
    pub origin_x: i32,
    /// Buffer width in columns; `None` for the grid's columns.
    pub width: Option<usize>,
    /// Use Endless body colors ([`endless_body_color`]).
    pub endless_tint: bool,
}

impl Default for RasterOptions {
    fn default() -> Self {
        Self {
            lighting: WindowLighting::Current,
            origin_x: 0,
            width: None,
            endless_tint: false,
        }
    }
}

/// Landmark light color: the palette's fixed decor color, or the
/// decoration's hue at the palette's decor lightness.
pub fn decoration_color(hue: f64, palette: &Palette) -> Rgb {
    palette.decor.unwrap_or_else(|| {
        hsl(
            hue,
            DECOR_SATURATION,
            f64::from(palette.decor_lightness) / 100.0,
        )
    })
}

/// The logical-resolution city image plus the ownership arrays the reveal,
/// flicker, and material features will use.
#[derive(Debug, Clone, PartialEq)]
pub struct CityRaster {
    /// Columns.
    pub width: usize,
    /// Rows.
    pub height: usize,
    /// World x of column 0 (0 for a screen-sized Normal raster).
    pub origin_x: i32,
    /// RGBA bytes, row-major, top row first, alpha always 255.
    pub rgba: Vec<u8>,
    /// Building index per cell, or [`OWNER_SKY`].
    pub owner: Vec<i32>,
    /// Window index per cell, [`WINDOW_BODY`], or [`WINDOW_MAST`].
    pub window_index: Vec<i32>,
    /// Layer of the owning building per cell (0 for sky).
    pub depth: Vec<u8>,
}

impl CityRaster {
    /// Buffer index for a world column and row, or `None` outside the
    /// buffer. Negative coordinates never wrap.
    pub fn index(&self, x: i32, row: i32) -> Option<usize> {
        let x = x - self.origin_x;
        let inside =
            x >= 0 && row >= 0 && (x as usize) < self.width && (row as usize) < self.height;
        inside.then(|| row as usize * self.width + x as usize)
    }

    /// The color at a cell, if it is inside the buffer.
    pub fn color_at(&self, x: i32, row: i32) -> Option<Rgb> {
        self.index(x, row)
            .map(|i| Rgb([self.rgba[i * 4], self.rgba[i * 4 + 1], self.rgba[i * 4 + 2]]))
    }

    /// Rasterizes `buildings` and `stars` on `grid` in `palette`, painting
    /// windows by their current `lit` flags.
    /// O(grid cells + building area + windows).
    pub fn render(grid: &Grid, buildings: &[Building], stars: &[Star], palette: &Palette) -> Self {
        Self::render_with(grid, buildings, stars, palette, WindowLighting::Current)
    }

    /// Like [`CityRaster::render`], choosing which window state to paint.
    pub fn render_with(
        grid: &Grid,
        buildings: &[Building],
        stars: &[Star],
        palette: &Palette,
        lighting: WindowLighting,
    ) -> Self {
        let options = RasterOptions {
            lighting,
            ..RasterOptions::default()
        };
        Self::render_options(grid, buildings, stars, palette, options)
    }

    /// Like [`CityRaster::render`], with full control over the window state,
    /// the buffer's world origin and width, and Endless body colors.
    /// O(buffer cells + building area + windows).
    pub fn render_options(
        grid: &Grid,
        buildings: &[Building],
        stars: &[Star],
        palette: &Palette,
        options: RasterOptions,
    ) -> Self {
        let width = options.width.unwrap_or(grid.cols.max(1) as usize).max(1);
        let height = grid.rows.max(1) as usize;
        let cells = width * height;
        let mut raster = Self {
            width,
            height,
            origin_x: options.origin_x,
            rgba: Vec::with_capacity(cells * 4),
            owner: vec![OWNER_SKY; cells],
            window_index: vec![WINDOW_BODY; cells],
            depth: vec![0; cells],
        };
        for _ in 0..cells {
            raster
                .rgba
                .extend([palette.bg.0[0], palette.bg.0[1], palette.bg.0[2], 255]);
        }
        for star in stars {
            raster.paint(star.x, star.row, mix(palette.star, palette.bg, star.fade));
        }
        raster.resolve_ownership(grid, buildings);
        raster.paint_buildings(grid, buildings, palette, options);
        raster
    }

    fn paint(&mut self, x: i32, row: i32, color: Rgb) {
        if let Some(i) = self.index(x, row) {
            self.rgba[i * 4..i * 4 + 3].copy_from_slice(&color.0);
        }
    }

    /// Assigns every cell to its nearest building before painting.
    fn resolve_ownership(&mut self, grid: &Grid, buildings: &[Building]) {
        let mut order: Vec<usize> = (0..buildings.len()).collect();
        order.sort_by_key(|&i| buildings[i].layer); // stable: far to near
        for &index in &order {
            let building = &buildings[index];
            let owner = index as i32;
            for tier in &building.tiers {
                for h in tier.h0..tier.h1 {
                    for x in tier.x0..tier.x1 {
                        self.claim(
                            x,
                            grid.row_for_height(h),
                            owner,
                            WINDOW_BODY,
                            building.layer,
                        );
                    }
                }
            }
            if let Some(mast) = building.mast {
                for h in mast.h0..mast.h1 {
                    self.claim(
                        mast.x,
                        grid.row_for_height(h),
                        owner,
                        WINDOW_MAST,
                        building.layer,
                    );
                }
            }
            for w in 0..building.windows.len() {
                let row = grid.row_for_height(building.windows.h[w]);
                if let Some(i) = self.index(building.windows.x[w], row) {
                    if self.owner[i] == owner && self.window_index[i] == WINDOW_BODY {
                        self.window_index[i] = w as i32;
                    }
                }
            }
        }
    }

    fn claim(&mut self, x: i32, row: i32, owner: i32, window: i32, layer: u8) {
        if let Some(i) = self.index(x, row) {
            self.owner[i] = owner;
            self.window_index[i] = window;
            self.depth[i] = layer;
        }
    }

    fn paint_buildings(
        &mut self,
        grid: &Grid,
        buildings: &[Building],
        palette: &Palette,
        options: RasterOptions,
    ) {
        let lighting = options.lighting;
        let tint = if options.endless_tint {
            endless_body_color
        } else {
            body_color
        };
        let bodies: Vec<Rgb> = buildings.iter().map(|b| tint(b, palette)).collect();
        let beacon_rows: Vec<Option<(i32, i32)>> = buildings
            .iter()
            .map(|b| b.mast.map(|m| (m.x, grid.row_for_height(m.h1 - 1))))
            .collect();
        for i in 0..self.owner.len() {
            let Ok(owner) = usize::try_from(self.owner[i]) else {
                continue;
            };
            let building = &buildings[owner];
            let body = bodies[owner];
            let color = match self.window_index[i] {
                WINDOW_MAST => {
                    let cell = (
                        (i % self.width) as i32 + self.origin_x,
                        (i / self.width) as i32,
                    );
                    if beacon_rows[owner] == Some(cell) {
                        palette.beacon
                    } else {
                        palette.antenna
                    }
                }
                w if w >= 0 => {
                    let w = w as usize;
                    let on = match lighting {
                        WindowLighting::Current => building.windows.lit[w],
                        WindowLighting::Target => building.windows.target[w],
                    };
                    if on {
                        let lit = palette.window_color(building.windows.tone[w]);
                        mix(lit, body, shade_from_level(building.windows.shade[w]))
                    } else {
                        body
                    }
                }
                _ => body,
            };
            self.rgba[i * 4..i * 4 + 3].copy_from_slice(&color.0);
        }
        for (owner, building) in buildings.iter().enumerate() {
            let Some(decoration) = &building.decoration else {
                continue;
            };
            let color = decoration_color(decoration.hue, palette);
            for (x, h) in decoration.iter_points() {
                let row = grid.row_for_height(h);
                if self
                    .index(x, row)
                    .is_some_and(|i| self.owner[i] == owner as i32)
                {
                    self.paint(x, row, color);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::building::{BuildingKind, Mast, Tier, WindowSet};
    use crate::model::city::generate_normal_city;
    use crate::model::palette::palette_by_key;

    fn slab(layer: u8, x0: i32, x1: i32, h: i32, body: u8) -> Building {
        Building {
            layer,
            x0,
            x1,
            kind: BuildingKind::Box,
            tiers: vec![Tier::new(x0, x1, 0, h).unwrap()],
            body_index: body,
            tone: 0,
            accent_roll: 1.0,
            windows: WindowSet::default(),
            mast: None,
            decoration: None,
        }
    }

    fn small_grid() -> Grid {
        Grid::new(40, 40, false) // cell 2 -> 20 x 20
    }

    #[test]
    fn empty_city_is_background_and_buffer_matches_grid() {
        let classic = palette_by_key("classic").unwrap();
        let raster = CityRaster::render(&small_grid(), &[], &[], classic);
        assert_eq!((raster.width, raster.height), (20, 20));
        assert_eq!(raster.rgba.len(), 20 * 20 * 4);
        assert!(raster.rgba.chunks(4).all(|px| px == [0, 0, 0, 255]));
        assert!(raster.owner.iter().all(|&o| o == OWNER_SKY));
    }

    #[test]
    fn index_rejects_outside_and_negative_cells() {
        let raster =
            CityRaster::render(&small_grid(), &[], &[], palette_by_key("classic").unwrap());
        assert_eq!(raster.index(0, 0), Some(0));
        assert_eq!(raster.index(19, 19), Some(399));
        assert_eq!(raster.index(-1, 0), None);
        assert_eq!(raster.index(0, -1), None);
        assert_eq!(raster.index(20, 0), None);
    }

    #[test]
    fn nearer_layer_owns_overlapping_cells_and_hides_windows_behind() {
        let grid = small_grid();
        let mut far = slab(0, 2, 10, 10, 0);
        far.windows.push(6, 3, 0, 0, true);
        far.windows.lit[0] = true;
        let near = slab(3, 5, 12, 6, 1);
        let classic = palette_by_key("classic").unwrap();
        let raster = CityRaster::render(&grid, &[near.clone(), far.clone()], &[], classic);
        let row = grid.row_for_height(3);
        let i = raster.index(6, row).unwrap();
        assert_eq!(
            raster.owner[i], 0,
            "near building (index 0) owns the overlap"
        );
        assert_eq!(raster.window_index[i], WINDOW_BODY, "far window is hidden");
        assert_eq!(raster.color_at(6, row), Some(body_color(&near, classic)));
        assert_eq!(raster.depth[i], 3);
        let far_cell = raster.index(3, grid.row_for_height(8)).unwrap();
        assert_eq!(raster.owner[far_cell], 1);
    }

    #[test]
    fn bodies_fade_toward_background_by_depth() {
        let parchment = palette_by_key("blackonwhite").unwrap();
        let near = slab(3, 0, 4, 4, 2);
        let far = slab(0, 0, 4, 4, 2);
        assert_eq!(
            body_color(&near, parchment),
            parchment.bodies[2],
            "layer 3 has no fade"
        );
        assert_eq!(
            body_color(&far, parchment),
            mix(parchment.bodies[2], parchment.bg, 0.45)
        );
    }

    #[test]
    fn lit_windows_use_tone_color_faded_by_shade_and_unlit_match_body() {
        let grid = small_grid();
        let classic = palette_by_key("classic").unwrap();
        let mut b = slab(3, 0, 10, 10, 0);
        b.windows.push(2, 5, 0, 0, true); // shade 0: full window color
        b.windows.push(4, 5, 15, 0, true); // shade 15: fully body color
        b.windows.push(6, 5, 0, 0, false);
        b.windows.lit = vec![true, true, false];
        let raster = CityRaster::render(&grid, &[b.clone()], &[], classic);
        let row = grid.row_for_height(5);
        assert_eq!(raster.color_at(2, row), Some(classic.window_color(0)));
        assert_eq!(raster.color_at(4, row), Some(body_color(&b, classic)));
        assert_eq!(raster.color_at(6, row), Some(body_color(&b, classic)));
    }

    #[test]
    fn masts_draw_antenna_with_beacon_tip_and_decorations_draw_on_owned_cells() {
        let grid = small_grid();
        let classic = palette_by_key("classic").unwrap();
        let mut b = slab(2, 0, 9, 8, 0);
        b.mast = Some(Mast {
            x: 4,
            h0: 8,
            h1: 12,
            beacon_period: 2.0,
            beacon_phase: 0.0,
        });
        b.decoration = Some(crate::model::building::Decoration {
            kind: crate::model::building::DecorationKind::Roofline,
            mode: crate::model::building::LightMode::Cycle,
            hue: 120.0,
            points: vec![0, 7, 1, 7],
        });
        let raster = CityRaster::render(&grid, &[b], &[], classic);
        assert_eq!(
            raster.color_at(4, grid.row_for_height(9)),
            Some(classic.antenna)
        );
        assert_eq!(
            raster.color_at(4, grid.row_for_height(11)),
            Some(classic.beacon)
        );
        let decor = decoration_color(120.0, classic);
        assert_eq!(raster.color_at(0, grid.row_for_height(7)), Some(decor));
        let i = raster.index(4, grid.row_for_height(10)).unwrap();
        assert_eq!(raster.window_index[i], WINDOW_MAST);
    }

    #[test]
    fn target_lighting_paints_the_built_look_without_changing_lit_flags() {
        let grid = small_grid();
        let classic = palette_by_key("classic").unwrap();
        let mut b = slab(3, 0, 10, 10, 0);
        b.windows.push(2, 5, 0, 0, true);
        let row = grid.row_for_height(5);
        let current = CityRaster::render(&grid, &[b.clone()], &[], classic);
        let target =
            CityRaster::render_with(&grid, &[b.clone()], &[], classic, WindowLighting::Target);
        assert_eq!(
            current.color_at(2, row),
            Some(body_color(&b, classic)),
            "not lit yet"
        );
        assert_eq!(target.color_at(2, row), Some(classic.window_color(0)));
        assert!(!b.windows.lit[0]);
    }

    #[test]
    fn stars_follow_density_and_stay_in_the_sky() {
        let grid = Grid::new(1920, 1080, false);
        let stars = generate_stars(9, &grid);
        let expected = js_round(f64::from(grid.cols) * f64::from(grid.ground_row) * STAR_DENSITY);
        assert_eq!(stars.len(), expected as usize);
        assert!(stars.iter().all(|s| (0..grid.cols).contains(&s.x)
            && (0..grid.ground_row).contains(&s.row)
            && (0.0..STAR_MAX_FADE).contains(&s.fade)));
        assert_eq!(stars, generate_stars(9, &grid));
    }

    #[test]
    fn hsl_converts_primary_hues() {
        assert_eq!(hsl(0.0, 1.0, 0.5), Rgb([255, 0, 0]));
        assert_eq!(hsl(120.0, 1.0, 0.5), Rgb([0, 255, 0]));
        assert_eq!(hsl(240.0, 1.0, 0.5), Rgb([0, 0, 255]));
        assert_eq!(hsl(360.0, 1.0, 0.5), Rgb([255, 0, 0]));
        assert_eq!(hsl(0.0, 0.0, 1.0), Rgb([255, 255, 255]));
        let gameboy = palette_by_key("gameboy").unwrap();
        assert_eq!(
            decoration_color(10.0, gameboy),
            Rgb::hex(0x9bbc0f),
            "fixed decor wins"
        );
    }

    #[test]
    fn whole_city_renders_with_buildings_windows_and_stars() {
        let grid = Grid::new(1280, 720, false);
        let mut city = generate_normal_city(5, &grid);
        for building in &mut city {
            building.windows.light_to_target();
        }
        let stars = generate_stars(5, &grid);
        let classic = palette_by_key("classic").unwrap();
        let raster = CityRaster::render(&grid, &city, &stars, classic);
        let owned = raster.owner.iter().filter(|&&o| o >= 0).count();
        let windows = raster.window_index.iter().filter(|&&w| w >= 0).count();
        assert!(
            owned > raster.owner.len() / 5,
            "buildings cover a good share of the grid"
        );
        assert!(windows > 1_000, "visible windows: {windows}");
        assert_eq!(raster, CityRaster::render(&grid, &city, &stars, classic));
    }
}
