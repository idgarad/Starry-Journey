//! Normal-mode construction: the city appears one logical cell at a time
//! (Rebuild Prompt, "Time, Reveal, And Endless Motion").
//!
//! Every cell owned by a building (body, window, or mast) is put in a
//! seeded Fisher-Yates order. At progress `f`, the first `floor(f * total)`
//! cells are visible, and all of them once `f >= 1`. When a revealed cell is
//! a window whose target is on, that window turns on. Unrevealed cells show
//! the sky behind them.
//!
//! [`ConstructionView`] keeps the image on screen up to date incrementally:
//! each newly revealed cell copies one pixel from the finished city image,
//! so a frame costs O(newly revealed cells), not O(grid).

use crate::model::building::Building;
use crate::model::rng::Mulberry32;
use crate::raster::CityRaster;

/// Seconds an untimed construction takes at 1x speed.
pub const UNTIMED_SECONDS: f64 = 240.0;
/// Seed XOR for the reveal-order stream (assumption AS-R007: the spec
/// does not name this stream; it is kept separate from the city stream).
pub const REVEAL_SEED_XOR: u32 = 0x5bd1_e995;
/// Largest frame time used to advance progress, in seconds.
pub const MAX_FRAME_DT: f64 = 0.25;

/// Reveal order and progress for one city.
#[derive(Debug, Clone, PartialEq)]
pub struct Reveal {
    order: Vec<usize>,
    revealed: Vec<bool>,
    shown: usize,
    progress: f64,
}

impl Reveal {
    /// Shuffles every owned cell of `raster` with the reveal stream for
    /// `seed`. O(grid cells).
    pub fn new(raster: &CityRaster, seed: u32) -> Self {
        let mut order: Vec<usize> = (0..raster.owner.len())
            .filter(|&cell| raster.owner[cell] >= 0)
            .collect();
        let mut rng = Mulberry32::new(seed ^ REVEAL_SEED_XOR);
        for i in (1..order.len()).rev() {
            let j = rng.rand_int(0, i as i64) as usize;
            order.swap(i, j);
        }
        Self {
            order,
            revealed: vec![false; raster.owner.len()],
            shown: 0,
            progress: 0.0,
        }
    }

    /// Number of cells that construction reveals.
    pub fn total(&self) -> usize {
        self.order.len()
    }

    /// Number of cells revealed so far.
    pub fn shown(&self) -> usize {
        self.shown
    }

    /// Progress in `0..=1`.
    pub fn progress(&self) -> f64 {
        self.progress
    }

    /// True once every cell is revealed.
    pub fn is_complete(&self) -> bool {
        self.shown == self.order.len()
    }

    /// True if the buffer cell is revealed. Sky cells are never "revealed".
    pub fn is_revealed(&self, cell: usize) -> bool {
        self.revealed.get(cell).copied().unwrap_or(false)
    }

    /// Moves progress to `progress` (clamped to `0..=1`; it never goes
    /// backwards) and returns the cells revealed by this step, in order.
    /// O(newly revealed cells).
    pub fn advance_to(&mut self, progress: f64) -> &[usize] {
        self.progress = progress.clamp(0.0, 1.0).max(self.progress);
        let target = if self.progress >= 1.0 {
            self.order.len()
        } else {
            ((self.progress * self.order.len() as f64).floor() as usize).min(self.order.len())
        };
        let start = self.shown;
        for &cell in &self.order[start..target.max(start)] {
            self.revealed[cell] = true;
        }
        self.shown = target.max(start);
        &self.order[start..self.shown]
    }
}

/// Untimed construction progress after `dt` seconds at `speed`:
/// `progress + dt * speed / 240`, with `dt` clamped to `0..=0.25`.
pub fn untimed_step(progress: f64, dt: f64, speed: f64) -> f64 {
    progress + dt.clamp(0.0, MAX_FRAME_DT) * speed / UNTIMED_SECONDS
}

/// Turns on target-lit windows in newly revealed cells. O(cells).
pub fn light_revealed(buildings: &mut [Building], raster: &CityRaster, cells: &[usize]) {
    for &cell in cells {
        let (Ok(owner), Ok(window)) = (
            usize::try_from(raster.owner[cell]),
            usize::try_from(raster.window_index[cell]),
        ) else {
            continue;
        };
        let windows = &mut buildings[owner].windows;
        if windows.target[window] {
            windows.lit[window] = true;
        }
    }
}

/// The image on screen during construction: revealed cells come from the
/// finished city image, unrevealed ones from the sky image.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstructionView {
    /// RGBA bytes to present, same layout as [`CityRaster::rgba`].
    pub rgba: Vec<u8>,
    built: Vec<u8>,
    sky: Vec<u8>,
    dirty: bool,
}

impl ConstructionView {
    /// Builds the view from the finished city image, the sky-only image, and
    /// the reveal state so far. O(grid cells).
    ///
    /// # Panics
    ///
    /// Panics if the two images differ in size.
    pub fn new(built: Vec<u8>, sky: Vec<u8>, reveal: &Reveal) -> Self {
        assert_eq!(built.len(), sky.len(), "images must match in size");
        let mut rgba = sky.clone();
        for cell in 0..rgba.len() / 4 {
            if reveal.is_revealed(cell) {
                rgba[cell * 4..cell * 4 + 4].copy_from_slice(&built[cell * 4..cell * 4 + 4]);
            }
        }
        Self {
            rgba,
            built,
            sky,
            dirty: true,
        }
    }

    /// Shows newly revealed cells. O(cells).
    pub fn reveal_cells(&mut self, cells: &[usize]) {
        for &cell in cells {
            self.rgba[cell * 4..cell * 4 + 4].copy_from_slice(&self.built[cell * 4..cell * 4 + 4]);
        }
        self.dirty |= !cells.is_empty();
    }

    /// The sky-only image (useful when rebuilding after a palette change).
    pub fn sky(&self) -> &[u8] {
        &self.sky
    }

    /// Returns true once if the image changed since the last call.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::city::generate_normal_city;
    use crate::model::grid::Grid;
    use crate::model::palette::palette_by_key;
    use crate::raster::{generate_stars, WindowLighting};

    struct Fixture {
        buildings: Vec<Building>,
        raster: CityRaster,
        built: CityRaster,
        sky: CityRaster,
    }

    fn fixture(seed: u32) -> Fixture {
        let grid = Grid::new(640, 360, false);
        let buildings = generate_normal_city(seed, &grid);
        let stars = generate_stars(seed, &grid);
        let classic = palette_by_key("classic").unwrap();
        let raster = CityRaster::render(&grid, &buildings, &stars, classic);
        let built =
            CityRaster::render_with(&grid, &buildings, &stars, classic, WindowLighting::Target);
        let sky = CityRaster::render(&grid, &[], &stars, classic);
        Fixture {
            buildings,
            raster,
            built,
            sky,
        }
    }

    #[test]
    fn order_covers_every_owned_cell_exactly_once() {
        let f = fixture(1);
        let reveal = Reveal::new(&f.raster, 1);
        let owned = f.raster.owner.iter().filter(|&&o| o >= 0).count();
        assert_eq!(reveal.total(), owned);
        let mut sorted = reveal.order.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), owned, "no duplicates");
        assert!(
            sorted.iter().all(|&cell| f.raster.owner[cell] >= 0),
            "no sky cells"
        );
    }

    #[test]
    fn order_is_seeded_and_shuffled() {
        let f = fixture(2);
        assert_eq!(Reveal::new(&f.raster, 2), Reveal::new(&f.raster, 2));
        assert_ne!(
            Reveal::new(&f.raster, 2).order,
            Reveal::new(&f.raster, 3).order
        );
        let reveal = Reveal::new(&f.raster, 2);
        let mut ascending = reveal.order.clone();
        ascending.sort_unstable();
        assert_ne!(reveal.order, ascending, "order is not just left-to-right");
    }

    #[test]
    fn progress_reveals_floor_of_fraction_and_everything_at_one() {
        let f = fixture(3);
        let mut reveal = Reveal::new(&f.raster, 3);
        let total = reveal.total();
        assert_eq!(reveal.advance_to(0.0).len(), 0);
        let first = reveal.advance_to(0.25).len();
        assert_eq!(first, (0.25 * total as f64).floor() as usize);
        assert_eq!(reveal.shown(), first);
        assert!(!reveal.is_complete());
        reveal.advance_to(0.999_999_999);
        assert!(reveal.shown() < total || total == 0);
        reveal.advance_to(1.0);
        assert!(reveal.is_complete());
        assert_eq!(reveal.shown(), total);
        assert!(reveal.advance_to(2.0).is_empty(), "nothing left to reveal");
    }

    #[test]
    fn progress_never_goes_backwards() {
        let f = fixture(4);
        let mut reveal = Reveal::new(&f.raster, 4);
        reveal.advance_to(0.5);
        let shown = reveal.shown();
        assert!(reveal.advance_to(0.2).is_empty());
        assert_eq!((reveal.shown(), reveal.progress()), (shown, 0.5));
        assert_eq!(reveal.advance_to(-1.0).len(), 0);
    }

    #[test]
    fn untimed_construction_takes_240_seconds_at_1x_and_60_at_4x() {
        let mut progress = 0.0;
        for _ in 0..(240 * 60) {
            progress = untimed_step(progress, 1.0 / 60.0, 1.0);
        }
        assert!((progress - 1.0).abs() < 1e-9);
        let mut fast = 0.0;
        for _ in 0..(60 * 60) {
            fast = untimed_step(fast, 1.0 / 60.0, 4.0);
        }
        assert!((fast - 1.0).abs() < 1e-9);
        assert_eq!(
            untimed_step(0.0, 10.0, 1.0),
            0.25 / 240.0,
            "long frames are clamped"
        );
        assert_eq!(
            untimed_step(0.5, -1.0, 1.0),
            0.5,
            "negative dt does nothing"
        );
    }

    #[test]
    fn revealing_lights_only_target_windows_in_revealed_cells() {
        let mut f = fixture(5);
        let mut reveal = Reveal::new(&f.raster, 5);
        let cells = reveal.advance_to(0.5).to_vec();
        light_revealed(&mut f.buildings, &f.raster, &cells);
        for cell in 0..f.raster.owner.len() {
            let (Ok(owner), Ok(window)) = (
                usize::try_from(f.raster.owner[cell]),
                usize::try_from(f.raster.window_index[cell]),
            ) else {
                continue;
            };
            let windows = &f.buildings[owner].windows;
            let expected = reveal.is_revealed(cell) && windows.target[window];
            assert_eq!(windows.lit[window], expected, "cell {cell}");
        }
        let cells = reveal.advance_to(1.0).to_vec();
        light_revealed(&mut f.buildings, &f.raster, &cells);
        let lit: usize = f.buildings.iter().map(|b| b.windows.lit_count()).sum();
        let visible_targets = (0..f.raster.owner.len())
            .filter(|&cell| {
                let (Ok(o), Ok(w)) = (
                    usize::try_from(f.raster.owner[cell]),
                    usize::try_from(f.raster.window_index[cell]),
                ) else {
                    return false;
                };
                f.buildings[o].windows.target[w]
            })
            .count();
        assert_eq!(
            lit, visible_targets,
            "every visible target window is lit at the end"
        );
    }

    #[test]
    fn view_starts_as_sky_and_ends_as_the_built_city() {
        let f = fixture(6);
        let mut reveal = Reveal::new(&f.raster, 6);
        let mut view = ConstructionView::new(f.built.rgba.clone(), f.sky.rgba.clone(), &reveal);
        assert_eq!(view.rgba, f.sky.rgba);
        assert!(view.take_dirty());
        assert!(!view.take_dirty(), "dirty is reported once");
        let cells = reveal.advance_to(0.3).to_vec();
        view.reveal_cells(&cells);
        assert!(view.take_dirty());
        let cell = cells[0];
        assert_eq!(
            view.rgba[cell * 4..cell * 4 + 4],
            f.built.rgba[cell * 4..cell * 4 + 4]
        );
        let cells = reveal.advance_to(1.0).to_vec();
        view.reveal_cells(&cells);
        assert_eq!(view.rgba, f.built.rgba);
    }

    #[test]
    fn view_rebuilt_mid_construction_matches_incremental_view() {
        let f = fixture(7);
        let mut reveal = Reveal::new(&f.raster, 7);
        let mut incremental =
            ConstructionView::new(f.built.rgba.clone(), f.sky.rgba.clone(), &reveal);
        let cells = reveal.advance_to(0.6).to_vec();
        incremental.reveal_cells(&cells);
        let rebuilt = ConstructionView::new(f.built.rgba.clone(), f.sky.rgba.clone(), &reveal);
        assert_eq!(incremental.rgba, rebuilt.rgba);
    }
}
