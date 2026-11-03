//! City generation: lays buildings out left to right across parallax layers
//! (Rebuild Prompt, "Deterministic Generation" and "Time, Reveal, And
//! Endless Motion").
//!
//! A [`CityGenerator`] owns one RNG stream and a cursor per selected layer.
//! It always extends the layer whose cursor is furthest left, so buildings
//! appear in world order and tint choices can see their left-hand
//! neighbors. A Normal city uses one generator for all four layers. Endless
//! mode uses one independent generator per layer, each with its own seed,
//! extended ahead of the camera and pruned behind it.

use super::building::{Building, BuildingKind};
use super::decorations::{is_eligible, make_decoration, maybe_decorate};
use super::grid::Grid;
use super::layers::LayerSpec;
use super::mast::plan_mast;
use super::rng::{layer_seed, Mulberry32};
use super::shapes::build_tiers;
use super::tint::{choose_body_index, TintNeighbor};
use super::windows::{generate_windows, with_cast_fade};

/// Number of parallax layers.
pub const LAYER_COUNT: u8 = 4;
/// Largest random pull-back of each layer cursor from the start x.
pub const START_JITTER: i64 = 10;
/// Generator start x for endless child layers.
pub const ENDLESS_START_X: i32 = -28;
/// How far (cells) to the left a nearer building can shade a window.
pub const CAST_REACH: i32 = 3;
/// Inclusive range of landmarks a Normal city guarantees.
pub const ENSURED_LANDMARKS: (i64, i64) = (1, 3);

/// Generates buildings for a set of layers from one seeded stream.
#[derive(Debug, Clone)]
pub struct CityGenerator {
    rng: Mulberry32,
    max_h: i32,
    layers: Vec<u8>,
    cursors: [i32; LAYER_COUNT as usize],
    neighbors: Vec<TintNeighbor>,
    light_windows: bool,
    buildings: Vec<Building>,
}

impl CityGenerator {
    /// Creates a generator for `layers` (each 0..=3, no duplicates).
    ///
    /// Each layer's cursor starts at `start_x - randInt(0, 10)`, drawn in the
    /// order the layers are given. When `light_windows` is true (Endless),
    /// windows are created already lit to their targets; otherwise (Normal)
    /// they start dark for the reveal.
    ///
    /// # Panics
    ///
    /// Panics if a layer is out of range or repeated.
    pub fn new(seed: u32, layers: &[u8], max_h: i32, start_x: i32, light_windows: bool) -> Self {
        let mut rng = Mulberry32::new(seed);
        let mut cursors = [i32::MAX; LAYER_COUNT as usize];
        for &layer in layers {
            assert!(layer < LAYER_COUNT, "layer {layer} out of range");
            let slot = &mut cursors[usize::from(layer)];
            assert_eq!(*slot, i32::MAX, "layer {layer} repeated");
            *slot = start_x - rng.rand_int(0, START_JITTER) as i32;
        }
        Self {
            rng,
            max_h,
            layers: layers.to_vec(),
            cursors,
            neighbors: Vec::new(),
            light_windows,
            buildings: Vec::new(),
        }
    }

    /// Buildings generated so far (and not pruned), in generation order.
    pub fn buildings(&self) -> &[Building] {
        &self.buildings
    }

    /// Consumes the generator, returning its buildings.
    pub fn into_buildings(self) -> Vec<Building> {
        self.buildings
    }

    /// The next free world x in `layer`, or `None` if it is not selected.
    pub fn cursor(&self, layer: u8) -> Option<i32> {
        self.layers
            .contains(&layer)
            .then(|| self.cursors[usize::from(layer)])
    }

    /// Generates buildings until every selected layer's cursor reaches
    /// `world_x`. Calling it again with a larger x continues the same city.
    /// O(new buildings × building cost).
    pub fn extend_to(&mut self, world_x: i32) {
        while let Some(layer) = self.next_layer(world_x) {
            let building = self.generate_building(layer);
            self.buildings.push(building);
        }
    }

    /// Drops buildings that end at or before `world_x`, bounding memory as
    /// the endless camera moves on. O(buildings).
    pub fn prune_before(&mut self, world_x: i32) {
        self.buildings.retain(|building| building.x1 > world_x);
    }

    /// The selected layer with the leftmost cursor below `world_x`; ties go
    /// to the farther layer.
    fn next_layer(&self, world_x: i32) -> Option<u8> {
        self.layers
            .iter()
            .copied()
            .filter(|&layer| self.cursors[usize::from(layer)] < world_x)
            .min_by_key(|&layer| (self.cursors[usize::from(layer)], layer))
    }

    fn generate_building(&mut self, layer: u8) -> Building {
        let spec = LayerSpec::for_layer(usize::from(layer));
        let rng = &mut self.rng;
        let x0 = self.cursors[usize::from(layer)];
        let picked = BuildingKind::pick(rng, layer);
        let width = spec.sample_footprint_width(picked, layer, rng);
        let height = spec.sample_height(rng, self.max_h);
        let (tiers, kind) = build_tiers(picked, x0, width, height, self.max_h, rng);
        let x1 = x0 + width;
        let body_index = choose_body_index(rng, layer, x0, x1, &self.neighbors);
        let tone = rng.rand_int(0, 255) as u8;
        let accent_roll = rng.next_f64();
        let mast = plan_mast(kind, &tiers, self.max_h, rng);
        let mut building = Building {
            layer,
            x0,
            x1,
            kind,
            tiers,
            body_index,
            tone,
            accent_roll,
            windows: Default::default(),
            mast,
            decoration: None,
        };
        building.windows = generate_windows(&building, rng);
        if self.light_windows {
            building.windows.light_to_target();
        }
        building.decoration = maybe_decorate(&building, self.max_h, rng);
        let gap = spec.sample_gap(rng);
        self.cursors[usize::from(layer)] = x1 + gap;

        self.neighbors.push(TintNeighbor {
            layer,
            x0,
            x1,
            body: body_index,
        });
        // New buildings start at or right of the leftmost cursor, so older
        // neighbors ending before it can never touch one again.
        let reach = self
            .layers
            .iter()
            .map(|&layer| self.cursors[usize::from(layer)])
            .min()
            .unwrap_or(i32::MAX);
        self.neighbors.retain(|neighbor| neighbor.x1 >= reach);
        building
    }
}

/// Generates a complete Normal-mode city covering the grid's columns.
///
/// All four layers share one stream seeded with `seed`. Windows start dark
/// (the reveal lights them). Afterwards the city gets 1 to 3 landmark
/// decorations if generation produced none, and the standard-mode cast
/// fade is applied.
pub fn generate_normal_city(seed: u32, grid: &Grid) -> Vec<Building> {
    let mut generator = CityGenerator::new(seed, &[0, 1, 2, 3], grid.max_h, 0, false);
    generator.extend_to(grid.cols);
    let mut rng = generator.rng.clone();
    let mut buildings = generator.into_buildings();
    ensure_landmarks(&mut buildings, grid.max_h, &mut rng);
    apply_cast_fade(&mut buildings);
    buildings
}

/// Creates the independent generator for one Endless layer, seeded with
/// `seed XOR (0x85ebca6b + layer)`, with windows lit to their targets.
pub fn endless_layer_generator(seed: u32, layer: u8, max_h: i32) -> CityGenerator {
    CityGenerator::new(
        layer_seed(seed, u32::from(layer)),
        &[layer],
        max_h,
        ENDLESS_START_X,
        true,
    )
}

/// If no building is decorated, decorates 1 to 3 eligible buildings chosen
/// at random. Returns how many were added. O(buildings).
pub fn ensure_landmarks(buildings: &mut [Building], max_h: i32, rng: &mut Mulberry32) -> usize {
    if buildings
        .iter()
        .any(|building| building.decoration.is_some())
    {
        return 0;
    }
    let mut eligible: Vec<usize> = (0..buildings.len())
        .filter(|&index| is_eligible(&buildings[index], max_h))
        .collect();
    if eligible.is_empty() {
        return 0;
    }
    let wanted =
        (rng.rand_int(ENSURED_LANDMARKS.0, ENSURED_LANDMARKS.1) as usize).min(eligible.len());
    // Partial Fisher-Yates: the first `wanted` entries become a random pick.
    for slot in 0..wanted {
        let other = rng.rand_int(slot as i64, eligible.len() as i64 - 1) as usize;
        eligible.swap(slot, other);
    }
    let mut added = 0;
    for &index in &eligible[..wanted] {
        buildings[index].decoration = make_decoration(&buildings[index], rng);
        added += usize::from(buildings[index].decoration.is_some());
    }
    added
}

/// Darkens windows that sit in the shadow of nearer geometry: a nearer-layer
/// building covering any cell up to three columns to the left, at the same
/// height or up to that many rows above (left / upper-left).
///
/// O(windows × nearby nearer buildings × tiers).
pub fn apply_cast_fade(buildings: &mut [Building]) {
    let shapes: Vec<(u8, i32, i32)> = buildings.iter().map(|b| (b.layer, b.x0, b.x1)).collect();
    for index in 0..buildings.len() {
        let (layer, x0, x1) = shapes[index];
        let casters: Vec<usize> = (0..buildings.len())
            .filter(|&other| {
                let (other_layer, ox0, ox1) = shapes[other];
                other_layer > layer && ox0 < x1 && x0 - CAST_REACH < ox1
            })
            .collect();
        if casters.is_empty() {
            continue;
        }
        let shaded: Vec<usize> = (0..buildings[index].windows.len())
            .filter(|&w| {
                let x = buildings[index].windows.x[w];
                let h = buildings[index].windows.h[w];
                casters.iter().any(|&caster| {
                    (1..=CAST_REACH)
                        .any(|dx| (0..=dx).any(|dy| buildings[caster].covers(x - dx, h + dy)))
                })
            })
            .collect();
        let windows = &mut buildings[index].windows;
        for w in shaded {
            windows.shade[w] = with_cast_fade(windows.shade[w]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::building::{Tier, WindowSet};

    fn slab(layer: u8, x0: i32, x1: i32, h: i32) -> Building {
        Building {
            layer,
            x0,
            x1,
            kind: BuildingKind::Box,
            tiers: vec![Tier::new(x0, x1, 0, h).unwrap()],
            body_index: 0,
            tone: 0,
            accent_roll: 0.0,
            windows: WindowSet::default(),
            mast: None,
            decoration: None,
        }
    }

    #[test]
    fn cursors_start_within_jitter_and_layers_are_selected() {
        let generator = CityGenerator::new(5, &[2, 0], 100, 0, false);
        for layer in [0, 2] {
            let cursor = generator.cursor(layer).unwrap();
            assert!((-10..=0).contains(&cursor), "layer {layer}: {cursor}");
        }
        assert_eq!(generator.cursor(1), None);
    }

    #[test]
    #[should_panic(expected = "repeated")]
    fn repeated_layers_are_rejected() {
        CityGenerator::new(1, &[1, 1], 100, 0, false);
    }

    #[test]
    fn extending_in_steps_matches_extending_at_once() {
        let mut stepped = endless_layer_generator(42, 2, 120);
        for x in [50, 120, 400, 401, 900] {
            stepped.extend_to(x);
        }
        let mut direct = endless_layer_generator(42, 2, 120);
        direct.extend_to(900);
        assert_eq!(stepped.buildings(), direct.buildings());
        assert!(direct.cursor(2).unwrap() >= 900);
    }

    #[test]
    fn pruning_keeps_only_buildings_that_reach_past_the_cut() {
        let mut generator = endless_layer_generator(7, 3, 120);
        generator.extend_to(600);
        let before = generator.buildings().len();
        generator.prune_before(300);
        assert!(generator.buildings().len() < before);
        assert!(generator.buildings().iter().all(|b| b.x1 > 300));
        // Continuing after pruning still works and stays non-overlapping.
        generator.extend_to(900);
        let xs: Vec<_> = generator.buildings().iter().map(|b| (b.x0, b.x1)).collect();
        assert!(xs.windows(2).all(|pair| pair[0].1 <= pair[1].0));
    }

    #[test]
    fn endless_windows_are_lit_and_normal_windows_start_dark() {
        let mut endless = endless_layer_generator(3, 1, 120);
        endless.extend_to(300);
        assert!(endless
            .buildings()
            .iter()
            .all(|b| b.windows.lit == b.windows.target));
        let normal = generate_normal_city(3, &Grid::new(1280, 720, false));
        assert!(normal.iter().all(|b| b.windows.lit_count() == 0));
    }

    #[test]
    fn ensure_landmarks_adds_one_to_three_only_when_missing() {
        let mut buildings: Vec<_> = (0..10).map(|i| slab(1, i * 10, i * 10 + 8, 60)).collect();
        let mut rng = Mulberry32::new(4);
        let added = ensure_landmarks(&mut buildings, 100, &mut rng);
        assert!((1..=3).contains(&added));
        let decorated = buildings.iter().filter(|b| b.decoration.is_some()).count();
        assert_eq!(decorated, added);
        assert_eq!(
            ensure_landmarks(&mut buildings, 100, &mut rng),
            0,
            "already has some"
        );

        let mut short: Vec<_> = (0..3).map(|i| slab(1, i * 10, i * 10 + 8, 10)).collect();
        assert_eq!(
            ensure_landmarks(&mut short, 100, &mut rng),
            0,
            "nothing eligible"
        );
    }

    #[test]
    fn cast_fade_darkens_windows_right_of_nearer_buildings() {
        let mut far = slab(0, 10, 30, 40);
        for x in [11, 15, 25] {
            far.windows.push(x, 20, 0, 0, true);
        }
        let near = slab(3, 0, 13, 50); // covers columns up to 12
        let mut buildings = vec![far, near];
        apply_cast_fade(&mut buildings);
        let shades = &buildings[0].windows.shade;
        // x=11 is behind the nearer building; x=15 is within 3 columns of
        // column 12; x=25 is out of reach.
        assert_eq!(shades[0], with_cast_fade(0));
        assert_eq!(shades[1], with_cast_fade(0));
        assert_eq!(shades[2], 0);
    }
}
