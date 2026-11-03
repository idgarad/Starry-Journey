//! Rebuild Prompt acceptance check 1, ported: "Same seed reproduces model
//! records; validate widths, supports, height limits, no overlap, window
//! containment, body-color neighbors, and varied landmark/roof/window
//! patterns." Extended for Starry Journey's wider low-rise buildings.

use std::collections::HashSet;

use starry_journey::model::building::{Building, BuildingKind};
use starry_journey::model::city::{endless_layer_generator, generate_normal_city};
use starry_journey::model::grid::Grid;
use starry_journey::model::layers::MAX_GENERAL_WIDTH;
use starry_journey::model::shapes::MAX_SHAPE_WIDTH;

const SEEDS: [u32; 6] = [0, 1, 42, 0xdead_beef, 123_456_789, u32::MAX];

fn grids() -> [Grid; 3] {
    [
        Grid::new(1920, 1080, false),
        Grid::new(1280, 720, false),
        Grid::new(3840, 2160, false),
    ]
}

fn by_layer(buildings: &[Building], layer: u8) -> Vec<&Building> {
    let mut layer_buildings: Vec<_> = buildings.iter().filter(|b| b.layer == layer).collect();
    layer_buildings.sort_by_key(|b| b.x0);
    layer_buildings
}

#[test]
fn same_seed_reproduces_the_city_and_different_seeds_differ() {
    let grid = Grid::new(1920, 1080, false);
    let a = generate_normal_city(42, &grid);
    let b = generate_normal_city(42, &grid);
    let c = generate_normal_city(43, &grid);
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn every_building_satisfies_structural_invariants() {
    for grid in grids() {
        for seed in SEEDS {
            for building in generate_normal_city(seed, &grid) {
                assert_eq!(
                    building.validate(grid.max_h),
                    Ok(()),
                    "seed {seed}: {building:?}"
                );
                let width = building.width();
                let limit = if building.kind.is_low_rise() || building.kind == BuildingKind::Factory
                {
                    MAX_SHAPE_WIDTH
                } else {
                    MAX_GENERAL_WIDTH
                };
                assert!(
                    (1..=limit).contains(&width),
                    "{:?} width {width}",
                    building.kind
                );
            }
        }
    }
}

#[test]
fn layers_never_overlap_and_cover_the_screen() {
    for grid in grids() {
        for seed in SEEDS {
            let city = generate_normal_city(seed, &grid);
            for layer in 0..4 {
                let row = by_layer(&city, layer);
                assert!(!row.is_empty());
                for pair in row.windows(2) {
                    assert!(
                        pair[0].x1 <= pair[1].x0,
                        "layer {layer} overlap, seed {seed}"
                    );
                }
                assert!(
                    row[0].x0 <= 0,
                    "layer {layer} starts at or before the left edge"
                );
                let last = row.last().unwrap();
                // The last cursor passed the right edge, so the last building
                // starts before it.
                assert!(last.x0 < grid.cols);
            }
        }
    }
}

#[test]
fn low_rise_buildings_stay_in_the_near_layers() {
    for seed in SEEDS {
        for building in generate_normal_city(seed, &Grid::new(3840, 2160, false)) {
            if building.kind.is_low_rise() {
                assert!(
                    building.kind.allowed_in_layer(building.layer),
                    "{building:?}"
                );
                assert!(building.layer >= 2);
            }
        }
    }
}

#[test]
fn touching_neighbors_rarely_share_a_body_color() {
    let mut touching_pairs = 0;
    let mut same_color = 0;
    for grid in grids() {
        for seed in SEEDS {
            let city = generate_normal_city(seed, &grid);
            for (i, a) in city.iter().enumerate() {
                for b in &city[i + 1..] {
                    let touching = a.x0 <= b.x1 && b.x0 <= a.x1;
                    if touching && a.layer.abs_diff(b.layer) <= 1 {
                        touching_pairs += 1;
                        same_color += usize::from(a.body_index == b.body_index);
                    }
                }
            }
        }
    }
    assert!(touching_pairs > 1_000);
    // Only the last relaxation step (every color already used nearby) can
    // repeat a color, which is rare with six colors.
    let share = same_color as f64 / touching_pairs as f64;
    assert!(
        share < 0.01,
        "{same_color} of {touching_pairs} touching pairs match"
    );
}

#[test]
fn cities_have_varied_kinds_landmarks_masts_and_windows() {
    for seed in SEEDS {
        let city = generate_normal_city(seed, &Grid::new(1920, 1080, false));
        let kinds: HashSet<_> = city.iter().map(|b| b.kind).collect();
        assert!(kinds.len() >= 10, "seed {seed}: only {} kinds", kinds.len());
        let landmarks = city.iter().filter(|b| b.decoration.is_some()).count();
        assert!(
            landmarks >= 1,
            "seed {seed}: a Normal city always has a landmark"
        );
        assert!(
            city.iter().any(|b| b.mast.is_some()),
            "seed {seed}: no masts"
        );
        let windows: usize = city.iter().map(|b| b.windows.len()).sum();
        let targets: usize = city.iter().map(|b| b.windows.target_count()).sum();
        let ratio = targets as f64 / windows as f64;
        assert!(
            (0.45..0.95).contains(&ratio),
            "seed {seed}: lit ratio {ratio}"
        );
        let shades: HashSet<_> = city
            .iter()
            .flat_map(|b| b.windows.shade.iter().copied())
            .collect();
        assert!(shades.len() >= 8, "seed {seed}: shades should vary");
    }
}

#[test]
fn endless_layers_are_independent_and_extend_continuously() {
    let max_h = Grid::new(1920, 1080, true).max_h;
    for seed in SEEDS {
        for layer in 0..4u8 {
            let mut generator = endless_layer_generator(seed, layer, max_h);
            generator.extend_to(500);
            let first: Vec<_> = generator.buildings().to_vec();
            generator.extend_to(1_500);
            assert_eq!(&generator.buildings()[..first.len()], &first[..]);
            assert!(generator.buildings().iter().all(|b| b.layer == layer));
            for building in generator.buildings() {
                assert_eq!(building.validate(max_h), Ok(()));
            }
        }
        let mut layer0 = endless_layer_generator(seed, 0, max_h);
        let mut layer1 = endless_layer_generator(seed, 1, max_h);
        layer0.extend_to(300);
        layer1.extend_to(300);
        let xs0: Vec<_> = layer0.buildings().iter().map(|b| b.x0).collect();
        let xs1: Vec<_> = layer1.buildings().iter().map(|b| b.x0).collect();
        assert_ne!(xs0, xs1, "layers use separate streams");
    }
}
