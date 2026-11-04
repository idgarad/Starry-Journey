//! Clouds (Rebuild Prompt, "Clouds"; Starry Journey extensions, owner
//! decisions 2026-11-03: variety and density for later rain and storms,
//! clouds in both modes, dynamic lighting).
//!
//! **Shapes.** Every cloud is a union of stepped domes on a shared flat
//! base, built from two-cell-high strips as the spec describes. Three kinds:
//! small rounded *puffs*, the spec's long stepped *banks* (38-90 x 6-13),
//! and broad thick *decks* for overcast and storms.
//!
//! **Coverage.** A [`Cover`] preset (clear, puffs, scattered, broken,
//! overcast, storm) sets how many clouds there are, which kinds, how big,
//! and how dark. Scattered follows the spec: `max(3, ceil(cols / 130))`
//! clouds, mostly banks.
//!
//! **Motion.** Clouds live in their own world, seen through a view that
//! drifts with the wind at 0.35 cells per second times the speed setting
//! and, in Endless mode, follows the root camera at a tenth of its speed
//! (far behind the farthest city layer). A cloud that drifts out past the
//! left of the `cols + 160` wrap span comes back on the right with a fresh
//! shape. Seeded with `seed XOR 0xc2b2ae35`.
//!
//! **Light.** Cells store only shape (how far each is from the cloud's
//! edge); color comes from the light sources each time the strip is drawn,
//! snapped to four tones between the palette's sky and star colors. This
//! branch has ambient night light and moonlight: brightness by phase, a
//! lit rim on the edges facing the moon, and a glow on clouds near it.
//! Thin edges are translucent, so the moon and stars show softly through
//! them. City glow, searchlights, the sun, and lightning join the same
//! model later.

use crate::model::grid::Grid;
use crate::model::palette::{Palette, Rgb};
use crate::model::rng::{cloud_seed, Mulberry32};
use crate::raster::mix;
use crate::street::Span;

/// Wind drift in cells per second at 1x (Rebuild Prompt).
pub const WIND_SPEED: f64 = 0.35;
/// Fraction of the Endless root camera's motion the clouds follow.
pub const CLOUD_PARALLAX: f64 = 0.1;
/// Cells beyond each screen edge in the wrap span (`cols + 160` in all).
pub const WRAP_MARGIN: i32 = 80;
/// Longest simulated motion step per frame, in seconds.
pub const MAX_MOTION_STEP: f64 = 1.0 / 30.0;
/// Number of light tones a cloud is drawn in.
pub const TONES: usize = 4;
/// Opacity of a cloud's outermost cells (thin edges).
pub const EDGE_ALPHA: f64 = 0.55;
/// Extra columns on each side of the drawn strip, so the GPU can slide it.
pub const STRIP_MARGIN: i32 = 2;

/// Cloud coverage presets, in the order `C` cycles them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cover {
    /// No clouds.
    Clear,
    /// A few small fair-weather puffs.
    Puffs,
    /// The spec's default: a few long banks and some puffs.
    #[default]
    Scattered,
    /// More and bigger clouds, some decks.
    Broken,
    /// Mostly decks covering much of the sky.
    Overcast,
    /// Heavy, dark, low decks (lightning arrives in a later feature).
    Storm,
}

/// Every preset, in cycling order.
pub const COVERS: [Cover; 6] = [
    Cover::Clear,
    Cover::Puffs,
    Cover::Scattered,
    Cover::Broken,
    Cover::Overcast,
    Cover::Storm,
];

/// The three cloud shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudKind {
    Puff,
    Bank,
    Deck,
}

/// What a preset asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CoverSpec {
    /// Columns per cloud (`f64::INFINITY` for none).
    cols_per_cloud: f64,
    /// Fewest clouds when there are any.
    min_clouds: usize,
    /// Relative weights of puffs, banks, and decks.
    weights: [f64; 3],
    /// Size multiplier for banks and decks.
    size: f64,
    /// Ambient light on the clouds (0 black, 1 full star color).
    ambient: f64,
    /// Tone range: fractions from the sky color toward the star color.
    tone_range: (f64, f64),
    /// Altitude band for cloud tops, as fractions of the ground row.
    band: (f64, f64),
    /// Deck heights as fractions of the sky's rows, so heavy skies stay
    /// heavy on any screen.
    deck_height: (f64, f64),
}

impl Cover {
    fn spec(self) -> CoverSpec {
        match self {
            Cover::Clear => CoverSpec {
                cols_per_cloud: f64::INFINITY,
                min_clouds: 0,
                weights: [1.0, 0.0, 0.0],
                size: 1.0,
                ambient: 0.3,
                tone_range: (0.08, 0.45),
                band: (0.04, 0.45),
                deck_height: (0.06, 0.08),
            },
            Cover::Puffs => CoverSpec {
                cols_per_cloud: 90.0,
                min_clouds: 3,
                weights: [1.0, 0.0, 0.0],
                size: 1.0,
                ambient: 0.3,
                tone_range: (0.08, 0.45),
                band: (0.06, 0.45),
                deck_height: (0.06, 0.08),
            },
            Cover::Scattered => CoverSpec {
                cols_per_cloud: 130.0,
                min_clouds: 3,
                weights: [0.3, 0.7, 0.0],
                size: 1.0,
                ambient: 0.28,
                tone_range: (0.08, 0.42),
                band: (0.04, 0.45),
                deck_height: (0.06, 0.08),
            },
            Cover::Broken => CoverSpec {
                cols_per_cloud: 50.0,
                min_clouds: 6,
                weights: [0.15, 0.5, 0.35],
                size: 1.4,
                ambient: 0.25,
                tone_range: (0.07, 0.38),
                band: (0.03, 0.42),
                deck_height: (0.08, 0.14),
            },
            Cover::Overcast => CoverSpec {
                cols_per_cloud: 45.0,
                min_clouds: 8,
                weights: [0.05, 0.3, 0.65],
                size: 1.5,
                ambient: 0.24,
                tone_range: (0.07, 0.32),
                band: (0.0, 0.4),
                deck_height: (0.15, 0.25),
            },
            Cover::Storm => CoverSpec {
                cols_per_cloud: 28.0,
                min_clouds: 10,
                weights: [0.0, 0.15, 0.85],
                size: 1.8,
                ambient: 0.2,
                tone_range: (0.06, 0.26),
                band: (0.0, 0.36),
                deck_height: (0.25, 0.4),
            },
        }
    }

    /// The preset after this one (wrapping), for the `C` key.
    pub fn next(self) -> Self {
        let index = COVERS.iter().position(|&c| c == self).unwrap_or(0);
        COVERS[(index + 1) % COVERS.len()]
    }

    /// Display name, shown in the message box when cycling.
    pub fn label(self) -> &'static str {
        match self {
            Cover::Clear => "Clear sky",
            Cover::Puffs => "Fair-weather puffs",
            Cover::Scattered => "Scattered clouds",
            Cover::Broken => "Broken clouds",
            Cover::Overcast => "Overcast",
            Cover::Storm => "Storm clouds",
        }
    }

    /// Command-line key.
    pub fn key(self) -> &'static str {
        match self {
            Cover::Clear => "clear",
            Cover::Puffs => "puffs",
            Cover::Scattered => "scattered",
            Cover::Broken => "broken",
            Cover::Overcast => "overcast",
            Cover::Storm => "storm",
        }
    }

    /// Parses a command-line key (case-insensitive).
    ///
    /// # Errors
    ///
    /// Returns a message listing the keys when `value` is not one.
    pub fn parse(value: &str) -> Result<Self, String> {
        let wanted = value.trim().to_ascii_lowercase();
        COVERS
            .iter()
            .copied()
            .find(|c| c.key() == wanted)
            .ok_or_else(|| {
                let keys: Vec<&str> = COVERS.iter().map(|c| c.key()).collect();
                format!("must be one of {}, got '{value}'", keys.join(", "))
            })
    }

    /// How many clouds a sky `cols` wide gets.
    pub fn cloud_count(self, cols: i32) -> usize {
        let spec = self.spec();
        if spec.cols_per_cloud.is_infinite() {
            return 0;
        }
        let wanted = (f64::from(cols.max(1)) / spec.cols_per_cloud).ceil() as usize;
        wanted.max(spec.min_clouds)
    }
}

/// One cloud: a shape mask placed in the cloud world.
#[derive(Debug, Clone, PartialEq)]
pub struct Cloud {
    pub kind: CloudKind,
    /// Cloud-world column of the mask's left edge.
    pub x: i32,
    /// Grid row of the mask's top edge.
    pub top: i32,
    pub width: i32,
    pub height: i32,
    /// Per cell, row-major: 0 outside, else the distance to the nearest
    /// outside cell (1 on the edge, capped at 3).
    pub depth: Vec<u8>,
}

impl Cloud {
    /// Whether the mask covers local cell `(cx, cy)`.
    pub fn covers(&self, cx: i32, cy: i32) -> bool {
        self.depth_at(cx, cy) > 0
    }

    /// The edge depth of local cell `(cx, cy)`, 0 outside the mask.
    pub fn depth_at(&self, cx: i32, cy: i32) -> u8 {
        if cx < 0 || cy < 0 || cx >= self.width || cy >= self.height {
            return 0;
        }
        self.depth[(cy * self.width + cx) as usize]
    }

    /// Covered cells.
    pub fn area(&self) -> usize {
        self.depth.iter().filter(|&&d| d > 0).count()
    }
}

/// Tallest a dome may be, in rows per cell of half-width: domes stay wide
/// and rounded instead of rising into arches.
pub const DOME_ASPECT: f64 = 0.55;

/// One stepped dome: centered at `cx`, standing on strip `base` (counted
/// from the mask's bottom), `half` cells half-wide at its base and
/// `strips` two-row strips tall.
#[derive(Debug, Clone, Copy)]
struct Dome {
    cx: f64,
    half: f64,
    base: i32,
    strips: i32,
}

impl Dome {
    /// A dome no taller than [`DOME_ASPECT`] allows and no more than
    /// `room` strips.
    fn flat(cx: f64, half: f64, base: i32, room: i32) -> Self {
        let rows = half * DOME_ASPECT;
        let strips = ((rows / 2.0).round() as i32).clamp(1, room.max(1));
        Self {
            cx,
            half,
            base,
            strips,
        }
    }
}

/// Builds a cloud mask: a slab (one `(left, right)` span per strip, from
/// the bottom) plus domes. Every strip is two rows, so outlines step in
/// whole cells. O(width × height).
fn build_mask(width: i32, height: i32, slab: &[(i32, i32)], domes: &[Dome]) -> Vec<bool> {
    let mut mask = vec![false; (width * height) as usize];
    let mut fill = |strip: i32, left: i32, right: i32| {
        for row in [height - 1 - 2 * strip, height - 2 - 2 * strip] {
            if row < 0 {
                continue;
            }
            for x in left.max(0)..right.min(width) {
                mask[(row * width + x) as usize] = true;
            }
        }
    };
    for (strip, &(left, right)) in slab.iter().enumerate() {
        fill(strip as i32, left, right);
    }
    for dome in domes {
        for strip in 0..dome.strips {
            let t = (f64::from(strip) + 0.5) / f64::from(dome.strips);
            let half = (dome.half * (1.0 - t * t).sqrt()).round();
            let left = (dome.cx - half).round() as i32;
            let right = (dome.cx + half).round() as i32;
            fill(dome.base + strip, left, right);
        }
    }
    mask
}

/// Drops empty rows above the shape, in whole strips. Returns the new
/// height. O(cells).
fn trim_top(mask: &mut Vec<bool>, width: i32, height: i32) -> i32 {
    let w = width as usize;
    let first = (0..height as usize)
        .find(|&row| mask[row * w..(row + 1) * w].iter().any(|&c| c))
        .unwrap_or(height as usize);
    let drop = first - first % 2;
    mask.drain(..drop * w);
    height - drop as i32
}

/// Distance from each covered cell to the nearest uncovered one (4-way),
/// capped at 3; 0 for uncovered cells. O(cells).
fn edge_depths(mask: &[bool], width: i32, height: i32) -> Vec<u8> {
    let at = |x: i32, y: i32| {
        x >= 0 && y >= 0 && x < width && y < height && mask[(y * width + x) as usize]
    };
    let mut depth = vec![0u8; mask.len()];
    for y in 0..height {
        for x in 0..width {
            if !at(x, y) {
                continue;
            }
            let mut d = 1u8;
            while d < 3 {
                let r = i32::from(d);
                let open = (-r..=r).any(|k| {
                    [
                        (x + k, y - r),
                        (x + k, y + r),
                        (x - r, y + k),
                        (x + r, y + k),
                    ]
                    .iter()
                    .any(|&(px, py)| !at(px, py))
                });
                if open {
                    break;
                }
                d += 1;
            }
            depth[(y * width + x) as usize] = d;
        }
    }
    depth
}

/// Generates one cloud of `kind` with its left edge at `x`. `size` scales
/// banks and decks; `deck_rows` bounds deck heights. O(cloud cells).
///
/// - Puff: one to three flattened domes on a shared base.
/// - Bank: three to five domes on a shared base, the main one setting the
///   spec height.
/// - Deck: a tall slab whose sides wander as they rise, with a lumpy top
///   of six to ten small domes standing on it.
fn make_cloud(
    rng: &mut Mulberry32,
    kind: CloudKind,
    x: i32,
    size: f64,
    band: (i32, i32),
    deck_rows: (i32, i32),
) -> Cloud {
    let between = |rng: &mut Mulberry32, lo: i64, hi: i64| rng.rand_int(lo, hi) as i32;
    let unit = |rng: &mut Mulberry32| rng.next_f64();
    let (width, height) = match kind {
        CloudKind::Puff => {
            let w = between(rng, 10, 24);
            // Round, not towering: at most about half as tall as wide.
            let tallest = ((w * 45 / 100) / 2).clamp(2, 4);
            (w, between(rng, 2, i64::from(tallest)) * 2)
        }
        CloudKind::Bank => {
            let w = (f64::from(between(rng, 38, 90)) * size).round() as i32;
            // Rebuild Prompt: heights 6..13; strips are two rows.
            let h = between(rng, 6, 13);
            (w, h + h % 2)
        }
        CloudKind::Deck => {
            let w = (f64::from(between(rng, 80, 160)) * size).round() as i32;
            let lo = deck_rows.0.max(10);
            let h = between(rng, i64::from(lo), i64::from(deck_rows.1.max(lo)));
            (w, h + h % 2)
        }
    };
    let strips = height / 2;
    let fw = f64::from(width);
    let mut slab = Vec::new();
    let mut domes = Vec::new();
    match kind {
        CloudKind::Puff => {
            // The first dome spans the cloud and sets its height.
            domes.push(Dome {
                cx: fw / 2.0,
                half: fw / 2.0,
                base: 0,
                strips,
            });
            for _ in 0..between(rng, 0, 2) {
                let half = fw * (0.2 + 0.15 * unit(rng));
                let cx = half + unit(rng) * (fw - 2.0 * half).max(0.0);
                domes.push(Dome::flat(cx, half, 0, strips));
            }
        }
        CloudKind::Bank => {
            // No slab: the domes share a flat base, and a slab wider than
            // them reads as a thin ledge. The main dome is wide enough to give the bank the spec's
            // height (6..13), so it may be a little taller than DOME_ASPECT.
            let main_half = (f64::from(height) / DOME_ASPECT).min(fw / 2.0);
            let main_cx = main_half + unit(rng) * (fw - 2.0 * main_half).max(0.0);
            domes.push(Dome {
                cx: main_cx,
                half: main_half,
                base: 0,
                strips,
            });
            for _ in 0..between(rng, 2, 4) {
                let half = fw * (0.12 + 0.2 * unit(rng));
                let cx = half + unit(rng) * (fw - 2.0 * half).max(0.0);
                domes.push(Dome::flat(cx, half, 0, strips));
            }
        }
        CloudKind::Deck => {
            let body = ((f64::from(strips) * (0.55 + 0.2 * unit(rng))).round() as i32).max(1);
            let mut left = between(rng, 0, i64::from(width / 20));
            let mut right = width - between(rng, 0, i64::from(width / 20));
            for _ in 0..body {
                if left >= right {
                    break;
                }
                slab.push((left, right));
                left = (left + between(rng, -1, 3)).max(0);
                right = (right - between(rng, -1, 3)).min(width);
            }
            let top = slab.len() as i32;
            let (top_left, top_right) = slab.last().copied().unwrap_or((0, width));
            let span = f64::from((top_right - top_left).max(2));
            for _ in 0..between(rng, 6, 10) {
                let half = span * (0.06 + 0.12 * unit(rng));
                let cx = f64::from(top_left) + half + unit(rng) * (span - 2.0 * half).max(0.0);
                let base = (top - 1 - between(rng, 0, 1)).max(0);
                domes.push(Dome::flat(cx, half, base, strips - base));
            }
        }
    }
    let mut mask = build_mask(width, height, &slab, &domes);
    let height = trim_top(&mut mask, width, height);
    let depth = edge_depths(&mask, width, height);
    let (lo, hi) = band;
    let top = between(rng, i64::from(lo), i64::from(hi.max(lo)));
    Cloud {
        kind,
        x,
        top,
        width,
        height,
        depth,
    }
}

/// The moon as a light source, in screen cells.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoonLight {
    /// Center column (fractional).
    pub x: f64,
    /// Center row (fractional).
    pub row: f64,
    /// Disk radius in cells.
    pub radius: f64,
    /// 0 for a new moon, 1 for a full moon.
    pub brightness: f64,
}

impl MoonLight {
    /// Brightness for a lunar phase (0 new, 0.5 full, 1 new again).
    pub fn brightness_for_phase(phase: f64) -> f64 {
        1.0 - (phase.rem_euclid(1.0) - 0.5).abs() * 2.0
    }
}

/// Everything lighting the clouds this frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CloudLights {
    pub moon: Option<MoonLight>,
}

/// How far the moon's glow reaches, in moon radii.
pub const MOON_GLOW_REACH: f64 = 2.2;

/// How much light a cloud cell gets, before quantizing (0 dark, 1 bright).
///
/// `surface` is the cell's distance in rows below the top of its column
/// and above the bottom. The top two rows of each column catch sky light
/// and the bottom two are shaded, so every bump gets its own highlight and
/// undersides read darker (the two-tone look). The moon adds, scaled by its
/// brightness: a glow that fades with the square of the distance within
/// [`MOON_GLOW_REACH`] radii (dimmer deep inside thick cloud), and a rim on
/// edge cells whose neighbor toward the moon is open sky. O(1).
pub fn cell_light(
    cloud: &Cloud,
    cx: i32,
    cy: i32,
    surface: (i32, i32),
    screen: (f64, f64),
    ambient: f64,
    lights: &CloudLights,
) -> f64 {
    let mut light = ambient;
    if surface.0 < 2 {
        light += 0.12;
    }
    if surface.1 < 2 {
        light -= 0.08;
    }
    if let Some(moon) = lights.moon {
        let (dx, dy) = (moon.x - screen.0, moon.row - screen.1);
        let distance = (dx * dx + dy * dy).sqrt();
        let reach = MOON_GLOW_REACH * moon.radius;
        let fade = (1.0 - distance / reach).max(0.0);
        let thick = if cloud.depth_at(cx, cy) >= 3 {
            0.6
        } else {
            1.0
        };
        light += moon.brightness * 0.6 * fade * fade * thick;
        if cloud.depth_at(cx, cy) == 1 && distance > 0.0 {
            let step = (dx.signum() as i32, dy.signum() as i32);
            let toward = [(step.0, 0), (0, step.1), step];
            let facing = toward
                .iter()
                .any(|&(sx, sy)| (sx, sy) != (0, 0) && !cloud.covers(cx + sx, cy + sy));
            if facing {
                light += moon.brightness * 0.35;
            }
        }
    }
    light.clamp(0.0, 1.0)
}

/// The first and last covered row of each column of a cloud (0 and -1 for
/// an empty column). O(cells).
fn column_extents(cloud: &Cloud) -> (Vec<i32>, Vec<i32>) {
    (0..cloud.width)
        .map(|cx| {
            let first = (0..cloud.height)
                .find(|&cy| cloud.covers(cx, cy))
                .unwrap_or(0);
            let last = (0..cloud.height)
                .rev()
                .find(|&cy| cloud.covers(cx, cy))
                .unwrap_or(-1);
            (first, last)
        })
        .unzip()
}

/// The tone ramp for a preset and palette: `TONES` colors from dim to
/// bright, between the palette's sky and star colors.
pub fn tone_ramp(cover: Cover, palette: &Palette) -> [Rgb; TONES] {
    let (lo, hi) = cover.spec().tone_range;
    std::array::from_fn(|k| {
        let t = lo + (hi - lo) * k as f64 / (TONES - 1) as f64;
        mix(palette.bg, palette.star, t)
    })
}

/// Snaps a light level to a tone index.
pub fn tone_index(light: f64) -> usize {
    ((light.clamp(0.0, 1.0) * TONES as f64) as usize).min(TONES - 1)
}

/// All clouds in the sky, and the view onto them.
#[derive(Debug, Clone)]
pub struct CloudField {
    rng: Mulberry32,
    cover: Cover,
    cols: i32,
    /// Rows of sky clouds may use (the ground row).
    sky_rows: i32,
    /// Cloud-world column at the screen's left edge.
    view: f64,
    /// Wind travel so far, in cells.
    wind: f64,
    /// The clouds, in drawing order.
    pub clouds: Vec<Cloud>,
}

impl CloudField {
    /// A sky of `cover` clouds for `grid`, seeded from the scene seed.
    pub fn new(scene_seed: u32, grid: &Grid, cover: Cover) -> Self {
        let mut field = Self {
            rng: Mulberry32::new(cloud_seed(scene_seed)),
            cover,
            cols: grid.cols.max(1),
            sky_rows: grid.ground_row.max(1),
            view: 0.0,
            wind: 0.0,
            clouds: Vec::new(),
        };
        field.populate();
        field
    }

    /// The coverage preset.
    pub fn cover(&self) -> Cover {
        self.cover
    }

    /// Replaces every cloud with a fresh sky of `cover`.
    pub fn set_cover(&mut self, cover: Cover) {
        self.cover = cover;
        self.populate();
    }

    /// The wrap span: `cols + 160`.
    pub fn span_width(&self) -> i32 {
        self.cols + 2 * WRAP_MARGIN
    }

    /// Cloud-world column at the screen's left edge.
    pub fn view(&self) -> f64 {
        self.view
    }

    fn band(&self) -> (i32, i32) {
        let (lo, hi) = self.cover.spec().band;
        let rows = f64::from(self.sky_rows);
        ((rows * lo).round() as i32, (rows * hi).round() as i32)
    }

    fn pick_kind(&mut self) -> CloudKind {
        let weights = self.cover.spec().weights;
        let total: f64 = weights.iter().sum();
        let mut roll = self.rng.next_f64() * total;
        for (kind, weight) in [CloudKind::Puff, CloudKind::Bank, CloudKind::Deck]
            .into_iter()
            .zip(weights)
        {
            if roll < weight {
                return kind;
            }
            roll -= weight;
        }
        CloudKind::Bank
    }

    fn spawn(&mut self, x: i32) -> Cloud {
        let kind = self.pick_kind();
        let spec = self.cover.spec();
        let band = self.band();
        let rows = f64::from(self.sky_rows);
        let deck_rows = (
            (rows * spec.deck_height.0).round() as i32,
            (rows * spec.deck_height.1).round() as i32,
        );
        let mut cloud = make_cloud(&mut self.rng, kind, x, spec.size, band, deck_rows);
        // Keep the whole cloud in the sky above the ground row.
        cloud.top = cloud.top.min(self.sky_rows - cloud.height).max(0);
        cloud
    }

    /// Spreads the preset's clouds evenly over the wrap span, each at a
    /// random offset within its share. O(clouds × cloud cells).
    fn populate(&mut self) {
        let count = self.cover.cloud_count(self.cols);
        let span = f64::from(self.span_width());
        let left = self.view.floor() as i32 - WRAP_MARGIN;
        self.clouds = (0..count)
            .map(|i| {
                let share = span / count as f64;
                let x = left + (share * (i as f64 + self.rng.next_f64())) as i32;
                let mut cloud = self.spawn(x);
                // Center the cloud on its slot rather than starting there.
                cloud.x -= cloud.width / 2;
                cloud
            })
            .collect();
    }

    /// Points the view at the Endless root camera (0 in Normal mode).
    pub fn set_camera(&mut self, camera: f64) {
        if camera.is_finite() {
            self.view = self.wind + CLOUD_PARALLAX * camera;
        }
    }

    /// Drifts with the wind for one frame (at most [`MAX_MOTION_STEP`]),
    /// then wraps clouds that left the span on the left back to the right
    /// with new shapes. `camera` is the root camera. Returns true if the
    /// view moved to a new whole column or a cloud was replaced.
    pub fn advance(&mut self, dt: f64, speed: f64, camera: f64) -> bool {
        let step = if dt.is_finite() {
            dt.clamp(0.0, MAX_MOTION_STEP)
        } else {
            0.0
        };
        let before = self.view.floor();
        self.wind += WIND_SPEED * speed.max(0.0) * step;
        self.set_camera(camera);
        let left = self.view.floor() as i32 - WRAP_MARGIN;
        let span = self.span_width();
        let mut replaced = false;
        for index in 0..self.clouds.len() {
            let cloud = &self.clouds[index];
            if cloud.x + cloud.width < left {
                let x = cloud.x + span;
                self.clouds[index] = self.spawn(x);
                replaced = true;
            }
        }
        replaced || self.view.floor() != before
    }

    /// The strip of the cloud world to draw: two columns either side of
    /// the screen, so the GPU can slide it by the view's fraction.
    pub fn strip(&self) -> Span {
        Span {
            origin_x: self.view.floor() as i32 - STRIP_MARGIN,
            width: self.cols + 2 * STRIP_MARGIN,
            rows: self.sky_rows,
        }
    }

    /// Device pixels from the strip's left edge to the screen's left edge.
    pub fn strip_offset_px(&self, cell: u32) -> f64 {
        (self.view - f64::from(self.strip().origin_x)) * f64::from(cell)
    }

    /// Draws the clouds into an RGBA strip (see [`CloudField::strip`]),
    /// lit by `lights`: four tones per palette and preset, edges
    /// translucent, transparent sky. Later clouds draw over earlier ones.
    /// O(strip cells + cloud cells).
    pub fn draw(&self, palette: &Palette, lights: &CloudLights) -> Vec<u8> {
        let span = self.strip();
        let mut rgba = vec![0u8; span.cells() * 4];
        let ramp = tone_ramp(self.cover, palette);
        let ambient = self.cover.spec().ambient;
        let view = self.view;
        for cloud in &self.clouds {
            let (tops, bottoms) = column_extents(cloud);
            for cy in 0..cloud.height {
                for cx in 0..cloud.width {
                    let depth = cloud.depth_at(cx, cy);
                    if depth == 0 {
                        continue;
                    }
                    let (wx, row) = (cloud.x + cx, cloud.top + cy);
                    let Some(cell) = span.index(wx, row) else {
                        continue;
                    };
                    let screen = (f64::from(wx) - view + 0.5, f64::from(row) + 0.5);
                    let column = cx as usize;
                    let surface = (cy - tops[column], bottoms[column] - cy);
                    let light = cell_light(cloud, cx, cy, surface, screen, ambient, lights);
                    let tone = ramp[tone_index(light)];
                    let pixel = &mut rgba[cell * 4..cell * 4 + 4];
                    // Edges are soft only against open sky; over another
                    // cloud they are solid, so overlaps show no seams.
                    let below = f64::from(pixel[3]) / 255.0;
                    let alpha = if depth == 1 && below == 0.0 {
                        EDGE_ALPHA
                    } else {
                        1.0
                    };
                    let total = alpha + below * (1.0 - alpha);
                    for (channel, value) in pixel[..3].iter_mut().zip(tone.0) {
                        let mixed = (f64::from(value) * alpha
                            + f64::from(*channel) * below * (1.0 - alpha))
                            / total.max(1e-9);
                        *channel = mixed.round() as u8;
                    }
                    pixel[3] = (total * 255.0).round() as u8;
                }
            }
        }
        rgba
    }

    /// Fraction of the visible sky (screen columns, rows above the ground)
    /// that clouds cover. For tuning and tests. O(screen sky cells).
    pub fn coverage(&self) -> f64 {
        let left = self.view.floor() as i32;
        let mut covered = 0usize;
        for row in 0..self.sky_rows {
            for x in left..left + self.cols {
                if self.clouds.iter().any(|c| c.covers(x - c.x, row - c.top)) {
                    covered += 1;
                }
            }
        }
        covered as f64 / (self.cols * self.sky_rows).max(1) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::grid::CellScaling;
    use crate::model::palette::{palette_by_key, palettes};

    fn grid() -> Grid {
        Grid::with_scaling(1920, 1080, true, CellScaling::Fixed(3))
    }

    fn classic() -> &'static Palette {
        palette_by_key("classic").unwrap()
    }

    #[test]
    fn presets_cycle_parse_and_scattered_follows_the_spec_count() {
        let mut cover = Cover::Clear;
        for expected in COVERS.iter().skip(1).chain(&[Cover::Clear]) {
            cover = cover.next();
            assert_eq!(cover, *expected);
        }
        for cover in COVERS {
            assert_eq!(Cover::parse(cover.key()), Ok(cover));
            assert!(!cover.label().is_empty());
        }
        assert_eq!(Cover::parse(" Storm "), Ok(Cover::Storm));
        assert!(Cover::parse("drizzle").unwrap_err().contains("overcast"));
        assert_eq!(Cover::default(), Cover::Scattered);
        // max(3, ceil(cols / 130)).
        assert_eq!(Cover::Scattered.cloud_count(640), 5);
        assert_eq!(Cover::Scattered.cloud_count(200), 3);
        assert_eq!(Cover::Clear.cloud_count(640), 0);
    }

    #[test]
    fn shapes_are_stepped_two_row_strips_within_their_size_ranges() {
        let mut rng = Mulberry32::new(4);
        for _ in 0..200 {
            for kind in [CloudKind::Puff, CloudKind::Bank, CloudKind::Deck] {
                let cloud = make_cloud(&mut rng, kind, 0, 1.0, (5, 40), (10, 16));
                let (w, h) = (cloud.width, cloud.height);
                match kind {
                    CloudKind::Puff => assert!((10..=24).contains(&w) && (4..=8).contains(&h)),
                    CloudKind::Bank => assert!((38..=90).contains(&w) && (6..=14).contains(&h)),
                    // A deck body is at least 55% of the requested 10..16 rows.
                    CloudKind::Deck => assert!((80..=160).contains(&w) && (6..=16).contains(&h)),
                }
                assert_eq!(h % 2, 0, "whole two-row strips");
                // Rows come in identical pairs: the stepped look.
                for strip in 0..h / 2 {
                    for x in 0..w {
                        assert_eq!(cloud.covers(x, 2 * strip), cloud.covers(x, 2 * strip + 1));
                    }
                }
                assert!(cloud.area() > 0);
                assert!(
                    (0..w).any(|x| cloud.covers(x, 0)),
                    "{kind:?} reaches its top"
                );
                assert!((0..w).any(|x| cloud.covers(x, h - 1)), "flat base");
            }
        }
    }

    #[test]
    fn puffs_stay_rounder_than_tall() {
        let mut rng = Mulberry32::new(11);
        for _ in 0..200 {
            let cloud = make_cloud(&mut rng, CloudKind::Puff, 0, 1.0, (5, 40), (10, 16));
            assert!(
                cloud.height <= (cloud.width * 45 / 100).max(4),
                "{} x {}",
                cloud.width,
                cloud.height
            );
        }
    }

    #[test]
    fn banks_have_no_thin_ledge_along_the_base() {
        let mut rng = Mulberry32::new(12);
        for _ in 0..200 {
            let cloud = make_cloud(&mut rng, CloudKind::Bank, 0, 1.0, (5, 40), (10, 16));
            // The bottom strip sticks out at most a third of the width
            // beyond the strip above it.
            let row_width =
                |row: i32| (0..cloud.width).filter(|&x| cloud.covers(x, row)).count() as i32;
            let (bottom, above) = (row_width(cloud.height - 1), row_width(cloud.height - 3));
            assert!(
                bottom - above <= cloud.width / 3,
                "{bottom} vs {above} of {}",
                cloud.width
            );
        }
    }

    #[test]
    fn edge_depth_is_one_on_the_outline_and_grows_inward() {
        let mask = vec![true; 7 * 7];
        let depth = edge_depths(&mask, 7, 7);
        assert_eq!(depth[0], 1);
        assert_eq!(depth[7 + 1], 2);
        assert_eq!(depth[3 * 7 + 3], 3, "capped at 3");
    }

    #[test]
    fn coverage_grows_from_clear_to_storm() {
        let grid = grid();
        let mut previous = -1.0;
        for cover in COVERS {
            // Average a few seeds so one lucky sky can't break the order.
            let mean: f64 = (0..4)
                .map(|seed| CloudField::new(seed, &grid, cover).coverage())
                .sum::<f64>()
                / 4.0;
            assert!(mean > previous, "{cover:?}: {mean} after {previous}");
            previous = mean;
        }
        assert_eq!(CloudField::new(1, &grid, Cover::Clear).coverage(), 0.0);
        let storm = CloudField::new(1, &grid, Cover::Storm).coverage();
        assert!(storm > 0.4, "storms cover much of the sky: {storm}");
    }

    #[test]
    fn clouds_stay_in_the_sky_above_the_ground_row() {
        let grid = grid();
        for cover in COVERS {
            let field = CloudField::new(7, &grid, cover);
            for cloud in &field.clouds {
                assert!(cloud.top >= 0 && cloud.top + cloud.height <= grid.ground_row);
            }
        }
    }

    #[test]
    fn wind_drifts_at_the_spec_rate_and_endless_adds_parallax() {
        let grid = grid();
        let mut field = CloudField::new(2, &grid, Cover::Scattered);
        for _ in 0..300 {
            field.advance(1.0 / 30.0, 1.0, 0.0);
        }
        assert!((field.view() - 3.5).abs() < 1e-9, "10 s at 0.35 cells/s");
        field.advance(1.0, 4.0, 100.0);
        let expected = 3.5 + WIND_SPEED * 4.0 / 30.0 + CLOUD_PARALLAX * 100.0;
        assert!((field.view() - expected).abs() < 1e-9);
    }

    #[test]
    fn clouds_wrap_with_new_shapes_and_the_count_is_kept() {
        let grid = grid();
        let mut field = CloudField::new(3, &grid, Cover::Broken);
        let count = field.clouds.len();
        let first = field.clouds.clone();
        let span = field.span_width();
        // An hour of Endless at 4x: the camera covers 28,800 cells.
        for frame in 0..(3600 * 30) {
            let camera = 8.0 * f64::from(frame) / 30.0;
            field.advance(1.0 / 30.0, 4.0, camera);
        }
        assert_eq!(field.clouds.len(), count);
        assert_ne!(field.clouds, first, "shapes were renewed");
        let left = field.view().floor() as i32 - WRAP_MARGIN;
        for cloud in &field.clouds {
            assert!(cloud.x + cloud.width >= left, "nothing left behind");
            assert!(cloud.x < left + span + 200, "nothing runs far ahead");
        }
    }

    #[test]
    fn same_seed_gives_the_same_sky() {
        let grid = grid();
        let a = CloudField::new(9, &grid, Cover::Overcast);
        let b = CloudField::new(9, &grid, Cover::Overcast);
        assert_eq!(a.clouds, b.clouds);
        assert_ne!(a.clouds, CloudField::new(10, &grid, Cover::Overcast).clouds);
    }

    fn lit_cloud() -> Cloud {
        let mask = vec![true; 20 * 8];
        Cloud {
            kind: CloudKind::Bank,
            x: 0,
            top: 0,
            width: 20,
            height: 8,
            depth: edge_depths(&mask, 20, 8),
        }
    }

    #[test]
    fn a_full_moon_lights_nearby_clouds_and_a_new_moon_does_not() {
        let cloud = lit_cloud();
        let moon = |brightness| CloudLights {
            moon: Some(MoonLight {
                x: 14.0,
                row: 4.0,
                radius: 19.5,
                brightness,
            }),
        };
        let at =
            |lights: &CloudLights| cell_light(&cloud, 10, 4, (4, 3), (10.5, 4.5), 0.25, lights);
        let none = at(&CloudLights::default());
        assert!(at(&moon(1.0)) > none + 0.2);
        assert_eq!(at(&moon(0.0)), none);
        assert_eq!(MoonLight::brightness_for_phase(0.5), 1.0);
        assert_eq!(MoonLight::brightness_for_phase(0.0), 0.0);
        assert!((MoonLight::brightness_for_phase(0.25) - 0.5).abs() < 1e-12);
        // Far from the moon there is no glow.
        let far = CloudLights {
            moon: Some(MoonLight {
                x: 500.0,
                ..moon(1.0).moon.unwrap()
            }),
        };
        assert_eq!(at(&far), none);
    }

    #[test]
    fn the_edge_facing_the_moon_gets_a_rim() {
        let cloud = lit_cloud();
        // Moon far to the right, beyond the glow, so only the rim differs.
        let lights = CloudLights {
            moon: Some(MoonLight {
                x: 400.0,
                row: 4.0,
                radius: 19.5,
                brightness: 1.0,
            }),
        };
        let facing = cell_light(&cloud, 19, 4, (4, 3), (19.5, 4.5), 0.25, &lights);
        let away = cell_light(&cloud, 0, 4, (4, 3), (0.5, 4.5), 0.25, &lights);
        assert!(facing > away + 0.3, "{facing} vs {away}");
    }

    #[test]
    fn undersides_are_darker_than_tops() {
        let cloud = lit_cloud();
        let lights = CloudLights::default();
        let top = cell_light(&cloud, 10, 0, (0, 7), (10.5, 0.5), 0.25, &lights);
        let bottom = cell_light(&cloud, 10, 7, (7, 0), (10.5, 7.5), 0.25, &lights);
        assert!(top > bottom);
    }

    #[test]
    fn drawn_clouds_use_four_palette_tones_with_translucent_edges() {
        let grid = grid();
        let field = CloudField::new(5, &grid, Cover::Broken);
        for palette in palettes() {
            let rgba = field.draw(palette, &CloudLights::default());
            assert_eq!(rgba.len(), field.strip().cells() * 4);
            let ramp = tone_ramp(Cover::Broken, palette);
            let opaque: Vec<[u8; 3]> = rgba
                .chunks(4)
                .filter(|p| p[3] == 255)
                .map(|p| [p[0], p[1], p[2]])
                .collect();
            assert!(!opaque.is_empty(), "{}", palette.key);
            // Opaque cells covered by a single cloud are exactly ramp tones;
            // allow blends where clouds overlap.
            let ramp_cells = opaque
                .iter()
                .filter(|c| ramp.iter().any(|t| t.0 == **c))
                .count();
            assert!(ramp_cells * 10 > opaque.len() * 9, "{}", palette.key);
            assert!(rgba.chunks(4).any(|p| p[3] > 0 && p[3] < 255), "soft edges");
            assert!(rgba.chunks(4).any(|p| p[3] == 0), "open sky stays clear");
        }
    }

    #[test]
    fn storm_tones_are_darker_than_fair_weather() {
        let classic = classic();
        let luminance = |c: Rgb| c.0.iter().map(|&v| u32::from(v)).sum::<u32>();
        let storm = tone_ramp(Cover::Storm, classic);
        let fair = tone_ramp(Cover::Puffs, classic);
        assert!(luminance(storm[TONES - 1]) < luminance(fair[TONES - 1]));
        assert!(tone_index(0.0) == 0 && tone_index(1.0) == TONES - 1);
    }

    #[test]
    fn the_strip_follows_the_view_with_a_fractional_offset() {
        let grid = grid();
        let mut field = CloudField::new(6, &grid, Cover::Scattered);
        field.advance(1.0 / 30.0, 1.0, 37.3);
        let strip = field.strip();
        assert_eq!(strip.origin_x, field.view().floor() as i32 - STRIP_MARGIN);
        assert_eq!(strip.width, grid.cols + 2 * STRIP_MARGIN);
        let offset = field.strip_offset_px(grid.cell);
        let cell = f64::from(grid.cell);
        assert!(offset >= 2.0 * cell && offset < 3.0 * cell, "{offset}");
    }
}
