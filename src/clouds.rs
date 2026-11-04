//! Clouds (Rebuild Prompt, "Clouds"; Starry Journey extensions, owner
//! decisions 2026-11-03: variety and density for later rain and storms,
//! clouds in both modes, translucent and dynamically lit).
//!
//! **Shapes.** Clouds are horizontal and layered, like the owner's
//! references (the Python version's stepped lenses, and the layered haze
//! of the mood images), built from two-cell-high strips as the spec says:
//! - *wisps*: long thin streaks, one or two strips tall;
//! - *lenses*: flat stepped saucers, widest just below the middle and
//!   tapering in steps to pointed ends;
//! - *banks*: stacks of offset, uneven strips with occasional gaps between
//!   layers, so they read as stratified haze. Overcast and storms use wide,
//!   deep banks.
//!
//! **Coverage.** A [`Cover`] preset (clear, puffs, scattered, broken,
//! overcast, storm) sets how many clouds there are, which kinds, how big,
//! how dark, and how opaque. Scattered keeps the spec's count:
//! `max(3, ceil(cols / 130))`.
//!
//! **Translucency.** No cloud is fully opaque: wisps are faint, and even
//! the thickest storm cell lets some light through, so the moon (and later
//! the sun) always glows through. Edges are fainter than interiors.
//!
//! **Motion.** Clouds live in their own world, seen through a view that
//! drifts with the wind at 0.35 cells per second times the speed setting
//! and, in Endless mode, follows the root camera at a tenth of its speed.
//! A cloud that drifts out past the left of the `cols + 160` wrap span
//! comes back on the right with a fresh shape. Seeded with
//! `seed XOR 0xc2b2ae35`.
//!
//! **Light.** Cells store only shape (how far each is from the edge);
//! color comes from the light sources each time the strip is drawn,
//! snapped to four tones between the palette's sky and star colors. This
//! branch has ambient night light and moonlight: brightness by phase, a
//! lit rim on the edges facing the moon, and a glow near it. City glow,
//! searchlights, the sun, and lightning join the same model later.

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
/// Opacity of a cloud's outermost cells, relative to its interior.
pub const EDGE_FADE: f64 = 0.6;
/// The most opaque any cloud cell can be, so light always bleeds through.
pub const MAX_OPACITY: f64 = 0.85;
/// The most opaque overlapping clouds can stack up to, so light bleeds
/// through even the heaviest storm.
pub const MAX_STACKED_OPACITY: f64 = 0.9;
/// Extra columns on each side of the drawn strip, so the GPU can slide it.
pub const STRIP_MARGIN: i32 = 2;

/// Cloud coverage presets, in the order `C` cycles them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cover {
    /// No clouds.
    Clear,
    /// A few small lenses and wisps.
    Puffs,
    /// The spec's default: a few lenses and wisps.
    #[default]
    Scattered,
    /// More and bigger clouds, some layered banks.
    Broken,
    /// Wide layered banks across much of the sky.
    Overcast,
    /// Deep, dark banks (lightning arrives in a later feature).
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
    Wisp,
    Lens,
    Bank,
}

impl CloudKind {
    /// Interior opacity before the preset's scale: wisps are faint.
    fn opacity(self) -> f64 {
        match self {
            CloudKind::Wisp => 0.45,
            CloudKind::Lens => 0.7,
            CloudKind::Bank => 0.8,
        }
    }
}

/// What a preset asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CoverSpec {
    /// Columns per cloud (`f64::INFINITY` for none).
    cols_per_cloud: f64,
    /// Fewest clouds when there are any.
    min_clouds: usize,
    /// Relative weights of wisps, lenses, and banks.
    weights: [f64; 3],
    /// Size multiplier for lenses and banks.
    size: f64,
    /// Ambient light on the clouds (0 dark, 1 bright).
    ambient: f64,
    /// Tone range: fractions from the sky color toward the star color.
    tone_range: (f64, f64),
    /// Altitude band for cloud tops, as fractions of the ground row.
    band: (f64, f64),
    /// Bank heights as fractions of the sky's rows.
    bank_height: (f64, f64),
    /// Bank widths as fractions of the screen's columns.
    bank_width: (f64, f64),
    /// Opacity scale (thicker skies are more opaque, up to MAX_OPACITY).
    opacity: f64,
}

impl Cover {
    fn spec(self) -> CoverSpec {
        let fair = CoverSpec {
            cols_per_cloud: f64::INFINITY,
            min_clouds: 0,
            weights: [0.5, 0.5, 0.0],
            size: 1.0,
            ambient: 0.3,
            tone_range: (0.12, 0.5),
            band: (0.04, 0.45),
            bank_height: (0.05, 0.08),
            bank_width: (0.25, 0.45),
            opacity: 0.9,
        };
        match self {
            Cover::Clear => fair,
            Cover::Puffs => CoverSpec {
                cols_per_cloud: 110.0,
                min_clouds: 4,
                weights: [0.4, 0.6, 0.0],
                size: 0.7,
                ..fair
            },
            Cover::Scattered => CoverSpec {
                cols_per_cloud: 130.0,
                min_clouds: 3,
                weights: [0.45, 0.55, 0.0],
                size: 1.2,
                ..fair
            },
            Cover::Broken => CoverSpec {
                cols_per_cloud: 55.0,
                min_clouds: 6,
                weights: [0.35, 0.4, 0.25],
                size: 1.3,
                ambient: 0.27,
                tone_range: (0.07, 0.4),
                band: (0.03, 0.42),
                bank_height: (0.06, 0.12),
                opacity: 0.95,
                ..fair
            },
            Cover::Overcast => CoverSpec {
                cols_per_cloud: 70.0,
                min_clouds: 6,
                weights: [0.25, 0.15, 0.6],
                size: 1.5,
                ambient: 0.24,
                tone_range: (0.07, 0.34),
                band: (0.0, 0.35),
                bank_height: (0.12, 0.22),
                bank_width: (0.45, 0.9),
                opacity: 1.0,
            },
            Cover::Storm => CoverSpec {
                cols_per_cloud: 55.0,
                min_clouds: 8,
                weights: [0.15, 0.05, 0.8],
                size: 1.8,
                ambient: 0.2,
                tone_range: (0.06, 0.28),
                band: (0.0, 0.3),
                bank_height: (0.2, 0.36),
                bank_width: (0.6, 1.2),
                opacity: 1.0,
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
    /// Interior opacity, at most [`MAX_OPACITY`].
    pub opacity: f64,
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

    /// The opacity of local cell `(cx, cy)`: the interior opacity, faded on
    /// the outermost cells; 0 outside.
    pub fn alpha_at(&self, cx: i32, cy: i32) -> f64 {
        match self.depth_at(cx, cy) {
            0 => 0.0,
            1 => self.opacity * EDGE_FADE,
            _ => self.opacity,
        }
    }
}

/// One horizontal run of a cloud: two rows (a strip), from the bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Run {
    strip: i32,
    left: i32,
    right: i32,
}

/// Builds a mask `width x 2 * strips` from runs. O(cells).
fn mask_from_runs(width: i32, strips: i32, runs: &[Run]) -> Vec<bool> {
    let height = 2 * strips;
    let mut mask = vec![false; (width * height) as usize];
    for run in runs {
        for row in [height - 1 - 2 * run.strip, height - 2 - 2 * run.strip] {
            if !(0..height).contains(&row) {
                continue;
            }
            for x in run.left.max(0)..run.right.min(width) {
                mask[(row * width + x) as usize] = true;
            }
        }
    }
    mask
}

/// Drops empty strips above the shape. Returns the new height. O(cells).
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

/// Rounds a half-width down to an even number of cells, so outlines step
/// in two-cell increments like the spec's strips.
fn even(half: f64) -> i32 {
    ((half / 2.0).floor() as i32 * 2).max(1)
}

/// Runs for a lens `width` wide and `strips` tall: widest a third of the
/// way up, narrowing in steps toward the top and (less) toward the bottom,
/// so the ends are pointed and the base nearly flat.
fn lens_runs(width: i32, strips: i32, offset: i32, base: i32) -> Vec<Run> {
    let center = f64::from(width) / 2.0;
    let widest = f64::from(strips - 1) / 3.0;
    (0..strips)
        .map(|strip| {
            let above = f64::from(strip) - widest;
            let reach = if above >= 0.0 {
                f64::from(strips) - widest
            } else {
                widest + 1.0
            };
            let t = (above.abs() / reach).min(1.0);
            let half = even(center * (1.0 - 0.75 * t.powf(1.2)));
            Run {
                strip: base + strip,
                left: offset + (center as i32 - half),
                right: offset + (center as i32 + half),
            }
        })
        .collect()
}

/// Generates one cloud of `kind` with its left edge at `x`. O(cells).
///
/// - Wisp: one long strip, sometimes with a shorter one above or below,
///   offset to one side.
/// - Lens: one stepped lens.
/// - Bank: a stack of strips whose widths and offsets wander; some strips
///   are skipped for gaps between layers, and lenses ride on top.
fn make_cloud(
    rng: &mut Mulberry32,
    kind: CloudKind,
    x: i32,
    spec: &CoverSpec,
    sky: (i32, i32),
) -> Cloud {
    let (cols, sky_rows) = sky;
    let between = |rng: &mut Mulberry32, lo: i64, hi: i64| rng.rand_int(lo, hi.max(lo)) as i32;
    let unit = |rng: &mut Mulberry32| rng.next_f64();
    let mut runs = Vec::new();
    let (width, strips) = match kind {
        CloudKind::Wisp => {
            let w = (f64::from(between(rng, 24, 110)) * spec.size.sqrt()).round() as i32;
            runs.push(Run {
                strip: 0,
                left: 0,
                right: w,
            });
            let strips = if unit(rng) < 0.5 {
                let len = (f64::from(w) * (0.3 + 0.4 * unit(rng))) as i32;
                let start = between(rng, 0, i64::from(w - len));
                runs.push(Run {
                    strip: 1,
                    left: start,
                    right: start + len,
                });
                2
            } else {
                1
            };
            (w, strips)
        }
        CloudKind::Lens => {
            let w = (f64::from(between(rng, 38, 90)) * spec.size).round() as i32;
            // Rebuild Prompt heights 6..13 rows: three to six strips.
            let strips = between(rng, 3, 6).min(w / 10).max(2);
            runs.extend(lens_runs(w, strips, 0, 0));
            (w, strips)
        }
        CloudKind::Bank => {
            let (lo, hi) = spec.bank_width;
            let w = (f64::from(cols) * (lo + (hi - lo) * unit(rng))).round() as i32;
            let w = w.max(60);
            let (lo, hi) = spec.bank_height;
            let rows = f64::from(sky_rows) * (lo + (hi - lo) * unit(rng));
            // Flat and layered: never taller than a fifth of the width.
            let strips = ((rows / 2.0).round() as i32).clamp(3, (w / 10).max(3));
            let fw = f64::from(w);
            let mut center = fw / 2.0;
            for strip in 0..strips {
                let rise = f64::from(strip) / f64::from(strips);
                // A few gaps between layers, never at the base.
                if strip > 0 && strip + 1 < strips && unit(rng) < 0.12 {
                    continue;
                }
                let narrowing = 1.0 - 0.55 * rise.powf(1.4);
                let half = even(fw / 2.0 * narrowing * (0.75 + 0.25 * unit(rng)));
                center = (center + (unit(rng) - 0.5) * fw * 0.08)
                    .clamp(f64::from(half), fw - f64::from(half));
                runs.push(Run {
                    strip,
                    left: (center as i32 - half).max(0),
                    right: (center as i32 + half).min(w),
                });
                // Now and then a wispy tail trails off one side.
                if unit(rng) < 0.2 {
                    let tail = (fw * (0.1 + 0.2 * unit(rng))) as i32;
                    let (left, right) = if unit(rng) < 0.5 {
                        ((center as i32 - half - tail).max(0), center as i32 - half)
                    } else {
                        (center as i32 + half, (center as i32 + half + tail).min(w))
                    };
                    runs.push(Run { strip, left, right });
                }
            }
            (w, strips)
        }
    };
    let mut mask = mask_from_runs(width, strips, &runs);
    let height = trim_top(&mut mask, width, 2 * strips);
    let depth = edge_depths(&mask, width, height);
    let (lo, hi) = (
        (f64::from(sky_rows) * spec.band.0).round() as i32,
        (f64::from(sky_rows) * spec.band.1).round() as i32,
    );
    let top = between(rng, i64::from(lo), i64::from(hi))
        .min(sky_rows - height)
        .max(0);
    Cloud {
        kind,
        x,
        top,
        width,
        height,
        depth,
        opacity: (kind.opacity() * spec.opacity).min(MAX_OPACITY),
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
pub const MOON_GLOW_REACH: f64 = 2.5;

/// How much light a cloud cell gets, before quantizing (0 dark, 1 bright).
///
/// `surface` is the cell's distance in rows below the top of its column
/// and above the bottom: the top two rows catch sky light and the bottom
/// two are shaded, so every layer gets its own highlight. The moon adds,
/// scaled by its brightness: a glow fading with the square of the distance
/// within [`MOON_GLOW_REACH`] radii, and a rim on edge cells whose neighbor
/// toward the moon is open sky. O(1).
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
        light += 0.08;
    }
    if surface.1 < 2 {
        light -= 0.08;
    }
    if let Some(moon) = lights.moon {
        let (dx, dy) = (moon.x - screen.0, moon.row - screen.1);
        let distance = (dx * dx + dy * dy).sqrt();
        let fade = (1.0 - distance / (MOON_GLOW_REACH * moon.radius)).max(0.0);
        light += moon.brightness * 0.6 * fade * fade;
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

    fn pick_kind(&mut self) -> CloudKind {
        let weights = self.cover.spec().weights;
        let total: f64 = weights.iter().sum();
        let mut roll = self.rng.next_f64() * total;
        for (kind, weight) in [CloudKind::Wisp, CloudKind::Lens, CloudKind::Bank]
            .into_iter()
            .zip(weights)
        {
            if roll < weight {
                return kind;
            }
            roll -= weight;
        }
        CloudKind::Lens
    }

    fn spawn(&mut self, x: i32) -> Cloud {
        let kind = self.pick_kind();
        let spec = self.cover.spec();
        make_cloud(&mut self.rng, kind, x, &spec, (self.cols, self.sky_rows))
    }

    /// Spreads the preset's clouds evenly over the wrap span, each centered
    /// at a random point within its share. O(clouds × cloud cells).
    fn populate(&mut self) {
        let count = self.cover.cloud_count(self.cols);
        let span = f64::from(self.span_width());
        let left = self.view.floor() as i32 - WRAP_MARGIN;
        self.clouds = (0..count)
            .map(|i| {
                let share = span / count as f64;
                let x = left + (share * (i as f64 + self.rng.next_f64())) as i32;
                let mut cloud = self.spawn(x);
                cloud.x -= cloud.width / 2;
                cloud
            })
            .collect();
        // Far (higher) clouds first, so nearer, lower ones draw over them.
        self.clouds.sort_by_key(|cloud| cloud.top);
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
    /// lit by `lights`: four tones per palette and preset, every cell
    /// translucent (at most [`MAX_OPACITY`], and overlaps at most
    /// [`MAX_STACKED_OPACITY`]), transparent sky. Clouds are composited over
    /// each other in drawing order with straight alpha.
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
                    let mut alpha = cloud.alpha_at(cx, cy);
                    if alpha == 0.0 {
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
                    let below = f64::from(pixel[3]) / 255.0;
                    // Edges are faint only against open sky; over another
                    // cloud they take the full interior opacity, so
                    // overlaps don't draw outlines.
                    if below > 0.0 {
                        alpha = cloud.opacity;
                    }
                    let total = alpha + below * (1.0 - alpha);
                    for (channel, value) in pixel[..3].iter_mut().zip(tone.0) {
                        let mixed = (f64::from(value) * alpha
                            + f64::from(*channel) * below * (1.0 - alpha))
                            / total.max(1e-9);
                        *channel = mixed.round() as u8;
                    }
                    pixel[3] = (total.min(MAX_STACKED_OPACITY) * 255.0).round() as u8;
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

    fn sky() -> (i32, i32) {
        let grid = grid();
        (grid.cols, grid.ground_row)
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

    fn row_widths(cloud: &Cloud) -> Vec<i32> {
        (0..cloud.height)
            .map(|cy| (0..cloud.width).filter(|&cx| cloud.covers(cx, cy)).count() as i32)
            .collect()
    }

    #[test]
    fn every_shape_is_built_of_two_row_strips() {
        let mut rng = Mulberry32::new(4);
        for cover in COVERS.iter().skip(1) {
            let spec = cover.spec();
            for kind in [CloudKind::Wisp, CloudKind::Lens, CloudKind::Bank] {
                for _ in 0..40 {
                    let cloud = make_cloud(&mut rng, kind, 0, &spec, sky());
                    assert_eq!(cloud.height % 2, 0);
                    for strip in 0..cloud.height / 2 {
                        for x in 0..cloud.width {
                            assert_eq!(cloud.covers(x, 2 * strip), cloud.covers(x, 2 * strip + 1));
                        }
                    }
                    assert!(cloud.area() > 0);
                    assert!(
                        (0..cloud.width).any(|x| cloud.covers(x, 0)),
                        "reaches its top"
                    );
                }
            }
        }
    }

    #[test]
    fn clouds_are_flat_and_wide_like_the_references() {
        let mut rng = Mulberry32::new(5);
        let spec = Cover::Broken.spec();
        for kind in [CloudKind::Wisp, CloudKind::Lens, CloudKind::Bank] {
            for _ in 0..100 {
                let cloud = make_cloud(&mut rng, kind, 0, &spec, sky());
                assert!(
                    cloud.width >= 4 * cloud.height,
                    "{kind:?} {} x {}",
                    cloud.width,
                    cloud.height
                );
            }
        }
        // Wisps are thin streaks.
        for _ in 0..100 {
            let wisp = make_cloud(&mut rng, CloudKind::Wisp, 0, &spec, sky());
            assert!(wisp.height <= 4);
        }
    }

    #[test]
    fn lenses_are_widest_low_and_taper_to_both_ends() {
        let mut rng = Mulberry32::new(6);
        let spec = Cover::Scattered.spec();
        for _ in 0..100 {
            let lens = make_cloud(&mut rng, CloudKind::Lens, 0, &spec, sky());
            let widths = row_widths(&lens);
            let widest = *widths.iter().max().unwrap();
            let at = widths.iter().position(|&w| w == widest).unwrap() as i32;
            assert!(
                at >= lens.height / 2 - 1,
                "widest in the lower part: {widths:?}"
            );
            assert!(widths[0] < widest, "the top is narrower: {widths:?}");
            // Stepped outline: rows only change width between strips.
            for strip in 0..lens.height / 2 {
                assert_eq!(widths[2 * strip as usize], widths[2 * strip as usize + 1]);
            }
        }
    }

    #[test]
    fn banks_are_layered_and_narrow_upward() {
        let mut rng = Mulberry32::new(7);
        let spec = Cover::Overcast.spec();
        let mut gaps = 0;
        for _ in 0..60 {
            let bank = make_cloud(&mut rng, CloudKind::Bank, 0, &spec, sky());
            let widths = row_widths(&bank);
            let bottom = widths[widths.len() - 1];
            assert!(widths[0] <= bottom, "narrower at the top: {widths:?}");
            gaps += widths.iter().filter(|&&w| w == 0).count();
        }
        assert!(gaps > 0, "some banks have gaps between layers");
    }

    #[test]
    fn every_cloud_lets_light_through() {
        let mut rng = Mulberry32::new(8);
        for cover in COVERS.iter().skip(1) {
            let spec = cover.spec();
            for kind in [CloudKind::Wisp, CloudKind::Lens, CloudKind::Bank] {
                let cloud = make_cloud(&mut rng, kind, 0, &spec, sky());
                assert!(cloud.opacity <= MAX_OPACITY && cloud.opacity > 0.2);
                for cy in 0..cloud.height {
                    for cx in 0..cloud.width {
                        assert!(cloud.alpha_at(cx, cy) <= MAX_OPACITY);
                    }
                }
            }
        }
        let wisp = make_cloud(
            &mut rng,
            CloudKind::Wisp,
            0,
            &Cover::Scattered.spec(),
            sky(),
        );
        let bank = make_cloud(&mut rng, CloudKind::Bank, 0, &Cover::Storm.spec(), sky());
        assert!(
            wisp.opacity < bank.opacity,
            "wisps are fainter than storm banks"
        );
    }

    #[test]
    fn drawn_strips_are_never_fully_opaque_even_where_clouds_overlap() {
        let grid = grid();
        let field = CloudField::new(2, &grid, Cover::Storm);
        let rgba = field.draw(classic(), &CloudLights::default());
        let most = rgba.chunks(4).map(|p| p[3]).max().unwrap();
        assert!(most < 255, "light bleeds through: {most}");
        assert!(most > 200, "but storms are thick: {most}");
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
            assert!(cloud.x < left + span + grid.cols, "nothing runs far ahead");
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

    fn block_cloud() -> Cloud {
        let mask = vec![true; 20 * 8];
        Cloud {
            kind: CloudKind::Bank,
            x: 0,
            top: 0,
            width: 20,
            height: 8,
            depth: edge_depths(&mask, 20, 8),
            opacity: 0.8,
        }
    }

    #[test]
    fn a_full_moon_lights_nearby_clouds_and_a_new_moon_does_not() {
        let cloud = block_cloud();
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
        let cloud = block_cloud();
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
    fn tops_are_brighter_than_undersides() {
        let cloud = block_cloud();
        let lights = CloudLights::default();
        let top = cell_light(&cloud, 10, 0, (0, 7), (10.5, 0.5), 0.25, &lights);
        let bottom = cell_light(&cloud, 10, 7, (7, 0), (10.5, 7.5), 0.25, &lights);
        assert!(top > bottom);
    }

    #[test]
    fn drawn_clouds_use_palette_tones_and_leave_open_sky_clear() {
        let grid = grid();
        let field = CloudField::new(5, &grid, Cover::Broken);
        for palette in palettes() {
            let rgba = field.draw(palette, &CloudLights::default());
            assert_eq!(rgba.len(), field.strip().cells() * 4);
            let ramp = tone_ramp(Cover::Broken, palette);
            let single: Vec<[u8; 3]> = rgba
                .chunks(4)
                .filter(|p| p[3] > 0)
                .map(|p| [p[0], p[1], p[2]])
                .collect();
            assert!(!single.is_empty(), "{}", palette.key);
            let on_ramp = single
                .iter()
                .filter(|c| ramp.iter().any(|t| t.0 == **c))
                .count();
            assert!(on_ramp * 10 > single.len() * 8, "{}", palette.key);
            assert!(rgba.chunks(4).any(|p| p[3] == 0), "open sky stays clear");
        }
    }

    #[test]
    fn storm_tones_are_darker_than_fair_weather() {
        let luminance = |c: Rgb| c.0.iter().map(|&v| u32::from(v)).sum::<u32>();
        let storm = tone_ramp(Cover::Storm, classic());
        let fair = tone_ramp(Cover::Puffs, classic());
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
