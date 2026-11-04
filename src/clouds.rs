//! Clouds (Rebuild Prompt, "Clouds"; Starry Journey extensions, owner
//! decisions 2026-11-03: variety and density for later rain and storms,
//! clouds in both modes, translucent, dynamically lit, and styled after
//! the owner's two references).
//!
//! Two layers, each with its own depth:
//!
//! - **Near clouds** follow the owner's outlined pixel-art cumulus: a flat
//!   bottom, a stepped top of rounded lobes (biggest in the middle), a
//!   crisp one-cell outline, a shadow band along the bottom, a highlight
//!   under the top outline, crease lines where a lower lobe overlaps one
//!   behind it, and sometimes a short tail. They drift across the sky.
//! - **Far clouds** follow the owner's distant-cloud reference: towers of
//!   many small round puffs rising from the horizon behind the city, and a
//!   lumpy bank along the horizon in front of them, with no outline. Each
//!   puff is shaded like a small sphere lit from the moon's direction, so
//!   the side facing the light is bright and the far side falls into
//!   shadow. They drift slower than everything else.
//!
//! **Coverage.** A [`Cover`] preset (clear, puffs, scattered, broken,
//! overcast, storm) sets how many clouds each layer gets, how big, and how
//! dark. Storms bring heavy near clouds and a tall dark horizon bank.
//!
//! **Translucency.** No cloud cell is fully opaque, and overlaps are
//! capped, so the moon (and later the sun) always shows through. The
//! moon's soft glow on the clouds is added by the GPU ([`moon_glow`]).
//!
//! **Motion.** Each layer is seen through its own view, drifting with the
//! wind times the speed setting and, in Endless mode, following a fraction
//! of the root camera. A cloud that drifts out past the left of the
//! `cols + 160` wrap span comes back on the right with a fresh shape.
//! Seeded with `seed XOR 0xc2b2ae35` (far clouds with a further salt).
//!
//! Deviation from the spec: outlines step one cell at a time, like the
//! owner's reference, rather than in two-cell strips.

use crate::model::grid::Grid;
use crate::model::palette::{Palette, Rgb};
use crate::model::rng::{cloud_seed, Mulberry32};
use crate::raster::mix;
use crate::street::Span;

/// Cells beyond each screen edge in the wrap span (`cols + 160` in all).
pub const WRAP_MARGIN: i32 = 80;
/// Longest simulated motion step per frame, in seconds.
pub const MAX_MOTION_STEP: f64 = 1.0 / 30.0;
/// Number of tones a cloud is drawn in.
pub const TONES: usize = 4;
/// The most opaque any single cloud cell can be.
pub const MAX_OPACITY: f64 = 0.85;
/// The most opaque overlapping clouds can stack up to, so light bleeds
/// through even the heaviest storm.
pub const MAX_STACKED_OPACITY: f64 = 0.9;
/// Extra columns on each side of the drawn strip, so the GPU can slide it.
pub const STRIP_MARGIN: i32 = 2;
/// Seed salt that separates the far layer from the near one.
const FAR_SALT: u32 = 0x51f1_5eed;

/// Which cloud layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    /// Outlined cumulus drifting across the sky.
    Near,
    /// Billowing towers and a bank on the horizon behind the city.
    Far,
}

impl Depth {
    /// Wind drift in cells per second at 1x. The near layer keeps the
    /// spec's 0.35.
    pub fn wind(self) -> f64 {
        match self {
            Depth::Near => 0.35,
            Depth::Far => 0.12,
        }
    }

    /// Fraction of the Endless root camera's motion the layer follows.
    pub fn parallax(self) -> f64 {
        match self {
            Depth::Near => 0.1,
            Depth::Far => 0.04,
        }
    }
}

/// Cloud coverage presets, in the order `C` cycles them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cover {
    /// No clouds.
    Clear,
    /// A few small puffs.
    Puffs,
    /// The default: a few cumulus and a distant tower or two.
    #[default]
    Scattered,
    /// More and bigger cumulus, towers, and a low horizon bank.
    Broken,
    /// Big overlapping cumulus and a tall horizon bank.
    Overcast,
    /// Heavy, dark clouds (lightning arrives in a later feature).
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

/// The cloud shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudKind {
    /// Near: outlined cumulus (small ones are puffs).
    Cumulus,
    /// Far: a billowing tower of puffs.
    Tower,
    /// Far: a lumpy bank along the horizon.
    Bank,
}

/// What a preset asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CoverSpec {
    /// Near clouds: columns per cloud (`f64::INFINITY` for none).
    cols_per_cloud: f64,
    /// Near clouds: fewest when there are any.
    min_clouds: usize,
    /// Near cumulus widths in cells.
    width: (i64, i64),
    /// Altitude band for near cloud tops, as fractions of the ground row.
    band: (f64, f64),
    /// Near tone ramp (outline, shadow, body, highlight) as fractions from
    /// the sky color toward the star color.
    near_tones: [f64; TONES],
    /// Far towers per 640 columns.
    towers: f64,
    /// Far tower heights as fractions of the sky's rows.
    tower_height: (f64, f64),
    /// Far horizon bank height as a fraction of the sky's rows (0: none).
    bank_height: f64,
    /// Far tone ramp from shadow to lit, as fractions toward the star color.
    far_tones: [f64; TONES],
    /// Opacity of cloud interiors (at most MAX_OPACITY).
    opacity: f64,
}

impl Cover {
    fn spec(self) -> CoverSpec {
        let base = CoverSpec {
            cols_per_cloud: f64::INFINITY,
            min_clouds: 0,
            width: (30, 70),
            band: (0.05, 0.4),
            near_tones: [0.08, 0.24, 0.36, 0.5],
            towers: 0.0,
            tower_height: (0.2, 0.35),
            bank_height: 0.0,
            far_tones: [0.05, 0.14, 0.25, 0.38],
            opacity: 0.8,
        };
        match self {
            Cover::Clear => base,
            Cover::Puffs => CoverSpec {
                cols_per_cloud: 100.0,
                min_clouds: 4,
                width: (14, 30),
                ..base
            },
            Cover::Scattered => CoverSpec {
                cols_per_cloud: 130.0,
                min_clouds: 3,
                towers: 1.5,
                ..base
            },
            Cover::Broken => CoverSpec {
                cols_per_cloud: 60.0,
                min_clouds: 6,
                width: (30, 95),
                towers: 3.0,
                tower_height: (0.25, 0.45),
                bank_height: 0.08,
                ..base
            },
            Cover::Overcast => CoverSpec {
                cols_per_cloud: 38.0,
                min_clouds: 9,
                width: (60, 150),
                band: (0.0, 0.42),
                near_tones: [0.06, 0.18, 0.28, 0.4],
                towers: 4.0,
                tower_height: (0.3, 0.55),
                bank_height: 0.18,
                far_tones: [0.04, 0.11, 0.19, 0.29],
                opacity: 0.85,
            },
            Cover::Storm => CoverSpec {
                cols_per_cloud: 30.0,
                min_clouds: 11,
                width: (80, 180),
                band: (0.0, 0.42),
                near_tones: [0.04, 0.12, 0.2, 0.3],
                towers: 5.0,
                tower_height: (0.4, 0.65),
                bank_height: 0.28,
                far_tones: [0.03, 0.08, 0.14, 0.22],
                opacity: 0.85,
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

    /// How many near clouds a sky `cols` wide gets.
    pub fn cloud_count(self, cols: i32) -> usize {
        let spec = self.spec();
        if spec.cols_per_cloud.is_infinite() {
            return 0;
        }
        let wanted = (f64::from(cols.max(1)) / spec.cols_per_cloud).ceil() as usize;
        wanted.max(spec.min_clouds)
    }

    /// How many far clouds (towers, plus the bank if any) a sky gets.
    pub fn far_count(self, cols: i32) -> usize {
        let spec = self.spec();
        let towers = (spec.towers * f64::from(cols.max(1)) / 640.0).round() as usize;
        let banks = if spec.bank_height > 0.0 {
            (cols.max(1) as usize).div_ceil(240)
        } else {
            0
        };
        towers + banks
    }
}

/// One round puff of a cloud, in the cloud's local cells.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bubble {
    pub cx: f64,
    pub cy: f64,
    pub r: f64,
}

impl Bubble {
    /// Normalized distance of `(x, y)` (cell centers) from the center: below
    /// 1 inside. Near lobes are a little wider than tall.
    fn reach(&self, x: f64, y: f64, stretch: f64) -> f64 {
        let dx = (x - self.cx) / (self.r * stretch);
        let dy = (y - self.cy) / self.r;
        (dx * dx + dy * dy).sqrt()
    }
}

/// What a cell of a near cloud shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Outline,
    Shadow,
    Crease,
    Body,
    Highlight,
}

/// One cloud: its puffs and its cells, placed in its layer's world.
#[derive(Debug, Clone, PartialEq)]
pub struct Cloud {
    pub kind: CloudKind,
    /// World column of the left edge.
    pub x: i32,
    /// Grid row of the top edge.
    pub top: i32,
    pub width: i32,
    pub height: i32,
    /// The puffs, back to front.
    pub bubbles: Vec<Bubble>,
    /// Per cell, row-major: the front-most puff covering it, `-1` for the
    /// flat base or tail, `-2` for outside.
    pub front: Vec<i32>,
    /// Interior opacity, at most [`MAX_OPACITY`].
    pub opacity: f64,
}

/// `front` value for cells outside the cloud.
const OUTSIDE: i32 = -2;
/// `front` value for base and tail cells, which belong to no puff.
const SLAB: i32 = -1;
/// How much wider than tall near lobes are.
const LOBE_STRETCH: f64 = 1.15;

impl Cloud {
    /// Whether the cloud covers local cell `(cx, cy)`.
    pub fn covers(&self, cx: i32, cy: i32) -> bool {
        self.front_at(cx, cy) != OUTSIDE
    }

    fn front_at(&self, cx: i32, cy: i32) -> i32 {
        if cx < 0 || cy < 0 || cx >= self.width || cy >= self.height {
            return OUTSIDE;
        }
        self.front[(cy * self.width + cx) as usize]
    }

    /// Covered cells.
    pub fn area(&self) -> usize {
        self.front.iter().filter(|&&f| f != OUTSIDE).count()
    }

    /// True if a covered cell touches open sky (4-way).
    pub fn is_edge(&self, cx: i32, cy: i32) -> bool {
        self.covers(cx, cy)
            && [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .any(|&(dx, dy)| !self.covers(cx + dx, cy + dy))
    }

    /// The role of a near-cloud cell (see [`Role`]), or `None` outside.
    ///
    /// The outline is every cell touching open sky. Below the outline the
    /// bottom quarter is shadow. One or two rows under the top outline are
    /// highlight. A crease is a cell on the rim of its front lobe that also
    /// lies inside another lobe, so the lower lobe's edge shows over the
    /// one behind it. O(lobes).
    pub fn role(&self, cx: i32, cy: i32) -> Option<Role> {
        let front = self.front_at(cx, cy);
        if front == OUTSIDE {
            return None;
        }
        if self.is_edge(cx, cy) {
            return Some(Role::Outline);
        }
        let shadow_rows = (self.height / 4).max(2);
        if cy >= self.height - 1 - shadow_rows {
            return Some(Role::Shadow);
        }
        if front >= 0 {
            let lobe = self.bubbles[front as usize];
            let (x, y) = (f64::from(cx) + 0.5, f64::from(cy) + 0.5);
            // Only the upper arc of a lobe, where it overlaps one behind
            // it, so a cloud shows a few creases rather than every rim.
            let on_rim = y < lobe.cy && lobe.reach(x, y, LOBE_STRETCH) > 1.0 - 1.2 / lobe.r;
            let over_one_behind = self.bubbles[..front as usize]
                .iter()
                .any(|b| b.reach(x, y, LOBE_STRETCH) < 1.0);
            if on_rim && over_one_behind {
                return Some(Role::Crease);
            }
        }
        let below_top = (1..=2).any(|k| self.is_edge(cx, cy - k) && !self.covers(cx, cy - k - 1));
        if below_top {
            return Some(Role::Highlight);
        }
        Some(Role::Body)
    }
}

/// Marks every cell inside a puff with the front-most puff covering it.
/// Puffs later in the list are in front. O(sum of puff areas).
fn paint_bubbles(
    front: &mut [i32],
    width: i32,
    height: i32,
    bubbles: &[Bubble],
    stretch: f64,
    floor: i32,
) {
    for (index, b) in bubbles.iter().enumerate() {
        let (rx, ry) = (b.r * stretch, b.r);
        let x0 = ((b.cx - rx).floor() as i32).max(0);
        let x1 = ((b.cx + rx).ceil() as i32).min(width);
        let y0 = ((b.cy - ry).floor() as i32).max(0);
        let y1 = ((b.cy + ry).ceil() as i32).min(floor + 1).min(height);
        for y in y0..y1 {
            for x in x0..x1 {
                if b.reach(f64::from(x) + 0.5, f64::from(y) + 0.5, stretch) < 1.0 {
                    front[(y * width + x) as usize] = index as i32;
                }
            }
        }
    }
}

/// An outlined near cumulus `width` wide: lobes on a flat base, the biggest
/// near the middle and smaller toward the ends, with a filled base and
/// sometimes a short tail. Height is about 0.26-0.34 of the width.
/// O(cells × lobes).
fn make_cumulus(rng: &mut Mulberry32, width: i32) -> (i32, Vec<Bubble>, Vec<i32>) {
    let unit = |rng: &mut Mulberry32| rng.next_f64();
    let fw = f64::from(width);
    let height = ((fw * (0.26 + 0.08 * unit(rng))).round() as i32).max(6);
    let fh = f64::from(height);
    let base = height - 1;
    let main_at = 0.35 + 0.25 * unit(rng);
    let main_r = fh * 0.6;
    let mut bubbles = vec![Bubble {
        cx: fw * main_at,
        cy: main_r,
        r: main_r,
    }];
    let lobes = (2 + width / 18).min(7);
    for _ in 0..lobes {
        let at = 0.12 + 0.76 * unit(rng);
        let shrink = 1.0 - 0.9 * (at - main_at).abs();
        let r = (fh * (0.3 + 0.25 * unit(rng)) * shrink).max(2.0);
        // Lower lobes sit in front of the higher ones.
        bubbles.push(Bubble {
            cx: fw * at,
            cy: f64::from(base) + 1.0 - r * (0.55 + 0.25 * unit(rng)),
            r,
        });
    }
    bubbles.sort_by(|a, b| (a.cy - a.r).total_cmp(&(b.cy - b.r)));
    let mut front = vec![OUTSIDE; (width * height) as usize];
    paint_bubbles(&mut front, width, height, &bubbles, LOBE_STRETCH, base);
    // Flat base: fill the bottom two rows between the outermost lobes.
    let covered: Vec<i32> = (0..width)
        .filter(|&x| (0..height).any(|y| front[(y * width + x) as usize] != OUTSIDE))
        .collect();
    if let (Some(&left), Some(&right)) = (covered.first(), covered.last()) {
        let tail = if unit(rng) < 0.6 {
            2 + (4.0 * unit(rng)) as i32
        } else {
            0
        };
        let tail_left = unit(rng) < 0.5;
        for y in [base - 1, base] {
            let (mut l, mut r) = (left + 1, right);
            // The tail is two rows thick where it joins and one at its tip.
            let reach = if y == base { tail } else { tail / 2 };
            if tail_left {
                l -= reach;
            } else {
                r += reach;
            }
            for x in l.max(0)..r.min(width) {
                let cell = &mut front[(y * width + x) as usize];
                if *cell == OUTSIDE {
                    *cell = SLAB;
                }
            }
        }
    }
    (height, bubbles, front)
}

/// A far tower `width` wide and `height` tall: puffs piled from the base,
/// narrowing toward a rounded top, with one to three peaks. O(cells × puffs).
fn make_tower(rng: &mut Mulberry32, width: i32, height: i32) -> (Vec<Bubble>, Vec<i32>) {
    let unit = |rng: &mut Mulberry32| rng.next_f64();
    let (fw, fh) = (f64::from(width), f64::from(height));
    let peaks: Vec<(f64, f64, f64)> = (0..1 + (unit(rng) * 2.5) as usize)
        .map(|i| {
            let at = if i == 0 {
                0.35 + 0.3 * unit(rng)
            } else {
                0.15 + 0.7 * unit(rng)
            };
            let tall = if i == 0 { 1.0 } else { 0.45 + 0.4 * unit(rng) };
            let wide = 0.3 + 0.25 * unit(rng);
            (at, tall, wide)
        })
        .collect();
    let mut bubbles = Vec::new();
    for &(at, tall, wide) in &peaks {
        // Bigger puffs, fewer of them: each one reads as a lit sphere.
        let mean_r = fh * 0.11;
        let count = (fw * fh * tall * wide / (mean_r * mean_r * 1.2)) as usize + 6;
        for _ in 0..count {
            // More puffs low down; the column narrows as it rises.
            let t = unit(rng).powf(0.8);
            let half = fw * wide / 2.0 * (1.0 - 0.7 * t.powf(1.5));
            let r = (fh * (0.08 + 0.06 * unit(rng)) * (1.0 - 0.35 * t)).max(2.0);
            let cx = fw * at + (unit(rng) * 2.0 - 1.0) * half;
            let cy = fh - r - t * (fh * tall - 2.0 * r);
            bubbles.push(Bubble {
                cx: cx.clamp(r, fw - r),
                cy: cy.max(r),
                r,
            });
        }
    }
    // Higher puffs in front: the billowing outline is what catches light.
    bubbles.sort_by(|a, b| b.cy.total_cmp(&a.cy));
    let mut front = vec![OUTSIDE; (width * height) as usize];
    paint_bubbles(&mut front, width, height, &bubbles, 1.0, height - 1);
    (bubbles, front)
}

/// A far horizon bank: a lumpy wall of small puffs along the bottom of the
/// sky, `height` tall at its highest. O(cells × puffs).
fn make_bank(rng: &mut Mulberry32, width: i32, height: i32) -> (Vec<Bubble>, Vec<i32>) {
    let unit = |rng: &mut Mulberry32| rng.next_f64();
    let (fw, fh) = (f64::from(width), f64::from(height));
    let mut bubbles = Vec::new();
    let mut x = 0.0;
    while x < fw {
        let r = (fh * (0.12 + 0.18 * unit(rng))).max(2.0);
        let lift = fh * (0.2 + 0.8 * unit(rng));
        // A column of puffs from the base up to this lump's height.
        let mut y = fh - r;
        // Stop before a puff would poke out of the top and be cut flat.
        while y > fh - lift && y - r > 0.0 {
            bubbles.push(Bubble {
                cx: x + (unit(rng) - 0.5) * r,
                cy: y,
                r: r * (0.8 + 0.3 * unit(rng)),
            });
            y -= r * 0.9;
        }
        x += r * (0.9 + 0.5 * unit(rng));
    }
    bubbles.sort_by(|a, b| b.cy.total_cmp(&a.cy));
    let mut front = vec![OUTSIDE; (width * height) as usize];
    paint_bubbles(&mut front, width, height, &bubbles, 1.0, height - 1);
    (bubbles, front)
}

/// Drops empty rows above a cloud. Returns the new height. O(cells).
fn trim_top(front: &mut Vec<i32>, width: i32, height: i32) -> i32 {
    let w = width as usize;
    let first = (0..height as usize)
        .find(|&row| front[row * w..(row + 1) * w].iter().any(|&f| f != OUTSIDE))
        .unwrap_or(height as usize);
    front.drain(..first * w);
    height - first as i32
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
    /// Which side is lit: +1 right (waxing), -1 left (waning), 0 full or
    /// new.
    pub lit_side: f64,
}

impl MoonLight {
    /// Brightness for a lunar phase (0 new, 0.5 full, 1 new again).
    pub fn brightness_for_phase(phase: f64) -> f64 {
        1.0 - (phase.rem_euclid(1.0) - 0.5).abs() * 2.0
    }

    /// The moon at `(x, row)` with `radius`, lit for `phase`.
    pub fn for_phase(x: f64, row: f64, radius: f64, phase: f64) -> Self {
        let phase = phase.rem_euclid(1.0);
        let lit_side = if phase == 0.0 || phase == 0.5 {
            0.0
        } else if phase < 0.5 {
            1.0
        } else {
            -1.0
        };
        Self {
            x,
            row,
            radius,
            brightness: Self::brightness_for_phase(phase),
            lit_side,
        }
    }

    /// Where the glow is centered, in cells: on the disk for a full moon,
    /// leaning toward the lit limb as the moon thins to a crescent.
    pub fn glow_center(&self) -> (f64, f64) {
        let lean = self.lit_side * self.radius * 0.6 * (1.0 - self.brightness);
        (self.x + lean, self.row)
    }
}

/// How far the moon's glow on clouds reaches, in moon radii.
pub const MOON_GLOW_REACH: f64 = 3.0;
/// Glow strength on a cloud cell next to a full moon.
pub const MOON_GLOW_STRENGTH: f64 = 0.55;

/// The moon's glow on a cloud at `(x, row)` (cells): a smooth falloff
/// with the square of the distance from [`MoonLight::glow_center`], out to
/// [`MOON_GLOW_REACH`] radii, scaled by brightness. The presentation shader
/// applies this same formula per device pixel in linear light, weighted by
/// the cloud's opacity, so it brightens clouds but never open sky, and
/// never quantizes into rings. O(1).
pub fn moon_glow(moon: &MoonLight, x: f64, row: f64) -> f64 {
    let (cx, cy) = moon.glow_center();
    let distance = ((x - cx).powi(2) + (row - cy).powi(2)).sqrt();
    let fade = (1.0 - distance / (MOON_GLOW_REACH * moon.radius)).max(0.0);
    MOON_GLOW_STRENGTH * moon.brightness * fade * fade
}

/// Everything lighting the clouds this frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CloudLights {
    pub moon: Option<MoonLight>,
}

/// The tone index of a near-cloud cell: outline 0, shadow and crease 1,
/// body 2, highlight 3. An outline cell whose open side faces a moon of at
/// least half brightness becomes a highlight: the silver lining.
pub fn near_tone(
    cloud: &Cloud,
    cx: i32,
    cy: i32,
    screen: (f64, f64),
    lights: &CloudLights,
) -> Option<usize> {
    let role = cloud.role(cx, cy)?;
    Some(match role {
        Role::Outline => {
            let lit = lights.moon.is_some_and(|moon| {
                if moon.brightness < 0.5 {
                    return false;
                }
                // A three-way sign: f64::signum(0.0) is 1.0, which would
                // light the wrong side when the moon is straight above.
                let sign = |v: f64| {
                    if v > 0.5 {
                        1
                    } else if v < -0.5 {
                        -1
                    } else {
                        0
                    }
                };
                let step = (sign(moon.x - screen.0), sign(moon.row - screen.1));
                [(step.0, 0), (0, step.1)]
                    .iter()
                    .any(|&(dx, dy)| (dx, dy) != (0, 0) && !cloud.covers(cx + dx, cy + dy))
            });
            if lit {
                3
            } else {
                0
            }
        }
        Role::Shadow | Role::Crease => 1,
        Role::Body => 2,
        Role::Highlight => 3,
    })
}

/// How lit a far-cloud cell is, 0 to 1, before quantizing: its front puff
/// treated as a sphere, lit from the moon's direction (Lambert), plus a
/// little sky light on upward-facing surfaces. Cells in no puff are dim.
/// O(1).
pub fn far_light(cloud: &Cloud, cx: i32, cy: i32, screen: (f64, f64), lights: &CloudLights) -> f64 {
    let front = cloud.front_at(cx, cy);
    if front < 0 {
        return 0.1;
    }
    let b = cloud.bubbles[front as usize];
    let (dx, dy) = (
        (f64::from(cx) + 0.5 - b.cx) / b.r,
        (f64::from(cy) + 0.5 - b.cy) / b.r,
    );
    let nz = (1.0 - dx * dx - dy * dy).max(0.0).sqrt();
    // A horizon bank is a dark lumpy wall, lit only along its upper rims.
    let bank = cloud.kind == CloudKind::Bank;
    if bank {
        let rim = (-dy - 0.3).max(0.0) / 0.7;
        let mut light = 0.08 + 0.15 * rim;
        if let Some(moon) = lights.moon {
            light += 0.35 * moon.brightness * rim;
        }
        return light.clamp(0.0, 1.0);
    }
    let mut light = 0.15 + 0.2 * (-dy).max(0.0);
    if let Some(moon) = lights.moon {
        let (lx, ly) = (moon.x - screen.0, moon.row - screen.1);
        let length = (lx * lx + ly * ly).sqrt().max(1e-9);
        // Mostly from the moon's direction, partly from in front.
        let (lx, ly, lz) = (0.8 * lx / length, 0.8 * ly / length, 0.6);
        let lambert = (dx * lx + dy * ly + nz * lz).max(0.0);
        light += 0.7 * moon.brightness * lambert;
    }
    // Puffs low in the cloud are shaded by those above.
    let depth = f64::from(cy) / f64::from(cloud.height.max(1));
    (light * (1.0 - 0.35 * depth)).clamp(0.0, 1.0)
}

/// Snaps a light level to a tone index.
pub fn tone_index(light: f64) -> usize {
    ((light.clamp(0.0, 1.0) * TONES as f64) as usize).min(TONES - 1)
}

/// The tone ramp for a layer, preset, and palette: `TONES` colors between
/// the palette's sky and star colors.
pub fn tone_ramp(depth: Depth, cover: Cover, palette: &Palette) -> [Rgb; TONES] {
    let spec = cover.spec();
    let ts = match depth {
        Depth::Near => spec.near_tones,
        Depth::Far => spec.far_tones,
    };
    ts.map(|t| mix(palette.bg, palette.star, t))
}

/// One cloud layer, and the view onto it.
#[derive(Debug, Clone)]
pub struct CloudField {
    rng: Mulberry32,
    depth: Depth,
    cover: Cover,
    cols: i32,
    /// Rows of sky clouds may use (the ground row).
    sky_rows: i32,
    /// World column at the screen's left edge.
    view: f64,
    /// Wind travel so far, in cells.
    wind: f64,
    /// The clouds, in drawing order.
    pub clouds: Vec<Cloud>,
}

impl CloudField {
    /// The near layer: outlined cumulus for `cover`.
    pub fn new(scene_seed: u32, grid: &Grid, cover: Cover) -> Self {
        Self::layer(scene_seed, grid, cover, Depth::Near)
    }

    /// The far layer: towers and a horizon bank for `cover`.
    pub fn far(scene_seed: u32, grid: &Grid, cover: Cover) -> Self {
        Self::layer(scene_seed, grid, cover, Depth::Far)
    }

    fn layer(scene_seed: u32, grid: &Grid, cover: Cover, depth: Depth) -> Self {
        let seed = match depth {
            Depth::Near => cloud_seed(scene_seed),
            Depth::Far => cloud_seed(scene_seed) ^ FAR_SALT,
        };
        let mut field = Self {
            rng: Mulberry32::new(seed),
            depth,
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

    /// The layer.
    pub fn depth(&self) -> Depth {
        self.depth
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

    /// World column at the screen's left edge.
    pub fn view(&self) -> f64 {
        self.view
    }

    /// One new cloud whose left edge is at `x` (or, for a bank, a stretch
    /// of horizon starting there). `index` alternates far towers and banks.
    fn spawn(&mut self, x: i32, index: usize) -> Cloud {
        let spec = self.cover.spec();
        let rng = &mut self.rng;
        let sky = f64::from(self.sky_rows);
        match self.depth {
            Depth::Near => {
                let width = rng.rand_int(spec.width.0, spec.width.1) as i32;
                let (height, bubbles, mut front) = make_cumulus(rng, width);
                let height = trim_top(&mut front, width, height);
                let lo = (sky * spec.band.0) as i64;
                let hi = ((sky * spec.band.1) as i64).max(lo);
                let top = (rng.rand_int(lo, hi) as i32)
                    .min(self.sky_rows - height)
                    .max(0);
                Cloud {
                    kind: CloudKind::Cumulus,
                    x,
                    top,
                    width,
                    height,
                    bubbles,
                    front,
                    opacity: spec.opacity.min(MAX_OPACITY),
                }
            }
            Depth::Far => {
                let towers = self.cover.far_count(self.cols)
                    - if spec.bank_height > 0.0 {
                        (self.cols.max(1) as usize).div_ceil(240)
                    } else {
                        0
                    };
                let bank = index >= towers;
                let (kind, width, height) = if bank {
                    let h = ((sky * spec.bank_height).round() as i32).max(4);
                    (CloudKind::Bank, rng.rand_int(240, 320) as i32, h)
                } else {
                    let (lo, hi) = spec.tower_height;
                    let h = ((sky * (lo + (hi - lo) * rng.next_f64())).round() as i32).max(8);
                    let w = ((f64::from(h) * (1.4 + 1.2 * rng.next_f64())).round() as i32).max(16);
                    (CloudKind::Tower, w, h)
                };
                let (bubbles, mut front) = if bank {
                    make_bank(rng, width, height)
                } else {
                    make_tower(rng, width, height)
                };
                let height = trim_top(&mut front, width, height);
                Cloud {
                    kind,
                    x,
                    // Far clouds stand on the horizon, behind the city.
                    top: self.sky_rows - height,
                    width,
                    height,
                    bubbles,
                    front,
                    opacity: (spec.opacity * 0.95).min(MAX_OPACITY),
                }
            }
        }
    }

    /// Spreads the preset's clouds over the wrap span, each centered at a
    /// random point within its share; far banks tile the horizon.
    /// O(clouds × cloud cells).
    fn populate(&mut self) {
        let count = match self.depth {
            Depth::Near => self.cover.cloud_count(self.cols),
            Depth::Far => self.cover.far_count(self.cols),
        };
        let span = f64::from(self.span_width());
        let left = self.view.floor() as i32 - WRAP_MARGIN;
        let mut clouds = Vec::with_capacity(count);
        let mut bank_x = left;
        for index in 0..count {
            let share = span / count as f64;
            let x = left + (share * (index as f64 + self.rng.next_f64())) as i32;
            let mut cloud = self.spawn(x, index);
            if cloud.kind == CloudKind::Bank {
                cloud.x = bank_x;
                bank_x += cloud.width - 20;
            } else {
                cloud.x -= cloud.width / 2;
            }
            clouds.push(cloud);
        }
        // Towers behind banks; near clouds higher first, so lower ones draw
        // over them.
        clouds.sort_by_key(|cloud| (cloud.kind == CloudKind::Bank, cloud.top));
        self.clouds = clouds;
    }

    /// Points the view at the Endless root camera (0 in Normal mode).
    pub fn set_camera(&mut self, camera: f64) {
        if camera.is_finite() {
            self.view = self.wind + self.depth.parallax() * camera;
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
        self.wind += self.depth.wind() * speed.max(0.0) * step;
        self.set_camera(camera);
        let left = self.view.floor() as i32 - WRAP_MARGIN;
        let span = self.span_width();
        let mut replaced = false;
        for index in 0..self.clouds.len() {
            let cloud = &self.clouds[index];
            if cloud.x + cloud.width < left {
                let kind = cloud.kind;
                // A bank wraps by its own width plus the span, so the
                // horizon stays tiled.
                let x = if kind == CloudKind::Bank {
                    let banks: i32 = self
                        .clouds
                        .iter()
                        .filter(|c| c.kind == CloudKind::Bank)
                        .map(|c| c.width - 20)
                        .sum();
                    cloud.x + banks.max(span)
                } else {
                    cloud.x + span
                };
                let towers = self
                    .clouds
                    .iter()
                    .filter(|c| c.kind != CloudKind::Bank)
                    .count();
                let slot = if kind == CloudKind::Bank { towers } else { 0 };
                let mut fresh = self.spawn(x, slot);
                fresh.x = x;
                self.clouds[index] = fresh;
                replaced = true;
            }
        }
        replaced || self.view.floor() != before
    }

    /// The strip of the layer's world to draw: two columns either side of
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

    /// Draws the layer into an RGBA strip (see [`CloudField::strip`]), lit
    /// by `lights`, in four palette tones, every cell translucent (at most
    /// [`MAX_OPACITY`], overlaps at most [`MAX_STACKED_OPACITY`]), sky
    /// transparent. Clouds composite over each other in drawing order.
    /// O(strip cells + cloud cells × lobes).
    pub fn draw(&self, palette: &Palette, lights: &CloudLights) -> Vec<u8> {
        let span = self.strip();
        let mut rgba = vec![0u8; span.cells() * 4];
        let ramp = tone_ramp(self.depth, self.cover, palette);
        let view = self.view;
        for cloud in &self.clouds {
            for cy in 0..cloud.height {
                for cx in 0..cloud.width {
                    if !cloud.covers(cx, cy) {
                        continue;
                    }
                    let (wx, row) = (cloud.x + cx, cloud.top + cy);
                    let Some(cell) = span.index(wx, row) else {
                        continue;
                    };
                    let screen = (f64::from(wx) - view + 0.5, f64::from(row) + 0.5);
                    let tone = match self.depth {
                        Depth::Near => near_tone(cloud, cx, cy, screen, lights).unwrap_or(2),
                        Depth::Far => tone_index(far_light(cloud, cx, cy, screen, lights)),
                    };
                    let color = ramp[tone];
                    let alpha = cloud.opacity;
                    let pixel = &mut rgba[cell * 4..cell * 4 + 4];
                    let below = f64::from(pixel[3]) / 255.0;
                    let total = alpha + below * (1.0 - alpha);
                    for (channel, value) in pixel[..3].iter_mut().zip(color.0) {
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
    /// this layer covers. For tuning and tests. O(screen sky cells).
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
        assert_eq!(Cover::Scattered.cloud_count(640), 5);
        assert_eq!(Cover::Scattered.cloud_count(200), 3);
        assert_eq!(Cover::Clear.cloud_count(640), 0);
        assert_eq!(Cover::Clear.far_count(640), 0);
        assert!(Cover::Storm.far_count(640) > Cover::Scattered.far_count(640));
    }

    fn widest_row(cloud: &Cloud) -> i32 {
        (0..cloud.height)
            .map(|y| (0..cloud.width).filter(|&x| cloud.covers(x, y)).count() as i32)
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn cumulus_have_flat_bottoms_lobed_tops_and_reference_proportions() {
        let mut rng = Mulberry32::new(4);
        for _ in 0..200 {
            let width = rng.rand_int(14, 180) as i32;
            let (height, bubbles, mut front) = make_cumulus(&mut rng, width);
            let height = trim_top(&mut front, width, height);
            let cloud = Cloud {
                kind: CloudKind::Cumulus,
                x: 0,
                top: 0,
                width,
                height,
                bubbles,
                front,
                opacity: 0.8,
            };
            // Wider than tall, like the reference (about 3 to 4 to 1).
            assert!(cloud.width >= 2 * cloud.height, "{width} x {height}");
            // The bottom row is the widest or nearly so: a flat base.
            let bottom = (0..width).filter(|&x| cloud.covers(x, height - 1)).count() as i32;
            assert!(bottom * 10 >= widest_row(&cloud) * 6, "flat base");
            // The top is bumpy: the outline's top row is not the full width.
            let top = (0..width).filter(|&x| cloud.covers(x, 0)).count() as i32;
            assert!(top < bottom, "lobed top narrower than the base");
        }
    }

    #[test]
    fn near_clouds_have_an_outline_shadow_body_and_highlight() {
        let mut rng = Mulberry32::new(5);
        let (height, bubbles, mut front) = make_cumulus(&mut rng, 70);
        let height = trim_top(&mut front, 70, height);
        let cloud = Cloud {
            kind: CloudKind::Cumulus,
            x: 0,
            top: 0,
            width: 70,
            height,
            bubbles,
            front,
            opacity: 0.8,
        };
        let mut seen = std::collections::HashSet::new();
        for cy in 0..height {
            for cx in 0..70 {
                if let Some(role) = cloud.role(cx, cy) {
                    seen.insert(format!("{role:?}"));
                    if cloud.is_edge(cx, cy) {
                        assert_eq!(role, Role::Outline);
                    }
                    if role == Role::Shadow {
                        assert!(cy >= height / 2, "shadow is along the bottom");
                    }
                } else {
                    assert!(!cloud.covers(cx, cy));
                }
            }
        }
        for role in ["Outline", "Shadow", "Body", "Highlight"] {
            assert!(seen.contains(role), "{role} missing: {seen:?}");
        }
    }

    #[test]
    fn creases_appear_where_lobes_overlap() {
        let mut rng = Mulberry32::new(6);
        let mut creases = 0;
        for _ in 0..20 {
            let (height, bubbles, mut front) = make_cumulus(&mut rng, 90);
            let height = trim_top(&mut front, 90, height);
            let cloud = Cloud {
                kind: CloudKind::Cumulus,
                x: 0,
                top: 0,
                width: 90,
                height,
                bubbles,
                front,
                opacity: 0.8,
            };
            for cy in 0..height {
                for cx in 0..90 {
                    creases += usize::from(cloud.role(cx, cy) == Some(Role::Crease));
                }
            }
        }
        assert!(creases > 0);
    }

    #[test]
    fn far_clouds_stand_on_the_horizon_and_have_no_outline() {
        let grid = grid();
        let field = CloudField::far(3, &grid, Cover::Storm);
        assert!(!field.clouds.is_empty());
        assert!(field.clouds.iter().any(|c| c.kind == CloudKind::Bank));
        assert!(field.clouds.iter().any(|c| c.kind == CloudKind::Tower));
        for cloud in &field.clouds {
            assert_eq!(cloud.top + cloud.height, grid.ground_row, "on the horizon");
        }
        // Lit by the moon from the upper left, a far cloud's left side is
        // brighter than its right: shading, not an outline.
        let tower = field
            .clouds
            .iter()
            .find(|c| c.kind == CloudKind::Tower)
            .unwrap();
        let lights = CloudLights {
            moon: Some(MoonLight::for_phase(-500.0, -200.0, 19.5, 0.5)),
        };
        let row = tower.height / 2;
        let cols: Vec<i32> = (0..tower.width).filter(|&x| tower.covers(x, row)).collect();
        let at = |x: i32| far_light(tower, x, row, (f64::from(x), f64::from(row)), &lights);
        let left: f64 = cols.iter().take(4).map(|&x| at(x)).sum();
        let right: f64 = cols.iter().rev().take(4).map(|&x| at(x)).sum();
        assert!(left > right, "lit side faces the moon: {left} vs {right}");
    }

    #[test]
    fn a_full_moon_lights_far_clouds_more_than_a_new_moon() {
        let grid = grid();
        let field = CloudField::far(4, &grid, Cover::Broken);
        let tower = &field.clouds[0];
        let total = |phase: f64| -> f64 {
            let lights = CloudLights {
                moon: Some(MoonLight::for_phase(100.0, 40.0, 19.5, phase)),
            };
            (0..tower.height)
                .flat_map(|y| (0..tower.width).map(move |x| (x, y)))
                .filter(|&(x, y)| tower.covers(x, y))
                .map(|(x, y)| far_light(tower, x, y, (f64::from(x), f64::from(y)), &lights))
                .sum()
        };
        assert!(total(0.5) > total(0.0));
    }

    #[test]
    fn coverage_grows_from_clear_to_storm_in_both_layers() {
        let grid = grid();
        for far in [false, true] {
            let mut previous = -1.0;
            for cover in COVERS {
                let mean: f64 = (0..4)
                    .map(|seed| {
                        if far {
                            CloudField::far(seed, &grid, cover).coverage()
                        } else {
                            CloudField::new(seed, &grid, cover).coverage()
                        }
                    })
                    .sum::<f64>()
                    / 4.0;
                if cover != Cover::Puffs || !far {
                    assert!(mean >= previous, "{far} {cover:?}: {mean} after {previous}");
                }
                previous = mean;
            }
        }
        let near = CloudField::new(1, &grid, Cover::Storm).coverage();
        let far = CloudField::far(1, &grid, Cover::Storm).coverage();
        assert!(near + far > 0.5, "storms are heavy: {near} + {far}");
    }

    #[test]
    fn near_clouds_stay_in_the_sky_above_the_ground_row() {
        let grid = grid();
        for cover in COVERS {
            for cloud in &CloudField::new(7, &grid, cover).clouds {
                assert!(cloud.top >= 0 && cloud.top + cloud.height <= grid.ground_row);
            }
        }
    }

    #[test]
    fn every_cloud_lets_light_through() {
        let grid = grid();
        for cover in COVERS {
            for field in [
                CloudField::new(2, &grid, cover),
                CloudField::far(2, &grid, cover),
            ] {
                for cloud in &field.clouds {
                    assert!(cloud.opacity <= MAX_OPACITY);
                }
                let rgba = field.draw(classic(), &CloudLights::default());
                let most = rgba.chunks(4).map(|p| p[3]).max().unwrap_or(0);
                assert!(f64::from(most) <= MAX_STACKED_OPACITY * 255.0 + 0.5);
            }
        }
    }

    #[test]
    fn wind_and_parallax_differ_by_depth() {
        let grid = grid();
        let mut near = CloudField::new(2, &grid, Cover::Scattered);
        let mut far = CloudField::far(2, &grid, Cover::Scattered);
        for _ in 0..300 {
            near.advance(1.0 / 30.0, 1.0, 0.0);
            far.advance(1.0 / 30.0, 1.0, 0.0);
        }
        assert!(
            (near.view() - 3.5).abs() < 1e-9,
            "10 s at the spec's 0.35 cells/s"
        );
        assert!((far.view() - 1.2).abs() < 1e-9);
        near.set_camera(100.0);
        far.set_camera(100.0);
        assert!((near.view() - (3.5 + 10.0)).abs() < 1e-9);
        assert!((far.view() - (1.2 + 4.0)).abs() < 1e-9);
    }

    #[test]
    fn clouds_wrap_with_new_shapes_and_the_count_is_kept() {
        let grid = grid();
        for mut field in [
            CloudField::new(3, &grid, Cover::Broken),
            CloudField::far(3, &grid, Cover::Overcast),
        ] {
            let count = field.clouds.len();
            let first = field.clouds.clone();
            for frame in 0..(3600 * 30) {
                let camera = 8.0 * f64::from(frame) / 30.0;
                field.advance(1.0 / 30.0, 4.0, camera);
            }
            assert_eq!(field.clouds.len(), count);
            assert_ne!(field.clouds, first, "shapes were renewed");
            let left = field.view().floor() as i32 - WRAP_MARGIN;
            for cloud in &field.clouds {
                assert!(cloud.x + cloud.width >= left, "nothing left behind");
            }
        }
    }

    #[test]
    fn same_seed_gives_the_same_sky() {
        let grid = grid();
        assert_eq!(
            CloudField::new(9, &grid, Cover::Overcast).clouds,
            CloudField::new(9, &grid, Cover::Overcast).clouds
        );
        assert_ne!(
            CloudField::far(9, &grid, Cover::Overcast).clouds,
            CloudField::far(10, &grid, Cover::Overcast).clouds
        );
    }

    #[test]
    fn drawn_clouds_use_their_layer_tones_and_leave_open_sky_clear() {
        let grid = grid();
        for palette in palettes() {
            for field in [
                CloudField::new(5, &grid, Cover::Broken),
                CloudField::far(5, &grid, Cover::Broken),
            ] {
                let rgba = field.draw(palette, &CloudLights::default());
                assert_eq!(rgba.len(), field.strip().cells() * 4);
                let ramp = tone_ramp(field.depth(), Cover::Broken, palette);
                let drawn: Vec<[u8; 3]> = rgba
                    .chunks(4)
                    .filter(|p| p[3] > 0)
                    .map(|p| [p[0], p[1], p[2]])
                    .collect();
                assert!(!drawn.is_empty(), "{}", palette.key);
                let on_ramp = drawn
                    .iter()
                    .filter(|c| ramp.iter().any(|t| t.0 == **c))
                    .count();
                assert!(on_ramp * 10 > drawn.len() * 7, "{}", palette.key);
                assert!(rgba.chunks(4).any(|p| p[3] == 0), "open sky stays clear");
            }
        }
    }

    #[test]
    fn storm_tones_are_darker_than_fair_weather() {
        let luminance = |c: Rgb| c.0.iter().map(|&v| u32::from(v)).sum::<u32>();
        for depth in [Depth::Near, Depth::Far] {
            let storm = tone_ramp(depth, Cover::Storm, classic());
            let fair = tone_ramp(depth, Cover::Scattered, classic());
            assert!(luminance(storm[TONES - 1]) < luminance(fair[TONES - 1]));
        }
        assert!(tone_index(0.0) == 0 && tone_index(1.0) == TONES - 1);
    }

    #[test]
    fn the_glow_is_smooth_scales_with_phase_and_stays_near_the_moon() {
        let full = MoonLight::for_phase(100.0, 50.0, 19.5, 0.5);
        let new = MoonLight::for_phase(100.0, 50.0, 19.5, 0.0);
        assert_eq!(moon_glow(&new, 100.0, 50.0), 0.0);
        assert!((moon_glow(&full, 100.0, 50.0) - MOON_GLOW_STRENGTH).abs() < 1e-12);
        let mut previous = MOON_GLOW_STRENGTH;
        for step in 1..=60 {
            let glow = moon_glow(&full, 100.0 + f64::from(step), 50.0);
            assert!(glow <= previous && previous - glow < 0.05);
            previous = glow;
        }
        assert_eq!(
            moon_glow(&full, 100.0 + MOON_GLOW_REACH * 19.5 + 1.0, 50.0),
            0.0
        );
    }

    #[test]
    fn a_crescent_glows_from_its_lit_limb() {
        let waxing = MoonLight::for_phase(100.0, 50.0, 19.5, 0.125);
        let waning = MoonLight::for_phase(100.0, 50.0, 19.5, 0.875);
        assert!(waxing.glow_center().0 > 100.0);
        assert!(waning.glow_center().0 < 100.0);
        assert_eq!(
            MoonLight::for_phase(100.0, 50.0, 19.5, 0.5).glow_center(),
            (100.0, 50.0)
        );
    }

    #[test]
    fn outlines_facing_a_bright_moon_catch_a_silver_lining() {
        let mut rng = Mulberry32::new(8);
        let (height, bubbles, mut front) = make_cumulus(&mut rng, 60);
        let height = trim_top(&mut front, 60, height);
        let cloud = Cloud {
            kind: CloudKind::Cumulus,
            x: 0,
            top: 0,
            width: 60,
            height,
            bubbles,
            front,
            opacity: 0.8,
        };
        let cx = (0..60).find(|&x| cloud.covers(x, 0)).unwrap();
        let above = CloudLights {
            moon: Some(MoonLight::for_phase(f64::from(cx), -40.0, 19.5, 0.5)),
        };
        let below = CloudLights {
            moon: Some(MoonLight::for_phase(f64::from(cx), 400.0, 19.5, 0.5)),
        };
        let screen = (f64::from(cx) + 0.5, 0.5);
        assert_eq!(near_tone(&cloud, cx, 0, screen, &above), Some(3));
        assert_eq!(near_tone(&cloud, cx, 0, screen, &below), Some(0));
    }

    #[test]
    fn the_strip_follows_the_view_with_a_fractional_offset() {
        let grid = grid();
        let mut field = CloudField::new(6, &grid, Cover::Scattered);
        field.advance(1.0 / 30.0, 1.0, 37.3);
        let strip = field.strip();
        assert_eq!(strip.origin_x, field.view().floor() as i32 - STRIP_MARGIN);
        let offset = field.strip_offset_px(grid.cell);
        let cell = f64::from(grid.cell);
        assert!(offset >= 2.0 * cell && offset < 3.0 * cell, "{offset}");
    }
}
