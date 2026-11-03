//! Roadside trees (Rebuild Prompt, "Parchment Roadside Trees").
//!
//! Trees are found by querying world slots, never stored in a growing list,
//! and use their own hashed per-slot seeds so they never disturb the city or
//! traffic streams. Overlapping queries always return identical trees.
//!
//! Starry Journey extension (owner decision 2026-11-03): trees appear in
//! every palette and in Normal mode, not only in Endless mode with Ink on
//! Parchment. On dark (night) palettes their leaves fade toward the sky
//! color so they read as silhouettes; on light palettes they fade toward the
//! ink (star) color as the spec describes.

use super::palette::{Palette, Rgb};
use super::rng::{tree_slot_seed, Mulberry32};

/// Sprite rows, top first: `.` transparent, `l` leaf, `s` leaf shade,
/// `t` trunk. The first three are deciduous, the last three conifers.
pub const TREE_SPRITES: [&[&str]; 6] = [
    &[".lll.", "lssll", "lllsl", ".lll.", "..t..", "..t.."],
    &[
        "..l..", ".lll.", "lssll", "lllsl", ".lll.", "..t..", "..t..",
    ],
    &[".lll.", "llsll", ".lll.", "..t..", "..t.."],
    &[
        "..l..", ".lsl.", "..l..", ".lll.", "llsll", "..t..", "..t..",
    ],
    &["..l..", ".lsl.", "llsll", ".lsl.", "llsll", "..t.."],
    &["..l..", ".lsl.", "..l..", ".lll.", "..t.."],
];
/// Sprite width in cells.
pub const TREE_WIDTH: i32 = 5;
/// World-slot spacing for background-verge trees.
pub const BACKGROUND_SPACING: i32 = 23;
/// World-slot spacing for foreground-verge trees.
pub const FOREGROUND_SPACING: i32 = 29;
/// How far leaves move toward the silhouette color (background, foreground).
pub const LEAF_BLEND: (f64, f64) = (0.06, 0.18);
/// Extra move toward the silhouette color for shaded leaves.
pub const SHADE_BLEND: f64 = 0.22;
/// On dark palettes, how far leaves move toward the sky (assumption AS-R008).
pub const NIGHT_LEAF_BLEND: (f64, f64) = (0.45, 0.6);

/// One tree placed on a verge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tree {
    /// World slot index (may be negative).
    pub slot: i64,
    /// Left world column of the sprite.
    pub x: i32,
    /// Index into [`TREE_SPRITES`].
    pub shape: usize,
    /// Tint byte choosing the leaf color.
    pub tint: u8,
    /// True for the foreground verge.
    pub foreground: bool,
}

impl Tree {
    /// Sprite rows, top first.
    pub fn sprite(&self) -> &'static [&'static str] {
        TREE_SPRITES[self.shape]
    }

    /// Height in cells (5 to 7).
    pub fn height(&self) -> i32 {
        self.sprite().len() as i32
    }

    /// True for conifers (shapes 3 to 5).
    pub fn is_conifer(&self) -> bool {
        self.shape >= 3
    }
}

/// The tree in one slot. Consumes only that slot's own stream.
pub fn tree_in_slot(scene_seed: u32, slot: i64, foreground: bool) -> Tree {
    let spacing = if foreground {
        FOREGROUND_SPACING
    } else {
        BACKGROUND_SPACING
    };
    let mut rng = Mulberry32::new(tree_slot_seed(scene_seed, slot, foreground));
    let x = slot as i32 * spacing + rng.rand_int(0, 7) as i32;
    let family = if slot.rem_euclid(2) == 1 { 3 } else { 0 };
    let shape = family + rng.rand_int(0, 2) as usize;
    let tint = rng.rand_int(0, 255) as u8;
    Tree {
        slot,
        x,
        shape,
        tint,
        foreground,
    }
}

/// Trees whose sprites lie entirely inside world columns `[left, right)`.
///
/// Queries slots `floor((left - 5) / spacing)` through
/// `floor(right / spacing)`. Empty or inverted intervals return none.
/// O(slots in range).
pub fn trees_in(scene_seed: u32, left: i32, right: i32, foreground: bool) -> Vec<Tree> {
    if right <= left {
        return Vec::new();
    }
    let spacing = if foreground {
        FOREGROUND_SPACING
    } else {
        BACKGROUND_SPACING
    };
    let first = (left - TREE_WIDTH).div_euclid(spacing);
    let last = right.div_euclid(spacing);
    (first..=last)
        .map(|slot| tree_in_slot(scene_seed, i64::from(slot), foreground))
        .filter(|tree| tree.x >= left && tree.x + TREE_WIDTH <= right)
        .collect()
}

/// True if the palette's sky is light (like Ink on Parchment).
pub fn is_light_palette(palette: &Palette) -> bool {
    let [r, g, b] = palette.bg.0;
    let luma = 0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b);
    luma > 127.5
}

/// Leaf, shaded-leaf, and trunk colors for a tree in `palette`.
///
/// Deciduous trees take body color `tint % 6`; conifers use body 2 or 5 by
/// tint parity. Leaves move toward the silhouette color: the ink (star)
/// color on light palettes, the sky color on dark ones. Trunks use the
/// antenna color, also faded on dark palettes.
pub fn tree_colors(tree: &Tree, palette: &Palette) -> (Rgb, Rgb, Rgb) {
    let body = if tree.is_conifer() {
        palette.bodies[if tree.tint.is_multiple_of(2) { 2 } else { 5 }]
    } else {
        palette.bodies[usize::from(tree.tint % 6)]
    };
    let light = is_light_palette(palette);
    let (silhouette, blends) = if light {
        (palette.star, LEAF_BLEND)
    } else {
        (palette.bg, NIGHT_LEAF_BLEND)
    };
    let blend = if tree.foreground { blends.1 } else { blends.0 };
    let leaf = mix(body, silhouette, blend);
    let shade = mix(leaf, silhouette, SHADE_BLEND);
    let trunk = if light {
        palette.antenna
    } else {
        mix(palette.antenna, palette.bg, blend)
    };
    (leaf, shade, trunk)
}

fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let channel = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * t).round() as u8;
    Rgb([
        channel(a.0[0], b.0[0]),
        channel(a.0[1], b.0[1]),
        channel(a.0[2], b.0[2]),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::palette::palette_by_key;

    #[test]
    fn sprites_are_five_wide_and_five_to_seven_tall() {
        for sprite in TREE_SPRITES {
            assert!((5..=7).contains(&sprite.len()));
            assert!(sprite.iter().all(|row| row.len() == TREE_WIDTH as usize));
            assert!(sprite.last().unwrap().contains('t'), "trunk at the bottom");
        }
    }

    #[test]
    fn slots_alternate_families_and_stay_near_their_slot() {
        for slot in -20..20i64 {
            for foreground in [false, true] {
                let tree = tree_in_slot(77, slot, foreground);
                let spacing = if foreground { 29 } else { 23 };
                let base = slot as i32 * spacing;
                assert!((base..=base + 7).contains(&tree.x));
                assert_eq!(tree.is_conifer(), slot.rem_euclid(2) == 1, "slot {slot}");
                assert!((5..=7).contains(&tree.height()));
            }
        }
    }

    #[test]
    fn overlapping_and_negative_queries_agree() {
        let wide = trees_in(5, -200, 400, false);
        let left = trees_in(5, -200, 100, false);
        let right = trees_in(5, 50, 400, false);
        for tree in left.iter().chain(&right) {
            assert!(wide.contains(tree), "{tree:?}");
        }
        assert!(wide.iter().any(|tree| tree.slot < 0));
        assert!(wide
            .iter()
            .all(|tree| tree.x >= -200 && tree.x + TREE_WIDTH <= 400));
        assert!(trees_in(5, 10, 10, false).is_empty());
        assert!(trees_in(5, 10, 0, true).is_empty());
    }

    #[test]
    fn trees_do_not_depend_on_other_streams() {
        assert_eq!(tree_in_slot(9, 3, false), tree_in_slot(9, 3, false));
        assert_ne!(tree_in_slot(9, 3, false), tree_in_slot(9, 3, true));
        assert_ne!(tree_in_slot(9, 3, false), tree_in_slot(10, 3, false));
    }

    #[test]
    fn colors_follow_light_and_dark_palette_rules() {
        let parchment = palette_by_key("blackonwhite").unwrap();
        let classic = palette_by_key("classic").unwrap();
        assert!(is_light_palette(parchment));
        assert!(!is_light_palette(classic));

        let deciduous = Tree {
            slot: 0,
            x: 0,
            shape: 0,
            tint: 7,
            foreground: false,
        };
        let (leaf, shade, trunk) = tree_colors(&deciduous, parchment);
        assert_eq!(leaf, mix(parchment.bodies[1], parchment.star, 0.06));
        assert_eq!(shade, mix(leaf, parchment.star, 0.22));
        assert_eq!(trunk, parchment.antenna);

        let conifer = Tree {
            slot: 1,
            x: 0,
            shape: 4,
            tint: 3,
            foreground: true,
        };
        let (leaf, _, trunk) = tree_colors(&conifer, classic);
        assert_eq!(leaf, mix(classic.bodies[5], classic.bg, 0.6));
        assert_eq!(trunk, mix(classic.antenna, classic.bg, 0.6));
    }
}
