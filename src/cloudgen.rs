//! Procedural clouds (owner decision, 2026-11-04: "go procedural on them,
//! just like how we build the city"), following GitHub issue #1 (filed from
//! the Python version): "procedural cloud coverage with moonlight
//! transmission".
//!
//! **A continuous density field, not sprites.** Each sky layer is a seeded
//! field over world columns and rows: two low-frequency noise layers (a
//! broad one that groups clouds into fields and gaps, and billowy detail)
//! combined and shaped by the layer's vertical profile. Clouds are wherever
//! the field passes a threshold set by **coverage** (0 clear to 1 full), so
//! changing coverage grows or shrinks clouds smoothly: presets blend without
//! popping, and nothing repeats. **Thickness** (optical depth) is separate
//! from coverage: how opaque the clouds are, not how many.
//!
//! **Transmission.** The moon and stars are behind the clouds. A cell lets
//! `T = exp(-thickness × density)` of the light behind it through, so thin
//! cloud shows the moon and dense cloud hides it. Opacity `1 - T` is capped
//! at [`MAX_OPACITY`]: even a storm lets a faint glow through (the owner had
//! asked for thick clouds to stay slightly transparent; the issue asks that
//! dense cloud obscure the moon; 0.92 is the compromise).
//!
//! **Light.** Self-shadowing: each cell marches toward the light through the
//! field and is dimmed by `exp(-k × density passed)`, so cores are dark and
//! thin edges facing the moon catch light. Edge lighting near the moon is
//! restrained and comes only through thin cloud, never as a halo through
//! opaque cloud.
//!
//! **Pixel-art finish.** Light snaps to [`TONES`] palette tones; thin
//! edges are dithered with a fixed 4x4 Bayer pattern. No outlines (owner
//! decision).
//!
//! **Layers** are like the city's: high (thin, streaky sheets), mid (heaped
//! cumulus with flat bases), and low (dark scud). Together they fill the
//! sky to the horizon (owner, 2026-11-04) with perspective: cumulus come in
//! rows that grow smaller, flatter, and closer together toward the horizon,
//! and sheet noise shrinks the same way. A
//! [`crate::clouds::Cover`] preset sets each layer's coverage and thickness.
//!
//! Pure (no graphics): the preview binary (`cargo run --bin clouds`) and
//! tests use it directly. Not yet wired into the app; the owner reviews it
//! in the preview first.

use crate::clouds::Cover;
use crate::model::palette::{Palette, Rgb};
use crate::raster::mix;

/// Light tones a cloud is drawn in, darkest first.
pub const TONES: usize = 5;
/// The most opaque a cloud cell can be.
pub const MAX_OPACITY: f64 = 0.92;
/// Columns between cumulus slots overhead: about one heap per slot at full
/// cover. Slots shrink with distance toward the horizon.
pub const HEAP_SPACING: f64 = 110.0;
/// Rows of cumulus from overhead to the horizon.
pub const HEAP_DEPTHS: i32 = 10;

/// Perspective scale at fraction `v` down the sky (0 top, 1 horizon):
/// features are full size overhead and a third of it at the horizon.
pub fn perspective(v: f64) -> f64 {
    1.0 - 0.68 * v.clamp(0.0, 1.0)
}
/// Opacity below which a cell is an edge, dithered rather than solid.
pub const EDGE_ALPHA: f64 = 0.3;

/// A 32-bit integer hash of a lattice point, in `0..1`. Stateless and
/// seeded, so the field is the same wherever and whenever it is sampled.
fn hash(ix: i32, iy: i32, seed: u32) -> f64 {
    let mut h = (ix as u32).wrapping_mul(0x27d4_eb2d)
        ^ (iy as u32).wrapping_mul(0x1656_67b1)
        ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    f64::from(h) / f64::from(u32::MAX)
}

fn smooth(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    smooth(((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0))
}

/// Value noise at `(x, y)`: lattice hashes blended with smoothstep, in
/// `0..1`. O(1).
pub fn value_noise(x: f64, y: f64, seed: u32) -> f64 {
    let (x0, y0) = (x.floor(), y.floor());
    let (tx, ty) = (smooth(x - x0), smooth(y - y0));
    let (ix, iy) = (x0 as i32, y0 as i32);
    let a = hash(ix, iy, seed);
    let b = hash(ix + 1, iy, seed);
    let c = hash(ix, iy + 1, seed);
    let d = hash(ix + 1, iy + 1, seed);
    let top = a + (b - a) * tx;
    let bottom = c + (d - c) * tx;
    top + (bottom - top) * ty
}

/// Fractal noise: `octaves` of value noise, each twice the frequency and
/// half the amplitude of the last, normalized to `0..1`. With `billow`,
/// each octave is folded and inverted (`1 - |2n - 1|`), giving puffy,
/// rounded blobs. O(octaves).
pub fn fractal(x: f64, y: f64, seed: u32, octaves: u32, billow: bool) -> f64 {
    let (mut total, mut amplitude, mut frequency, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for octave in 0..octaves {
        let mut n = value_noise(
            x * frequency,
            y * frequency,
            seed.wrapping_add(octave * 7919),
        );
        if billow {
            n = 1.0 - (2.0 * n - 1.0).abs();
        }
        total += n * amplitude;
        norm += amplitude;
        amplitude *= 0.5;
        frequency *= 2.0;
    }
    total / norm
}

/// The sky's cloud layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// High, thin, streaky sheets.
    High,
    /// Cumulus with flat bases, in rows from overhead to the horizon.
    Mid,
    /// Low, dark scud.
    Low,
}

/// Every layer, back (high) to front (low).
pub const LAYERS: [Layer; 3] = [Layer::High, Layer::Mid, Layer::Low];

impl Layer {
    /// The rows the layer occupies, as fractions of the sky's rows
    /// (top, base).
    pub fn band(self) -> (f64, f64) {
        match self {
            Layer::High => (0.0, 0.45),
            Layer::Mid => (0.0, 1.0),
            Layer::Low => (0.55, 1.0),
        }
    }

    /// Noise feature size in cells (horizontal, vertical) for the detail
    /// noise; the broad grouping noise is six times wider.
    fn scale(self) -> (f64, f64) {
        match self {
            Layer::High => (40.0, 3.5),
            Layer::Mid => (11.0, 8.0),
            Layer::Low => (26.0, 5.0),
        }
    }

    /// Puffy (billow) detail for heaps; smooth streaks for sheets.
    fn billow(self) -> bool {
        self == Layer::Mid
    }

    /// Ambient light: high clouds catch more sky light; low scud is dark.
    pub fn ambient(self) -> f64 {
        match self {
            Layer::High => 0.34,
            Layer::Mid => 0.24,
            Layer::Low => 0.12,
        }
    }

    fn salt(self) -> u32 {
        match self {
            Layer::High => 0x1111_0001,
            Layer::Mid => 0x2222_0002,
            Layer::Low => 0x3333_0003,
        }
    }

    /// The vertical profile at `v` (0 top of band, 1 base): how much the
    /// layer allows cloud there. Heaps have a sharp, flat base and rounded
    /// tops whose height `heap` (0..1) varies along the sky; sheets are
    /// thickest in the middle of their band.
    fn profile(self, v: f64, heap: f64) -> f64 {
        match self {
            Layer::Mid => {
                // A sharp base: fully allowed until about two rows above it.
                let base = smoothstep(1.0, 0.98, v);
                // A soft ceiling, so the billowy detail shapes the tops
                // into rounded lumps instead of a smooth ridge line.
                let top = 1.0 - heap;
                base * smoothstep(top - 0.3, top + 0.45, v)
            }
            Layer::High => {
                // Thickest near the top, dissolving toward its lower edge so
                // there is no ruler-straight line where it ends.
                smoothstep(1.0, 0.25, v) * smoothstep(0.0, 0.1, v)
            }
            Layer::Low => {
                // Dissolving upward from the horizon.
                smoothstep(0.0, 0.75, v)
            }
        }
    }
}

/// How much and how thick one layer is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerWeather {
    /// Share of the layer's sky that has cloud, 0 to 1.
    pub coverage: f64,
    /// Optical depth: opacity is `1 - exp(-thickness × density)`.
    pub thickness: f64,
}

/// The weather for each layer, high to low, for a preset. Coverage and
/// thickness are separate dials, so a preset can be many thin clouds or a
/// few dense ones; values between presets blend smoothly.
pub fn weather(cover: Cover) -> [LayerWeather; 3] {
    let w = |coverage, thickness| LayerWeather {
        coverage,
        thickness,
    };
    match cover {
        Cover::Clear => [w(0.0, 1.0), w(0.0, 1.5), w(0.0, 2.0)],
        Cover::Puffs => [w(0.0, 1.0), w(0.18, 1.6), w(0.0, 2.0)],
        Cover::Scattered => [w(0.2, 0.8), w(0.32, 1.8), w(0.0, 2.0)],
        Cover::Broken => [w(0.35, 1.0), w(0.55, 2.2), w(0.2, 2.2)],
        Cover::Overcast => [w(0.75, 1.4), w(0.7, 2.6), w(0.45, 2.6)],
        Cover::Storm => [w(1.0, 2.4), w(1.0, 3.4), w(0.95, 3.4)],
    }
}

/// One layer's field for a sky.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerField {
    pub layer: Layer,
    pub weather: LayerWeather,
    seed: u32,
    /// Rows of sky (the ground row).
    sky_rows: f64,
}

impl LayerField {
    /// The field for `layer` in a sky of `sky_rows`, from the scene seed.
    pub fn new(layer: Layer, weather: LayerWeather, sky_rows: i32, seed: u32) -> Self {
        Self {
            layer,
            weather,
            seed: seed ^ layer.salt(),
            sky_rows: f64::from(sky_rows.max(1)),
        }
    }

    /// Cloud density at world column `x` and row `row` (fractional, cell
    /// centers at `+0.5`), in `0..1`. 0 outside the layer's band or where
    /// the field is below the coverage threshold. O(octaves).
    pub fn density(&self, x: f64, row: f64) -> f64 {
        let coverage = self.weather.coverage;
        if coverage <= 0.0 {
            return 0.0;
        }
        let (top, base) = self.layer.band();
        let v = (row / self.sky_rows - top) / (base - top);
        if !(0.0..=1.0).contains(&v) {
            return 0.0;
        }
        if self.layer == Layer::Mid {
            return self.heap_density(x, row, v);
        }
        let (sx, sy) = self.layer.scale();
        // Broad grouping: where the sky has cloud fields and where gaps.
        // Perspective: features shrink toward the horizon.
        let p = perspective(row / self.sky_rows);
        let (sx, sy) = (sx * p, sy * p);
        let broad = fractal(x / (sx * 6.0), 0.37, self.seed ^ 0xb0b0, 3, false);
        // Heap height along the sky (mid layer), also broad.
        let heap = fractal(x / (sx * 3.0), 1.71, self.seed ^ 0x4ea9, 2, false);
        let detail = fractal(x / sx, row / sy, self.seed, 4, self.layer.billow());
        // The broad noise decides where cloud fields are (clear gaps between
        // them until coverage is high); the detail shapes each cloud.
        let group = smoothstep(0.2, 0.7, broad);
        let field = (0.35 * group + 0.65 * detail) * self.layer.profile(v, 0.35 + 0.65 * heap);
        // Coverage sets the threshold: more coverage, lower threshold.
        let threshold = 0.62 - 0.5 * coverage;
        smoothstep(threshold, threshold + 0.16, field)
    }

    /// Mid-layer density: separate cumulus heaps in seeded slots along the
    /// world (like roadside trees), so heaps stand apart with clear sky
    /// between them. Each slot has its own threshold; as coverage passes
    /// it, that slot's heap grows smoothly from nothing, so raising
    /// coverage adds and grows clouds without popping.
    ///
    /// A heap is a cluster of metaballs: soft balls whose densities add, so
    /// overlapping balls merge into one smooth, lumpy (cauliflower)
    /// silhouette. Big balls sit low in the middle, smaller ones stack
    /// around and above them, and a flat base cuts them off below. Billowy
    /// noise roughens only the edges; empty sky stays empty.
    /// O(slots × balls + octaves).
    fn heap_density(&self, x: f64, row: f64, _v: f64) -> f64 {
        // Rows of heaps from overhead (near, big) to the horizon (far,
        // small, flat, close together). The densest wins where rows overlap.
        let mut best: f64 = 0.0;
        for depth in 0..HEAP_DEPTHS {
            let t = f64::from(depth) / f64::from(HEAP_DEPTHS - 1);
            best = best.max(self.heap_row_density(x, row, depth, t));
        }
        best
    }

    /// One depth row of heaps: `t` is 0 overhead to 1 at the horizon.
    fn heap_row_density(&self, x: f64, row: f64, depth: i32, t: f64) -> f64 {
        let coverage = self.weather.coverage;
        let scale = 1.0 - 0.78 * t;
        let spacing = HEAP_SPACING * scale;
        // Bases step down toward the horizon.
        let base_v = 0.36 + 0.62 * t.powf(0.8);
        let salt = depth * 1000;
        let slot = (x / spacing).floor() as i32;
        let mut field: f64 = 0.0;
        for k in slot - 2..=slot + 2 {
            let r = |n: i32| hash(k, salt + n, self.seed);
            // Each heap grows over 0.3 of coverage past its own threshold.
            let grow = smoothstep(r(1), r(1) + 0.3, coverage * 1.15);
            if grow <= 0.0 {
                continue;
            }
            // Heaps get bigger as the sky fills; distant ones are flatter.
            let width = spacing * (0.6 + 1.1 * r(2)) * (0.4 + 0.6 * grow) * (0.8 + 0.5 * coverage);
            let height = width * (0.32 + 0.16 * r(3)) * (1.0 - 0.35 * t);
            let center = (f64::from(k) + 0.2 + 0.6 * r(4)) * spacing;
            if (x - center).abs() > width {
                continue;
            }
            // Jittered bases, so the rows blend into one recession.
            let base_row = self.sky_rows * (base_v + 0.1 * scale * (r(5) - 0.5)).min(0.995);
            // A hard cut, so the base is pixel-flat rather than a dithered,
            // ragged line.
            if row > base_row {
                continue;
            }
            let balls = 5 + (r(6) * 6.99) as i32;
            for ball in 0..balls {
                let h = |n: i32| hash(k, salt + 100 + ball * 8 + n, self.seed);
                // Spread across the heap, bunched toward the middle.
                let u = (h(0) + h(1)) - 1.0;
                let size = 1.0 - 0.55 * u.abs();
                let radius = height * 0.5 * size * (0.75 + 0.45 * h(2));
                let bx = center + u * width * 0.42;
                // Central balls stack higher; all rest near the base.
                let lift = radius * (0.35 + 0.9 * h(3) * (1.0 - u.abs()));
                let by = base_row - lift;
                let d2 = ((x - bx).powi(2) + (row - by).powi(2)) / (radius * radius);
                if d2 < 1.0 {
                    field += (1.0 - d2) * (1.0 - d2);
                }
            }
        }
        if field <= 0.0 {
            return 0.0;
        }
        let (sx, sy) = self.layer.scale();
        let detail = fractal(
            x / (sx * scale),
            row / (sy * scale),
            self.seed ^ salt as u32,
            4,
            true,
        );
        let rough = field + 0.45 * (detail - 0.5);
        smoothstep(0.3, 0.75, rough)
    }

    /// Opacity of a cell with `density`: `1 - exp(-thickness × density)`,
    /// at most [`MAX_OPACITY`].
    pub fn opacity(&self, density: f64) -> f64 {
        (1.0 - (-self.weather.thickness * density).exp()).min(MAX_OPACITY)
    }

    /// The share of light passing through a cell: `1 - opacity`.
    pub fn transmission(&self, density: f64) -> f64 {
        1.0 - self.opacity(density)
    }
}

/// The moon as a light on the clouds, in screen cells.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    pub x: f64,
    pub y: f64,
    /// Disk radius in cells.
    pub radius: f64,
    /// 0 (new moon) to 1 (full).
    pub strength: f64,
}

/// Steps marched toward the light for self-shadowing.
const MARCH_STEPS: i32 = 8;
/// Cells per march step.
const MARCH_STEP: f64 = 2.0;
/// How quickly density dims light passing through it.
const EXTINCTION: f64 = 0.5;
/// How far, in moon radii, its edge light reaches.
const EDGE_REACH: f64 = 4.0;

/// How lit the cloud cell at world `(x, row)` is, `0..1`.
///
/// Ambient light for the layer, a little more on thin cells; then the
/// light, dimmed by the density between the cell and the light along a
/// short march (`exp(-k × density passed)`), so cores are dark and the side
/// facing the moon is bright. Near the moon, thin cloud gets restrained
/// edge light, weighted by how thin it is, so opaque cloud never glows.
/// `screen_x` is the cell's screen column (the light is in screen space).
/// O(steps × octaves).
pub fn light_at(field: &LayerField, x: f64, row: f64, screen_x: f64, light: Option<Light>) -> f64 {
    let d = field.density(x, row);
    let mut level = field.layer.ambient() + 0.12 * (1.0 - d);
    if let Some(light) = light {
        let (dx, dy) = (light.x - screen_x, light.y - row);
        let length = (dx * dx + dy * dy).sqrt().max(1e-9);
        let (ux, uy) = (dx / length, dy / length);
        let mut passed = 0.0;
        for step in 1..=MARCH_STEPS {
            let t = MARCH_STEP * f64::from(step);
            passed += field.density(x + ux * t, row + uy * t);
        }
        level += 0.55 * light.strength * (-EXTINCTION * passed).exp();
        let near = (1.0 - length / (EDGE_REACH * light.radius)).max(0.0);
        level += 0.35 * light.strength * near * near * field.transmission(d);
    }
    level.clamp(0.0, 1.0)
}

/// The 4x4 Bayer matrix, as thresholds in `0..1`.
const BAYER: [[f64; 4]; 4] = [
    [0.0 / 16.0, 8.0 / 16.0, 2.0 / 16.0, 10.0 / 16.0],
    [12.0 / 16.0, 4.0 / 16.0, 14.0 / 16.0, 6.0 / 16.0],
    [3.0 / 16.0, 11.0 / 16.0, 1.0 / 16.0, 9.0 / 16.0],
    [15.0 / 16.0, 7.0 / 16.0, 13.0 / 16.0, 5.0 / 16.0],
];

/// The opacity actually drawn for a cell of `alpha` at screen cell
/// `(sx, sy)`: cells at or above [`EDGE_ALPHA`] keep their opacity; thinner
/// ones are dithered, drawn at [`EDGE_ALPHA`] in a fixed pattern whose
/// share follows their alpha, or not at all. The pattern is tied to the
/// screen, so it doesn't crawl as clouds drift. O(1).
pub fn dithered_alpha(alpha: f64, sx: i32, sy: i32) -> f64 {
    if alpha <= 0.0 {
        return 0.0;
    }
    if alpha >= EDGE_ALPHA {
        return alpha;
    }
    let threshold = BAYER[sy.rem_euclid(4) as usize][sx.rem_euclid(4) as usize];
    if alpha / EDGE_ALPHA > threshold {
        EDGE_ALPHA
    } else {
        0.0
    }
}

/// Snaps a light level to a tone index.
pub fn tone_index(level: f64) -> usize {
    ((level.clamp(0.0, 1.0) * TONES as f64) as usize).min(TONES - 1)
}

/// The tone ramp for a layer under a preset, darkest first: `TONES`
/// colors between the palette's sky and star colors. Low scud and stormy
/// skies are darker. O(TONES).
pub fn tone_ramp(layer: Layer, cover: Cover, palette: &Palette) -> [Rgb; TONES] {
    let (lo, hi) = match layer {
        Layer::High => (0.08, 0.5),
        Layer::Mid => (0.05, 0.55),
        Layer::Low => (0.03, 0.3),
    };
    let gloom = match cover {
        Cover::Overcast => 0.8,
        Cover::Storm => 0.6,
        _ => 1.0,
    };
    std::array::from_fn(|k| {
        let t = lo + (hi - lo) * k as f64 / (TONES - 1) as f64;
        mix(palette.bg, palette.star, t * gloom)
    })
}

/// The three layer fields for a preset, high to low.
pub fn sky_fields(cover: Cover, sky_rows: i32, seed: u32) -> [LayerField; 3] {
    let weather = weather(cover);
    [0, 1, 2].map(|i| LayerField::new(LAYERS[i], weather[i], sky_rows, seed))
}

/// The share of a `cols x sky_rows` window starting at world column `left`
/// where any layer has cloud. For tuning and tests. O(cells × layers).
pub fn coverage(fields: &[LayerField], left: f64, cols: i32, sky_rows: i32) -> f64 {
    let mut covered = 0usize;
    for row in 0..sky_rows {
        for col in 0..cols {
            let (x, y) = (left + f64::from(col) + 0.5, f64::from(row) + 0.5);
            if fields.iter().any(|f| f.density(x, y) > 0.0) {
                covered += 1;
            }
        }
    }
    covered as f64 / f64::from((cols * sky_rows).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clouds::COVERS;

    const ROWS: i32 = 300;

    #[test]
    fn noise_is_seeded_continuous_and_in_range() {
        let a = value_noise(3.3, 7.7, 1);
        assert_eq!(a, value_noise(3.3, 7.7, 1));
        assert_ne!(a, value_noise(3.3, 7.7, 2));
        let mut previous = value_noise(0.0, 0.0, 5);
        for step in 1..200 {
            let n = value_noise(f64::from(step) * 0.01, 0.0, 5);
            assert!((0.0..=1.0).contains(&n));
            assert!((n - previous).abs() < 0.05, "continuous");
            previous = n;
        }
        for billow in [false, true] {
            assert!((0.0..=1.0).contains(&fractal(12.5, 3.25, 9, 4, billow)));
        }
    }

    #[test]
    fn coverage_rises_with_the_preset_and_clear_is_empty() {
        let mut previous = -1.0;
        for cover in COVERS {
            let fields = sky_fields(cover, ROWS, 7);
            let share = coverage(&fields, 0.0, 400, ROWS);
            assert!(
                share > previous || cover == Cover::Clear,
                "{cover:?}: {share}"
            );
            previous = share;
        }
        assert_eq!(
            coverage(&sky_fields(Cover::Clear, ROWS, 7), 0.0, 400, ROWS),
            0.0
        );
    }

    #[test]
    fn coverage_changes_smoothly_without_popping() {
        // Sweeping the coverage dial, the covered share never jumps.
        let mut previous = 0.0;
        for step in 0..=40 {
            let w = LayerWeather {
                coverage: f64::from(step) / 40.0,
                thickness: 2.0,
            };
            let field = LayerField::new(Layer::Mid, w, ROWS, 3);
            let share = coverage(&[field], 0.0, 300, ROWS);
            assert!(share >= previous - 1e-9, "never shrinks as coverage grows");
            assert!(
                share - previous < 0.08,
                "no pop at {step}: {previous} to {share}"
            );
            previous = share;
        }
    }

    #[test]
    fn the_field_is_continuous_along_the_world() {
        // Clouds drift by moving the view: neighbouring columns differ only
        // a little, and the field doesn't repeat over a long stretch.
        let field = sky_fields(Cover::Broken, ROWS, 5)[1];
        let row = 80.5;
        let mut biggest_step: f64 = 0.0;
        for x in 0..2000 {
            let a = field.density(f64::from(x), row);
            let b = field.density(f64::from(x) + 0.25, row);
            biggest_step = biggest_step.max((a - b).abs());
        }
        assert!(biggest_step < 0.35, "{biggest_step}");
        let strip = |left: f64| {
            (0..200)
                .map(|x| field.density(left + f64::from(x), row))
                .collect::<Vec<_>>()
        };
        assert_ne!(strip(0.0), strip(1000.0));
    }

    #[test]
    fn mid_clouds_have_flat_bases() {
        let field = sky_fields(Cover::Overcast, ROWS, 2)[1];
        let (_, base) = Layer::Mid.band();
        let below = f64::from(ROWS) * base + 2.0;
        for x in 0..500 {
            assert_eq!(field.density(f64::from(x), below), 0.0);
        }
        // Heaps have flat bases. With the sky full, heaps at different
        // depths overlap, so flatness is checked locally: most neighbouring
        // columns end on the same bottom row (within one), with steps only
        // where one heap gives way to another.
        let bottom = |x: i32| {
            (0..ROWS)
                .rev()
                .find(|&row| field.density(f64::from(x) + 0.5, f64::from(row) + 0.5) > 0.0)
        };
        let bottoms: Vec<i32> = (0..400).filter_map(bottom).collect();
        assert!(bottoms.len() > 200);
        let flat = bottoms
            .windows(2)
            .filter(|w| (w[0] - w[1]).abs() <= 1)
            .count();
        assert!(
            flat * 10 >= (bottoms.len() - 1) * 8,
            "{flat} of {}",
            bottoms.len() - 1
        );
    }

    #[test]
    fn thick_cloud_hides_the_moon_and_thin_cloud_lets_it_through() {
        let storm = sky_fields(Cover::Storm, ROWS, 1)[1];
        let thin = LayerField::new(
            Layer::High,
            LayerWeather {
                coverage: 0.5,
                thickness: 0.6,
            },
            ROWS,
            1,
        );
        assert!(storm.transmission(1.0) <= 1.0 - MAX_OPACITY + 1e-12);
        assert!(
            storm.transmission(1.0) < 0.1,
            "dense storm cloud nearly hides the moon"
        );
        assert!(thin.transmission(0.5) > 0.7, "thin cloud lets it through");
        assert!(storm.opacity(0.0) == 0.0);
        // Opacity rises with density and never reaches solid.
        let mut previous = 0.0;
        for step in 1..=20 {
            let o = storm.opacity(f64::from(step) / 20.0);
            assert!(o >= previous && o <= MAX_OPACITY);
            previous = o;
        }
    }

    #[test]
    fn the_side_facing_the_moon_is_brighter_than_the_far_side() {
        let field = sky_fields(Cover::Broken, ROWS, 4)[1];
        let row = 100.5;
        let light = Some(Light {
            x: -400.0,
            y: row,
            radius: 19.5,
            strength: 1.0,
        });
        // Find a run of cloud along the row and compare its two ends.
        let cloudy: Vec<f64> = (0..600)
            .map(|x| f64::from(x) + 0.5)
            .filter(|&x| field.density(x, row) > 0.6)
            .collect();
        assert!(cloudy.len() > 20);
        let run_start = cloudy
            .windows(2)
            .position(|w| w[1] - w[0] > 1.5)
            .map_or(cloudy.len() - 1, |i| i);
        let (left, right) = (cloudy[0], cloudy[run_start]);
        assert!(right - left > 6.0, "a wide enough run");
        let lit = light_at(&field, left, row, left, light);
        let shaded = light_at(&field, right, row, right, light);
        assert!(lit > shaded, "{lit} vs {shaded}");
    }

    #[test]
    fn edge_light_near_the_moon_never_shines_through_opaque_cloud() {
        let field = sky_fields(Cover::Storm, ROWS, 6)[1];
        let moon = Light {
            x: 100.0,
            y: 100.0,
            radius: 19.5,
            strength: 1.0,
        };
        // The edge-light share is weighted by transmission: at full density
        // it is at most (1 - MAX_OPACITY) of its strength.
        let opaque_weight = field.transmission(1.0);
        assert!(opaque_weight < 0.1);
        let dense = (0..400)
            .map(|x| f64::from(x) + 0.5)
            .find(|&x| field.density(x, 100.5) > 0.95)
            .expect("a dense cell");
        let with = light_at(&field, dense, 100.5, dense, Some(moon));
        let far = light_at(
            &field,
            dense,
            100.5,
            dense,
            Some(Light {
                x: 10_000.0,
                ..moon
            }),
        );
        assert!(
            with - far < 0.2,
            "no halo through opaque cloud: {with} vs {far}"
        );
    }

    #[test]
    fn thin_edges_dither_in_a_fixed_pattern() {
        assert_eq!(dithered_alpha(0.0, 0, 0), 0.0);
        assert_eq!(dithered_alpha(0.8, 2, 3), 0.8);
        for (alpha, lo, hi) in [(0.03, 1, 3), (0.15, 7, 9), (0.27, 13, 15)] {
            let drawn = (0..4)
                .flat_map(|y| (0..4).map(move |x| (x, y)))
                .filter(|&(x, y)| dithered_alpha(alpha, x, y) > 0.0)
                .count();
            assert!((lo..=hi).contains(&drawn), "{alpha}: {drawn}");
        }
        assert_eq!(dithered_alpha(0.1, 6, 9), dithered_alpha(0.1, 2, 1));
    }

    #[test]
    fn layers_stay_in_their_bands_and_storms_are_dark_and_heavy() {
        for (i, layer) in LAYERS.iter().enumerate() {
            let field = sky_fields(Cover::Storm, ROWS, 8)[i];
            let (top, base) = layer.band();
            for x in 0..200 {
                assert_eq!(
                    field.density(f64::from(x), f64::from(ROWS) * top - 1.0),
                    0.0
                );
                assert_eq!(
                    field.density(f64::from(x), f64::from(ROWS) * base + 1.0),
                    0.0
                );
            }
        }
        // Clouds live in the upper 60% of the sky (below that is behind the
        // skyline); a storm fills most of it.
        let upper = (f64::from(ROWS) * 0.6) as i32;
        let storm = coverage(&sky_fields(Cover::Storm, ROWS, 8), 0.0, 400, upper);
        assert!(storm > 0.6, "{storm}");
        assert!(Layer::Low.ambient() < Layer::High.ambient());
    }

    #[test]
    fn tones_span_the_range() {
        assert_eq!(tone_index(0.0), 0);
        assert_eq!(tone_index(1.0), TONES - 1);
    }
}
