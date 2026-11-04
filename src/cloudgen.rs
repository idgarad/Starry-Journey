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
/// Typical seconds a heap lives (grows in, matures, dissolves) in calm
/// weather; each heap's own life is 0.4 to 2 times this. Owner,
/// 2026-11-04: the fade in and out must be very slow (first version 120 s,
/// then 600 s, still too fast).
pub const LIFETIME_CALM: f64 = 1800.0;
/// Typical seconds a heap lives in a boiling storm (first version: 20 s,
/// then 100 s).
pub const LIFETIME_STORM: f64 = 300.0;
// Checked when building: at least three times the second version's lives.
const _: () = assert!(LIFETIME_CALM >= 3.0 * 600.0 && LIFETIME_STORM >= 3.0 * 100.0);
/// Rows of cumulus from overhead to the horizon.
pub const HEAP_DEPTHS: i32 = 10;
/// How much faster a heap's balls fall off below its base line: bases are
/// nearly flat but rounded, never cut straight (owner, 2026-11-04).
pub const BASE_SQUASH: f64 = 3.0;

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
    /// How fast clouds live and change, 0 (calm) to 1 (a boiling storm):
    /// heap lifetimes shorten and billows rise faster.
    pub churn: f64,
}

/// The weather for each layer, high to low, for a preset. Coverage and
/// thickness are separate dials, so a preset can be many thin clouds or a
/// few dense ones; values between presets blend smoothly.
pub fn weather(cover: Cover) -> [LayerWeather; 3] {
    let churn = match cover {
        Cover::Clear => 0.0,
        Cover::Puffs => 0.1,
        Cover::Scattered => 0.15,
        Cover::Broken => 0.25,
        Cover::Overcast => 0.35,
        Cover::Storm => 1.0,
    };
    let w = |coverage, thickness| LayerWeather {
        coverage,
        thickness,
        churn,
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
    /// Seconds of cloud life: heaps grow and fade, and billows drift and
    /// rise, as this advances.
    pub time: f64,
}

impl LayerField {
    /// The field for `layer` in a sky of `sky_rows`, from the scene seed,
    /// at time 0.
    pub fn new(layer: Layer, weather: LayerWeather, sky_rows: i32, seed: u32) -> Self {
        Self {
            layer,
            weather,
            seed: seed ^ layer.salt(),
            sky_rows: f64::from(sky_rows.max(1)),
            time: 0.0,
        }
    }

    /// The same field at `time` seconds of cloud life.
    pub fn at_time(self, time: f64) -> Self {
        Self {
            time: if time.is_finite() { time } else { 0.0 },
            ..self
        }
    }

    /// How far the billowy detail has drifted and risen by now, in noise
    /// units: a slow sideways drift always, and in storms a quick rise that
    /// makes the tops boil.
    fn churn_offset(&self) -> (f64, f64) {
        // Calm skies barely stir (owner: fades must be very slow); the
        // square keeps a storm's full boil while calming everything else.
        let churn = self.weather.churn * self.weather.churn;
        (
            self.time * (0.0003 + 0.01 * churn),
            -self.time * (0.0002 + 0.09 * churn),
        )
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
        let (drift, rise) = self.churn_offset();
        let detail = fractal(
            x / sx + drift,
            row / sy + rise,
            self.seed,
            4,
            self.layer.billow(),
        );
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
    /// around and above them, and their bottoms are squashed into a nearly
    /// flat, rounded base. Billowy
    /// noise roughens only the edges; empty sky stays empty.
    /// O(slots × balls + octaves).
    fn heap_density(&self, x: f64, row: f64, _v: f64) -> f64 {
        // Rows of heaps from overhead (near, big) to the horizon (far,
        // small, flat, close together). The densest wins where rows overlap.
        (0..self.sublayers())
            .map(|sub| self.sublayer_density(x, row, sub))
            .fold(0.0, f64::max)
    }

    /// How many separately drawn sub-layers this layer has: one for a
    /// sheet; for cumulus, two interleaved sets of heaps per depth row, so
    /// a nearer heap can overlap the one behind it with its own lit edge.
    /// Sub-layer 0 is nearest; draw from the last to 0.
    pub fn sublayers(&self) -> usize {
        match self.layer {
            Layer::Mid => 2 * HEAP_DEPTHS as usize,
            Layer::High | Layer::Low => 1,
        }
    }

    /// Density of one sub-layer (see [`LayerField::sublayers`]) at world
    /// `(x, row)`. O(slots × balls + octaves).
    pub fn sublayer_density(&self, x: f64, row: f64, sub: usize) -> f64 {
        if self.layer != Layer::Mid {
            return self.density(x, row);
        }
        if self.weather.coverage <= 0.0 {
            return 0.0;
        }
        let depth = (sub / 2) as i32;
        // The even sub-layer of each depth is its front set of heaps.
        let front = sub.is_multiple_of(2);
        let t = f64::from(depth) / f64::from(HEAP_DEPTHS - 1);
        self.heap_row_density(x, row, depth, t, i32::from(front))
    }

    /// How hazy a sub-layer is, 0 (near) to 0.35 (at the horizon): farther
    /// clouds fade toward the sky color, so depth reads through tone too.
    pub fn haze(&self, sub: usize) -> f64 {
        match self.layer {
            Layer::Mid => {
                let depth = (sub / 2) as f64;
                0.35 * depth / f64::from(HEAP_DEPTHS - 1)
            }
            Layer::High => 0.15,
            Layer::Low => 0.25,
        }
    }

    /// One depth row's heaps: `t` is 0 overhead to 1 at the horizon.
    /// `parity` 0 is the back sub-layer of parent heaps; 1 is the front
    /// sub-layer of their offspring (owner design, 2026-11-04: a big cloud
    /// behind, smaller ones born in front of it, overlapping it).
    ///
    /// Each slot along the world holds one parent at a time. A parent lives
    /// a seeded lifetime (see [`LIFETIME_CALM`]), and each life is a new
    /// generation with its own shape and place, so a reborn cloud is a new
    /// cloud. During its life it gives birth, at staggered times, to 2-4
    /// smaller offspring overlapping it from slightly nearer; offspring can
    /// outlive their parent. So clouds keep appearing as others dissolve.
    /// O(slots × (1 + offspring) × balls + octaves).
    fn heap_row_density(&self, x: f64, row: f64, depth: i32, t: f64, parity: i32) -> f64 {
        let scale = 1.0 - 0.78 * t;
        let spacing = HEAP_SPACING * scale;
        let salt = depth * 1000;
        let slot = (x / spacing).floor() as i32;
        let mut field: f64 = 0.0;
        for k in slot - 3..=slot + 3 {
            let lifetime = self.slot_lifetime(k, depth);
            let r = |n: i32| hash(k, salt + n, self.seed);
            // Age in lifetimes: the whole part counts generations.
            let age = self.time / lifetime + r(8);
            let generation = age.floor();
            if parity == 0 {
                field += self.parent_field(x, row, k, generation, age - generation, depth, t);
                continue;
            }
            // Offspring of this generation and of the last (which may
            // outlive their parent).
            for g in [generation - 1.0, generation] {
                field += self.offspring_field(x, row, k, g, age - g, depth, t);
            }
        }
        if field <= 0.0 {
            return 0.0;
        }
        let (sx, sy) = self.layer.scale();
        let (drift, rise) = self.churn_offset();
        let detail = fractal(
            x / (sx * scale) + drift,
            row / (sy * scale) + rise,
            self.seed ^ (salt + parity) as u32,
            4,
            true,
        );
        let rough = field + 0.45 * (detail - 0.5);
        smoothstep(0.3, 0.75, rough)
    }

    /// Seconds each life of slot `k` in depth row `depth` lasts: the
    /// weather's typical life, spread 0.5x to 2x (evenly in log) by slot so
    /// heaps never fall into step with each other.
    fn slot_lifetime(&self, k: i32, depth: i32) -> f64 {
        let typical = LIFETIME_CALM - (LIFETIME_CALM - LIFETIME_STORM) * self.weather.churn;
        typical * 0.5 * 4f64.powf(hash(k, depth * 1000 + 7, self.seed))
    }

    /// The seed of slot `k`'s generation `generation`: each life of a slot
    /// draws a fresh cloud.
    fn generation_seed(&self, generation: f64) -> u32 {
        self.seed ^ (generation as i64 as u32).wrapping_mul(0x9e37_79b9)
    }

    /// The parent of slot `k`, generation `generation`, at `phase` (0..1)
    /// of its life: where it stands and how big it is when fully grown, or
    /// `None` if this sky's coverage doesn't call for it.
    fn parent_heap(&self, k: i32, generation: f64, depth: i32, t: f64) -> Option<Heap> {
        let coverage = self.weather.coverage;
        let scale = 1.0 - 0.78 * t;
        let spacing = HEAP_SPACING * scale;
        let salt = depth * 1000;
        let seed = self.generation_seed(generation);
        let q = |n: i32| hash(k, salt + n, seed);
        // Each parent grows over 0.3 of coverage past its own threshold, so
        // raising coverage adds clouds without popping.
        let grow = smoothstep(q(1), q(1) + 0.3, coverage * 1.4);
        if grow <= 0.0 {
            return None;
        }
        // Heaps get bigger as the sky fills; distant ones are flatter.
        let width = spacing * (0.6 + 1.1 * q(2)) * (0.8 + 0.5 * coverage);
        // Bases step down toward the horizon, jittered so the rows blend
        // into one recession.
        let base_v = 0.36 + 0.62 * t.powf(0.8);
        Some(Heap {
            center: (f64::from(k) + 0.2 + 0.6 * q(4)) * spacing,
            width,
            height: width * (0.32 + 0.16 * q(3)) * (1.0 - 0.35 * t),
            base: self.sky_rows * (base_v + 0.1 * scale * (q(5) - 0.5)).min(0.995),
            grow,
            rise: fade_share(q(10)),
            fall: fade_share(q(10)),
            key: (k, salt, seed),
        })
    }

    /// The density contribution of slot `k`'s parent of `generation` at
    /// `phase` of its life.
    #[allow(clippy::too_many_arguments)]
    fn parent_field(
        &self,
        x: f64,
        row: f64,
        k: i32,
        generation: f64,
        phase: f64,
        depth: i32,
        t: f64,
    ) -> f64 {
        let Some(parent) = self.parent_heap(k, generation, depth, t) else {
            return 0.0;
        };
        let life = life_envelope(phase, parent.rise, parent.fall);
        self.heap_field(x, row, &parent, life, t)
    }

    /// The density contribution of the offspring of slot `k`'s parent of
    /// `generation`, `age` parent lifetimes after that parent's birth.
    /// Each child is born at its own time in the parent's life, lives
    /// 0.4-0.8 of a parent lifetime, and stands on the parent's base,
    /// overlapping it from in front.
    #[allow(clippy::too_many_arguments)]
    fn offspring_field(
        &self,
        x: f64,
        row: f64,
        k: i32,
        generation: f64,
        age: f64,
        depth: i32,
        t: f64,
    ) -> f64 {
        if age < 0.0 {
            return 0.0;
        }
        let Some(parent) = self.parent_heap(k, generation, depth, t) else {
            return 0.0;
        };
        let (_, salt, seed) = parent.key;
        let count = 2 + (hash(k, salt + 9, seed) * 2.99) as i32;
        let mut field = 0.0;
        for child in 0..count {
            let h = |n: i32| hash(k, salt + 500 + child * 16 + n, seed);
            let birth = 0.05 + 0.6 * h(0);
            let span = 0.4 + 0.4 * h(1);
            let phase = (age - birth) / span;
            if !(0.0..1.0).contains(&phase) {
                continue;
            }
            let width = parent.width * (0.3 + 0.25 * h(2));
            let heap = Heap {
                center: parent.center + (h(3) - 0.5) * parent.width * 1.1,
                width,
                height: width * (0.4 + 0.2 * h(4)) * (1.0 - 0.35 * t),
                // A family shares its base (one condensation level), so
                // offspring read as nearer by overlap and their own lit
                // edge, not by hanging lower.
                base: parent.base,
                grow: parent.grow,
                rise: fade_share(h(6)),
                fall: fade_share(h(6)),
                key: (k, salt + 500 + child * 16, seed),
            };
            field += self.heap_field(x, row, &heap, life_envelope(phase, heap.rise, heap.fall), t);
        }
        field
    }

    /// Metaball density of one `heap` at `life` (0..1, scaling its size
    /// and weight, so it grows in and dissolves rather than popping).
    ///
    /// A heap is a cluster of soft balls whose densities add, so
    /// overlapping balls merge into one lumpy (cauliflower) silhouette. Big
    /// balls sit low in the middle, smaller ones stack around and above
    /// them, and their bottoms are squashed into a nearly flat, rounded
    /// base. O(balls).
    fn heap_field(&self, x: f64, row: f64, heap: &Heap, life: f64, t: f64) -> f64 {
        let amount = heap.grow * life;
        if amount <= 0.0 {
            return 0.0;
        }
        let scale = 1.0 - 0.78 * t;
        let width = heap.width * (0.4 + 0.6 * amount);
        let height = heap.height * (0.4 + 0.6 * amount);
        if (x - heap.center).abs() > width {
            return 0.0;
        }
        let (k, salt, seed) = heap.key;
        // A gentle undulation of a row or two along the base, so even big
        // heaps never sit on a ruler-straight line.
        let wobble = value_noise(x / (7.0 * scale), f64::from(salt), seed ^ 0x7e57) - 0.5;
        let base_row = heap.base + wobble * 5.0 * scale.max(0.5);
        // Bases are squashed, not cut: below the base line each ball falls
        // off BASE_SQUASH times faster, so big heaps are nearly flat
        // underneath with rounded ends and small puffs stay round.
        if row > base_row + height * 0.5 {
            return 0.0;
        }
        let balls = 5 + (hash(k, salt + 6, seed) * 6.99) as i32;
        let mut field = 0.0;
        for ball in 0..balls {
            let h = |n: i32| hash(k, salt + 100 + ball * 8 + n, seed);
            // Spread across the heap, bunched toward the middle.
            let u = (h(0) + h(1)) - 1.0;
            let size = 1.0 - 0.55 * u.abs();
            let radius = height * 0.5 * size * (0.75 + 0.45 * h(2));
            let bx = heap.center + u * width * 0.42;
            // Central balls stack higher; all rest near the base.
            let lift = radius * (0.35 + 0.9 * h(3) * (1.0 - u.abs()));
            let by = base_row - lift;
            let dy = if row > base_row {
                (base_row - by) + (row - base_row) * BASE_SQUASH
            } else {
                row - by
            };
            let d2 = ((x - bx).powi(2) + dy * dy) / (radius * radius);
            if d2 < 1.0 {
                field += life * (1.0 - d2) * (1.0 - d2);
            }
        }
        field
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

/// One cumulus heap, fully grown: a parent or an offspring.
#[derive(Debug, Clone, Copy)]
struct Heap {
    /// World column of its middle.
    center: f64,
    /// Half-width in cells.
    width: f64,
    /// Height in cells.
    height: f64,
    /// The row of its (squashed) base.
    base: f64,
    /// 0..1: how far this sky's coverage has grown it in.
    grow: f64,
    /// Shares of its life spent growing in and dissolving: the same for
    /// both (see [`fade_share`]), at each cloud's own pace.
    rise: f64,
    fall: f64,
    /// `(slot, salt, seed)` for its seeded balls.
    key: (i32, i32, u32),
}

/// The share of a cloud's life it spends fading in, and again fading
/// out, from its seeded `r` (0..1): 0.3 to 0.45. Owner, 2026-11-04: fade
/// ins and fade outs looked like different speeds (they were drawn
/// separately, 0.1-0.4 in and 0.15-0.5 out).
fn fade_share(r: f64) -> f64 {
    0.3 + 0.15 * r
}

/// How alive a cloud is at `phase` (0..1) of its life: it grows over the
/// first `rise` of it and dissolves over the last `fall`.
fn life_envelope(phase: f64, rise: f64, fall: f64) -> f64 {
    smoothstep(0.0, rise, phase) * smoothstep(1.0, 1.0 - fall, phase)
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

/// How lit the cloud cell at world `(x, row)` of sub-layer `sub` is,
/// `0..1`. Each sub-layer shadows only itself, so a nearer heap keeps its
/// lit edge where it overlaps one behind it.
///
/// Ambient light for the layer, a little more on thin cells; then the
/// light, dimmed by the density between the cell and the light along a
/// short march (`exp(-k × density passed)`), so cores are dark and the side
/// facing the moon is bright. Near the moon, thin cloud gets restrained
/// edge light, weighted by how thin it is, so opaque cloud never glows.
/// `screen_x` is the cell's screen column (the light is in screen space).
/// O(steps × octaves).
pub fn light_at(
    field: &LayerField,
    sub: usize,
    x: f64,
    row: f64,
    screen_x: f64,
    light: Option<Light>,
) -> f64 {
    let d = field.sublayer_density(x, row, sub);
    let mut level = field.layer.ambient() + 0.12 * (1.0 - d);
    if let Some(light) = light {
        let (dx, dy) = (light.x - screen_x, light.y - row);
        let length = (dx * dx + dy * dy).sqrt().max(1e-9);
        let (ux, uy) = (dx / length, dy / length);
        let mut passed = 0.0;
        for step in 1..=MARCH_STEPS {
            let t = MARCH_STEP * f64::from(step);
            passed += field.sublayer_density(x + ux * t, row + uy * t, sub);
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

/// Cloud layers by distance, like the city's four parallax layers (owner
/// decision, 2026-11-04): far, mid-far, mid-near, near.
pub const CLOUD_LAYERS: usize = 4;

/// The parts of the sky in distance layer `band` (0 farthest, 3 nearest),
/// as `(field index into sky_fields, sub-layer)` pairs in drawing order
/// (far to near):
///
/// - far: the high streaks and the heaps of depth rows 7-9 (the horizon);
/// - mid-far: heap rows 4-6;
/// - mid-near: heap rows 2-3 and the low scud;
/// - near: heap rows 0-1 (big, overhead).
pub fn layer_parts(band: usize) -> Vec<(usize, usize)> {
    // Mid sub-layers: depth d has front 2d and back 2d + 1; back first.
    let heaps = |depths: std::ops::RangeInclusive<usize>| {
        depths
            .rev()
            .flat_map(|d| [(1, 2 * d + 1), (1, 2 * d)])
            .collect::<Vec<_>>()
    };
    match band {
        0 => [vec![(0, 0)], heaps(7..=9)].concat(),
        1 => heaps(4..=6),
        2 => [vec![(2, 0)], heaps(2..=3)].concat(),
        _ => heaps(0..=1),
    }
}

/// The three layer fields for a preset, high to low, at time 0 (use
/// [`LayerField::at_time`] for later).
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
                churn: 0.0,
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
        // where one heap gives way to another. The bar is 75% (owner,
        // 2026-11-04): across 12 seeds the first heaps averaged 78% and
        // cloud families 77%, so 80% only passed on a lucky seed.
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
            flat * 100 >= (bottoms.len() - 1) * 75,
            "{flat} of {}",
            bottoms.len() - 1
        );
    }

    #[test]
    fn heap_bottoms_are_rounded_not_cut_straight() {
        // The owner saw "perfectly straight lines" under puffy clouds. In
        // every sub-layer, no heap's bottom stays on one row for long.
        let field = sky_fields(Cover::Broken, ROWS, 2)[1];
        let mut longest_overall = 0;
        for sub in 0..field.sublayers() {
            let bottoms: Vec<Option<i32>> = (0..500)
                .map(|x| {
                    (0..ROWS).rev().find(|&row| {
                        field.sublayer_density(f64::from(x) + 0.5, f64::from(row) + 0.5, sub) > 0.0
                    })
                })
                .collect();
            let mut run = 0;
            for pair in bottoms.windows(2) {
                match pair {
                    [Some(a), Some(b)] if a == b => run += 1,
                    _ => run = 0,
                }
                longest_overall = longest_overall.max(run);
            }
        }
        assert!(
            longest_overall < 30,
            "a straight run of {longest_overall} columns"
        );
    }

    #[test]
    fn nearer_heaps_overlap_farther_ones() {
        let field = sky_fields(Cover::Overcast, ROWS, 3)[1];
        let mut overlapping = 0;
        for row in (0..ROWS).step_by(3) {
            for x in (0..400).step_by(3) {
                let (x, y) = (f64::from(x) + 0.5, f64::from(row) + 0.5);
                let covered = (0..field.sublayers())
                    .filter(|&sub| field.sublayer_density(x, y, sub) > 0.5)
                    .count();
                overlapping += usize::from(covered >= 2);
            }
        }
        assert!(overlapping > 50, "{overlapping}");
        assert_eq!(field.sublayers(), 2 * HEAP_DEPTHS as usize);
        assert!(
            field.haze(field.sublayers() - 1) > field.haze(0),
            "far is hazier"
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
                churn: 0.0,
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
        // Find a sub-layer with a wide run of cloud along the row and
        // compare the run's two ends.
        let (sub, left, right) = (0..field.sublayers())
            .find_map(|sub| {
                let cloudy: Vec<f64> = (0..600)
                    .map(|x| f64::from(x) + 0.5)
                    .filter(|&x| field.sublayer_density(x, row, sub) > 0.6)
                    .collect();
                let first = *cloudy.first()?;
                let end = cloudy
                    .windows(2)
                    .position(|w| w[1] - w[0] > 1.5)
                    .map_or(cloudy.len() - 1, |i| i);
                (cloudy[end] - first > 10.0).then_some((sub, first, cloudy[end]))
            })
            .expect("a wide enough run");
        let lit = light_at(&field, sub, left, row, left, light);
        let shaded = light_at(&field, sub, right, row, right, light);
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
        // A cell deep inside thick cloud: dense for 16 cells all round, so
        // whatever direction the moon is in, cloud lies between.
        let deep = |x: f64, row: f64| {
            (-16..=16).step_by(4).all(|dx| {
                (-16..=16).step_by(4).all(|dy| {
                    field.sublayer_density(x + f64::from(dx), row + f64::from(dy), 1) > 0.95
                })
            })
        };
        let (dense, row) = (0..3000)
            .step_by(2)
            .flat_map(|x| {
                (60..260)
                    .step_by(4)
                    .map(move |r| (f64::from(x) + 0.5, f64::from(r) + 0.5))
            })
            .find(|&(x, r)| deep(x, r))
            .expect("a dense cell");
        let moon = Light {
            x: dense + 3.0,
            y: row - 3.0,
            ..moon
        };
        let with = light_at(&field, 1, dense, row, dense, Some(moon));
        let far = light_at(
            &field,
            1,
            dense,
            row,
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
    fn the_four_distance_layers_cover_every_part_exactly_once() {
        let mut parts: Vec<(usize, usize)> = (0..CLOUD_LAYERS).flat_map(layer_parts).collect();
        let total = parts.len();
        parts.sort_unstable();
        parts.dedup();
        assert_eq!(parts.len(), total, "no part in two layers");
        let fields = sky_fields(Cover::Storm, ROWS, 1);
        let expected: usize = fields.iter().map(LayerField::sublayers).sum();
        assert_eq!(total, expected, "every part in some layer");
        // Within a layer, heap parts run far to near (back sub-layers first).
        for band in 0..CLOUD_LAYERS {
            let heaps: Vec<usize> = layer_parts(band)
                .into_iter()
                .filter(|&(f, _)| f == 1)
                .map(|(_, sub)| sub)
                .collect();
            assert!(heaps.windows(2).all(|w| w[0] > w[1]), "{band}: {heaps:?}");
        }
        // Nearer layers hold nearer heaps.
        let nearest = |band: usize| {
            layer_parts(band)
                .iter()
                .filter(|p| p.0 == 1)
                .map(|p| p.1)
                .min()
        };
        assert!(nearest(3) < nearest(2) && nearest(2) < nearest(1) && nearest(1) < nearest(0));
    }

    #[test]
    fn clouds_change_smoothly_over_time() {
        let field = sky_fields(Cover::Broken, ROWS, 4)[1];
        let sample = |t: f64| {
            let f = field.at_time(t);
            (0..300)
                .flat_map(|x| (60..200).step_by(5).map(move |y| (x, y)))
                .map(|(x, y)| f.density(f64::from(x) + 0.5, f64::from(y) + 0.5))
                .collect::<Vec<f64>>()
        };
        let (a, b, later) = (sample(10.0), sample(10.1), sample(900.0));
        let step: f64 = a.iter().zip(&b).map(|(p, q)| (p - q).abs()).sum::<f64>() / a.len() as f64;
        let drift: f64 = a
            .iter()
            .zip(&later)
            .map(|(p, q)| (p - q).abs())
            .sum::<f64>()
            / a.len() as f64;
        assert!(step < 0.01, "a tenth of a second changes little: {step}");
        assert!(
            drift > 5.0 * step,
            "a quarter of an hour changes a lot: {drift} vs {step}"
        );
    }

    #[test]
    fn heaps_fade_in_and_out_over_their_lives() {
        // Somewhere a cell is clear, then cloudy, then clear again, over
        // two hours of calm weather.
        let field = sky_fields(Cover::Broken, ROWS, 5)[1];
        let mut found = false;
        'cells: for x in (0..400).step_by(7) {
            for y in (60..260).step_by(9) {
                let at = |t: f64| {
                    field
                        .at_time(t)
                        .density(f64::from(x) + 0.5, f64::from(y) + 0.5)
                };
                let series: Vec<f64> = (0..120).map(|i| at(f64::from(i) * 60.0)).collect();
                let first_cloud = series.iter().position(|&d| d > 0.5);
                if let Some(start) = first_cloud {
                    let clear_before = series[..start].contains(&0.0);
                    let clear_after = series[start..].contains(&0.0);
                    if clear_before && clear_after {
                        found = true;
                        break 'cells;
                    }
                }
            }
        }
        assert!(found, "clouds come and go");
    }

    #[test]
    fn calm_skies_change_only_slowly_at_the_edges() {
        // The lifetimes are checked at build time next to their constants.
        // Over 10 s a calm sky changes only at its edges, and little.
        let field = sky_fields(Cover::Scattered, ROWS, 2)[1];
        let (a, b) = (field.at_time(100.0), field.at_time(110.0));
        let cells: Vec<(f64, f64)> = (0..300)
            .flat_map(|x| {
                (60..260)
                    .step_by(6)
                    .map(move |y| (f64::from(x) + 0.5, f64::from(y) + 0.5))
            })
            .collect();
        let cloudy = cells
            .iter()
            .filter(|&&(x, y)| a.density(x, y) > 0.0)
            .count()
            .max(1);
        let changed = cells
            .iter()
            .filter(|&&(x, y)| (a.density(x, y) > 0.0) != (b.density(x, y) > 0.0))
            .count();
        assert!(
            changed * 10 < cloudy,
            "{changed} of {cloudy} cells changed in 10 s"
        );
    }

    #[test]
    fn a_cloud_fades_out_exactly_as_it_faded_in() {
        // The visible mass at a phase of its life equals the mass at the
        // mirrored phase, for parents of several sizes.
        let field = sky_fields(Cover::Storm, ROWS, 12)[1];
        for k in 0..6 {
            let Some(heap) = field.parent_heap(k, 0.0, 2, 0.2) else {
                continue;
            };
            let mass = |phase: f64| {
                let life = life_envelope(phase, heap.rise, heap.fall);
                let mut m = 0.0;
                for dx in -60..=60 {
                    for dy in -40..=10 {
                        let (x, row) = (heap.center + f64::from(dx), heap.base + f64::from(dy));
                        m += field.heap_field(x, row, &heap, life, 0.2);
                    }
                }
                m
            };
            for phase in [0.05, 0.15, 0.25, 0.35] {
                let (rising, falling) = (mass(phase), mass(1.0 - phase));
                assert!(
                    (rising - falling).abs() <= 1e-9 * rising.max(1.0),
                    "slot {k} at {phase}: {rising} vs {falling}"
                );
            }
        }
    }

    #[test]
    fn clouds_fade_at_their_own_paces() {
        // Lifetimes and fade lengths vary from heap to heap, so they never
        // fade in step.
        let field = sky_fields(Cover::Overcast, ROWS, 11)[1];
        let heaps: Vec<Heap> = (0..60)
            .filter_map(|k| field.parent_heap(k, 0.0, 3, 0.3))
            .collect();
        assert!(heaps.len() > 20);
        let spread = |f: fn(&Heap) -> f64| {
            let values: Vec<f64> = heaps.iter().map(f).collect();
            let max = values.iter().copied().fold(f64::MIN, f64::max);
            let min = values.iter().copied().fold(f64::MAX, f64::min);
            max - min
        };
        assert!(spread(|h| h.rise) > 0.1, "fade lengths vary between clouds");
        assert!(
            heaps.iter().all(|h| h.rise == h.fall),
            "each fades out as fast as it faded in"
        );
        let lives: Vec<f64> = (0..60).map(|k| field.slot_lifetime(k, 3)).collect();
        let longest = lives.iter().copied().fold(f64::MIN, f64::max);
        let shortest = lives.iter().copied().fold(f64::MAX, f64::min);
        assert!(
            longest > 3.0 * shortest,
            "lifetimes vary: {shortest} to {longest}"
        );
    }

    #[test]
    fn a_reborn_cloud_is_a_new_cloud() {
        // Each life of a slot draws a fresh shape in a fresh place.
        let field = sky_fields(Cover::Overcast, ROWS, 8)[1];
        let mut moved = 0;
        for k in 0..20 {
            let a = field.parent_heap(k, 3.0, 2, 0.2);
            let b = field.parent_heap(k, 4.0, 2, 0.2);
            if let (Some(a), Some(b)) = (a, b) {
                if (a.center - b.center).abs() > 1.0 || (a.width - b.width).abs() > 1.0 {
                    moved += 1;
                }
            }
        }
        assert!(moved >= 5, "generations differ: {moved}");
    }

    #[test]
    fn offspring_are_born_in_front_of_their_parents() {
        // Cells of the front (offspring) sub-layer mostly sit over a parent
        // in the back sub-layer of the same depth row.
        let field = sky_fields(Cover::Broken, ROWS, 9)[1];
        let (mut front, mut over_parent) = (0, 0);
        for i in 0..6 {
            let f = field.at_time(f64::from(i) * 90.0);
            for x in (0..600).step_by(3) {
                let x = f64::from(x) + 0.5;
                for row in (100..280).step_by(3) {
                    let row = f64::from(row) + 0.5;
                    if f.sublayer_density(x, row, 4) > 0.5 {
                        front += 1;
                        let near = (-12..=12)
                            .any(|dy| f.sublayer_density(x, row + f64::from(dy), 5) > 0.2);
                        over_parent += usize::from(near);
                    }
                }
            }
        }
        assert!(front > 50, "offspring exist: {front}");
        assert!(
            over_parent * 10 >= front * 6,
            "offspring overlap parents: {over_parent} of {front}"
        );
    }

    #[test]
    fn clouds_appear_about_as_often_as_they_vanish() {
        let field = sky_fields(Cover::Broken, ROWS, 10)[1];
        let (mut born, mut gone) = (0, 0);
        for x in (0..600).step_by(6) {
            for row in (80..280).step_by(6) {
                let (x, row) = (f64::from(x) + 0.5, f64::from(row) + 0.5);
                let mut was = field.at_time(0.0).density(x, row) > 0.5;
                for i in 1..60 {
                    let now = field.at_time(f64::from(i) * 120.0).density(x, row) > 0.5;
                    born += usize::from(now && !was);
                    gone += usize::from(was && !now);
                    was = now;
                }
            }
        }
        assert!(born > 50, "clouds are born: {born}");
        assert!(
            born * 10 >= gone * 7 && gone * 10 >= born * 7,
            "births {born} balance deaths {gone}"
        );
    }

    #[test]
    fn storms_churn_faster_than_fair_weather() {
        let change = |cover: Cover| {
            let field = sky_fields(cover, ROWS, 6)[1];
            let (a, b) = (field.at_time(0.0), field.at_time(5.0));
            (0..300)
                .flat_map(|x| (40..260).step_by(6).map(move |y| (x, y)))
                .map(|(x, y)| {
                    let (x, y) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                    (a.density(x, y) - b.density(x, y)).abs()
                })
                .sum::<f64>()
        };
        assert!(change(Cover::Storm) > 2.0 * change(Cover::Broken));
    }

    #[test]
    fn tones_span_the_range() {
        assert_eq!(tone_index(0.0), 0);
        assert_eq!(tone_index(1.0), TONES - 1);
    }
}
