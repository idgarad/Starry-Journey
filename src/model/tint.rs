//! Body-tint choice that keeps neighboring buildings visually distinct
//! (Rebuild Prompt, "Deterministic Generation", body tints paragraph).
//!
//! A new building avoids the body colors of buildings whose footprints
//! touch or overlap it, relaxing in three steps:
//! 1. exclude touching buildings in the same layer, adjacent layers, and
//!    any farther layer;
//! 2. exclude only the same and adjacent layers;
//! 3. allow all six colors.

use super::palette::BODY_COUNT;
use super::rng::Mulberry32;

/// A previously placed building that may constrain a new body tint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TintNeighbor {
    /// Layer, 0 (farthest) to 3 (nearest).
    pub layer: u8,
    /// Footprint start (inclusive).
    pub x0: i32,
    /// Footprint end (exclusive).
    pub x1: i32,
    /// Chosen body index, 0..=5.
    pub body: u8,
}

impl TintNeighbor {
    /// True if this footprint touches or overlaps `[x0, x1)`. Sharing an
    /// edge counts as touching.
    pub fn touches(&self, x0: i32, x1: i32) -> bool {
        self.x0 <= x1 && x0 <= self.x1
    }
}

/// Chooses a body index for a building in `layer` with footprint
/// `[x0, x1)`, consuming one RNG value. O(neighbors + 6).
pub fn choose_body_index(
    rng: &mut Mulberry32,
    layer: u8,
    x0: i32,
    x1: i32,
    neighbors: &[TintNeighbor],
) -> u8 {
    let touching: Vec<&TintNeighbor> = neighbors
        .iter()
        .filter(|neighbor| neighbor.touches(x0, x1))
        .collect();
    let near = |neighbor: &&TintNeighbor| neighbor.layer.abs_diff(layer) <= 1;
    let farther = |neighbor: &&TintNeighbor| neighbor.layer < layer;

    let strict = mask(touching.iter().filter(|n| near(n) || farther(n)));
    let relaxed = mask(touching.iter().filter(|n| near(n)));
    let used = if strict.count_ones() < BODY_COUNT as u32 {
        strict
    } else if relaxed.count_ones() < BODY_COUNT as u32 {
        relaxed
    } else {
        0
    };
    let candidates: Vec<u8> = (0..BODY_COUNT as u8)
        .filter(|body| used & (1 << body) == 0)
        .collect();
    let pick = rng.rand_int(0, candidates.len() as i64 - 1) as usize;
    candidates[pick]
}

/// Bit set of the body indices used by `neighbors`.
fn mask<'a>(neighbors: impl Iterator<Item = &'a &'a TintNeighbor>) -> u8 {
    neighbors.fold(0, |bits, neighbor| bits | (1 << neighbor.body))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(layer: u8, x0: i32, x1: i32, body: u8) -> TintNeighbor {
        TintNeighbor {
            layer,
            x0,
            x1,
            body,
        }
    }

    #[test]
    fn touching_includes_shared_edges_only() {
        let neighbor = n(0, 10, 20, 0);
        assert!(neighbor.touches(20, 25), "shared edge");
        assert!(neighbor.touches(5, 10), "shared edge on the left");
        assert!(neighbor.touches(12, 14), "overlap");
        assert!(!neighbor.touches(21, 25));
        assert!(!neighbor.touches(0, 9));
    }

    #[test]
    fn avoids_touching_colors_in_relevant_layers() {
        let neighbors = [
            n(2, 0, 10, 0),
            n(1, 5, 15, 1),
            n(0, 8, 12, 2),
            n(3, 9, 30, 3),
        ];
        let mut rng = Mulberry32::new(1);
        for _ in 0..500 {
            let body = choose_body_index(&mut rng, 2, 10, 20, &neighbors);
            assert!(
                body >= 3,
                "excludes same, adjacent, and farther layers: {body}"
            );
        }
    }

    #[test]
    fn ignores_nearer_non_adjacent_and_distant_buildings() {
        let neighbors = [n(3, 0, 50, 0), n(1, 100, 110, 1)];
        let mut rng = Mulberry32::new(2);
        let seen: std::collections::HashSet<u8> = (0..500)
            .map(|_| choose_body_index(&mut rng, 1, 10, 20, &neighbors))
            .collect();
        assert_eq!(
            seen.len(),
            6,
            "layer 3 is two layers nearer; layer-1 one is far away"
        );
    }

    #[test]
    fn relaxes_when_every_color_is_taken() {
        // Farther layer 0 uses colors 0..=3; same layer uses 4 and 5.
        let mut neighbors: Vec<_> = (0..4).map(|body| n(0, 0, 20, body)).collect();
        neighbors.push(n(2, 0, 20, 4));
        neighbors.push(n(2, 0, 20, 5));
        let mut rng = Mulberry32::new(3);
        for _ in 0..200 {
            let body = choose_body_index(&mut rng, 2, 5, 10, &neighbors);
            assert!(
                body <= 3,
                "farther layers relaxed, same layer still excluded"
            );
        }
        let all: Vec<_> = (0..6).map(|body| n(2, 0, 20, body)).collect();
        let seen: std::collections::HashSet<u8> = (0..300)
            .map(|_| choose_body_index(&mut rng, 2, 5, 10, &all))
            .collect();
        assert_eq!(
            seen.len(),
            6,
            "all six allowed when even same-layer colors are full"
        );
    }
}
