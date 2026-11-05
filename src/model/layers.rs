//! Per-layer generator parameters (Rebuild Prompt, "Deterministic Generation",
//! layer table).
//!
//! Layers are ordered farthest (0) to nearest (3). The farthest layer
//! intentionally has the tallest general silhouettes; nearer layers are
//! shorter with wider gaps, so the skyline reads as depth.

use super::building::{BuildingKind, BuildingProfile};
use super::rng::{js_round, Mulberry32};

/// Smallest generated building height in cells.
pub const MIN_BUILDING_HEIGHT: i32 = 4;

/// Chance that an ordinary building gets a wide footprint (extension; see
/// `docs/product.md`, "Expanded buildings").
pub const WIDE_CHANCE: f64 = 0.15;
/// Inclusive width range for a wide ordinary footprint.
pub const WIDE_RANGE: (i64, i64) = (19, 25);
/// Widest footprint for any building that is not near-road low-rise.
pub const MAX_GENERAL_WIDTH: i32 = 25;

/// Sampling ranges for one parallax layer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerSpec {
    /// Inclusive building width range in cells.
    pub width: (i64, i64),
    /// Inclusive gap range between buildings in cells.
    pub gap: (i64, i64),
    /// Height base as a fraction of `max_h`.
    pub height_base: f64,
    /// Height span as a fraction of `max_h`.
    pub height_span: f64,
    /// Exponent applied to the random height sample.
    pub power: f64,
}

/// The four layer specifications, farthest to nearest.
pub const LAYER_SPECS: [LayerSpec; 4] = [
    LayerSpec {
        width: (6, 16),
        gap: (0, 3),
        height_base: 0.45,
        height_span: 0.55,
        power: 1.2,
    },
    LayerSpec {
        width: (6, 18),
        gap: (1, 5),
        height_base: 0.32,
        height_span: 0.50,
        power: 1.2,
    },
    LayerSpec {
        width: (6, 18),
        gap: (2, 7),
        height_base: 0.18,
        height_span: 0.40,
        power: 1.0,
    },
    LayerSpec {
        width: (5, 16),
        gap: (3, 12),
        height_base: 0.07,
        height_span: 0.25,
        power: 1.0,
    },
];

impl LayerSpec {
    /// Returns the spec for `layer` (0..=3).
    ///
    /// # Panics
    ///
    /// Panics if `layer > 3`.
    pub fn for_layer(layer: usize) -> &'static LayerSpec {
        &LAYER_SPECS[layer]
    }

    /// Samples a building width, consuming one RNG value.
    pub fn sample_width(&self, rng: &mut Mulberry32) -> i32 {
        rng.rand_int(self.width.0, self.width.1) as i32
    }

    /// Samples the footprint width for a building of `kind` in `layer`.
    ///
    /// Kinds with their own range (near-road low-rise) use it, up to 50
    /// cells. Otherwise the layer's specified range is used, except that
    /// [`WIDE_CHANCE`] of buildings take a wide footprint from
    /// [`WIDE_RANGE`], never above [`MAX_GENERAL_WIDTH`]. Consumes one or two
    /// RNG values.
    pub fn sample_footprint_width(
        &self,
        kind: BuildingKind,
        layer: u8,
        rng: &mut Mulberry32,
    ) -> i32 {
        if let Some((lo, hi)) = kind.own_width_range(layer) {
            return rng.rand_int(lo, hi) as i32;
        }
        if rng.next_f64() < WIDE_CHANCE {
            rng.rand_int(WIDE_RANGE.0, WIDE_RANGE.1) as i32
        } else {
            self.sample_width(rng)
        }
    }

    /// Like [`LayerSpec::sample_footprint_width`] for `profile`: the
    /// baseline uses only the layer's specified range (one RNG value), like
    /// the Python reference.
    pub fn sample_footprint_width_in(
        &self,
        kind: BuildingKind,
        layer: u8,
        profile: BuildingProfile,
        rng: &mut Mulberry32,
    ) -> i32 {
        match profile {
            BuildingProfile::All => self.sample_footprint_width(kind, layer, rng),
            BuildingProfile::Baseline => self.sample_width(rng),
        }
    }

    /// Samples the gap after a building, consuming one RNG value.
    pub fn sample_gap(&self, rng: &mut Mulberry32) -> i32 {
        rng.rand_int(self.gap.0, self.gap.1) as i32
    }

    /// Samples a building height, consuming one RNG value:
    /// `max(4, round(max_h * (base + span * random^power)))`, then clamped to
    /// `max_h`.
    pub fn sample_height(&self, rng: &mut Mulberry32, max_h: i32) -> i32 {
        self.height_for(rng.next_f64(), max_h)
    }

    /// The height formula for a given random value in `[0, 1)`.
    pub fn height_for(&self, random: f64, max_h: i32) -> i32 {
        let fraction = self.height_base + self.height_span * random.powf(self.power);
        let height = js_round(f64::from(max_h) * fraction) as i32;
        height.max(MIN_BUILDING_HEIGHT).min(max_h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_specs_match_the_table() {
        assert_eq!(LayerSpec::for_layer(0).width, (6, 16));
        assert_eq!(LayerSpec::for_layer(1).gap, (1, 5));
        assert_eq!(LayerSpec::for_layer(2).height_base, 0.18);
        assert_eq!(LayerSpec::for_layer(3).width, (5, 16));
        assert_eq!(LayerSpec::for_layer(3).gap, (3, 12));
        assert_eq!(LayerSpec::for_layer(3).height_span, 0.25);
        assert_eq!(LayerSpec::for_layer(0).power, 1.2);
        assert_eq!(LayerSpec::for_layer(3).power, 1.0);
    }

    #[test]
    fn height_formula_endpoints() {
        let far = LayerSpec::for_layer(0);
        assert_eq!(far.height_for(0.0, 100), 45);
        assert_eq!(far.height_for(0.999_999, 100), 100);
        let near = LayerSpec::for_layer(3);
        assert_eq!(near.height_for(0.0, 100), 7);
        assert_eq!(near.height_for(0.5, 100), 20); // 7 + 12.5 -> round 19.5 = 20
    }

    #[test]
    fn height_respects_minimum_and_limit() {
        let near = LayerSpec::for_layer(3);
        assert_eq!(near.height_for(0.0, 8), MIN_BUILDING_HEIGHT); // round(0.56) = 1
        let far = LayerSpec::for_layer(0);
        for max_h in [8, 50, 191] {
            for step in 0..100 {
                let height = far.height_for(f64::from(step) / 100.0, max_h);
                assert!((MIN_BUILDING_HEIGHT..=max_h).contains(&height));
            }
        }
    }

    #[test]
    fn far_layers_are_taller_on_average() {
        let mut rng = Mulberry32::new(9);
        let mean = |layer: usize, rng: &mut Mulberry32| {
            let spec = LayerSpec::for_layer(layer);
            (0..2_000)
                .map(|_| f64::from(spec.sample_height(rng, 191)))
                .sum::<f64>()
                / 2_000.0
        };
        let means: Vec<f64> = (0..4).map(|layer| mean(layer, &mut rng)).collect();
        assert!(means.windows(2).all(|pair| pair[0] > pair[1]), "{means:?}");
    }

    #[test]
    fn footprint_widths_follow_general_and_low_rise_limits() {
        let mut rng = Mulberry32::new(21);
        let mut wide = 0;
        let draws = 20_000;
        for _ in 0..draws {
            for layer in 0..4u8 {
                let spec = LayerSpec::for_layer(usize::from(layer));
                let width = spec.sample_footprint_width(BuildingKind::Stepped, layer, &mut rng);
                assert!((5..=MAX_GENERAL_WIDTH).contains(&width), "width {width}");
                if layer == 0 && width >= 19 {
                    wide += 1;
                }
            }
        }
        // Layer 0's own range tops out at 16, so every width >= 19 is a wide roll.
        let share = f64::from(wide) / f64::from(draws);
        assert!((0.13..0.17).contains(&share), "wide share {share}");

        let near = LayerSpec::for_layer(3);
        let mut widest = 0;
        for _ in 0..2_000 {
            let width = near.sample_footprint_width(BuildingKind::Warehouse, 3, &mut rng);
            assert!((20..=50).contains(&width));
            widest = widest.max(width);
        }
        assert_eq!(widest, 50, "warehouses can reach the 50-cell limit");
    }

    #[test]
    fn samples_stay_in_inclusive_ranges() {
        let mut rng = Mulberry32::new(5);
        for spec in &LAYER_SPECS {
            for _ in 0..500 {
                let width = i64::from(spec.sample_width(&mut rng));
                let gap = i64::from(spec.sample_gap(&mut rng));
                assert!((spec.width.0..=spec.width.1).contains(&width));
                assert!((spec.gap.0..=spec.gap.1).contains(&gap));
            }
        }
    }
}
