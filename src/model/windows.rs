//! Window placement, lighting targets, tones, and shades
//! (Rebuild Prompt, "Deterministic Generation", window patterns).
//!
//! Each building picks one pattern: horizontal bands (20%), vertical
//! stripes (next 22%), or a grid (the rest). Patterns are anchored to the
//! building footprint and the ground so columns and floors line up across
//! tiers. Dark tiers get no windows; solid tiers get dense stripes on
//! alternate rows.

use super::building::{Building, Tier, WindowSet};
use super::palette::DEPTH_FADES;
use super::rng::{js_round, Mulberry32};

/// Number of quantized shade levels; level `n` means a fade of `n / 15`.
pub const SHADE_LEVELS: u8 = 16;
/// Largest fade toward the background.
pub const MAX_SHADE: f64 = 0.9;
/// Target-on chance for dense (solid-tier) windows.
pub const DENSE_TARGET_CHANCE: f64 = 0.92;
/// Chance that a window has its own tone instead of the building's.
pub const INDEPENDENT_TONE_CHANCE: f64 = 0.12;
/// Extra fade on the right face of a tier at least four cells wide.
pub const RIGHT_FACE_SHADE: f64 = 0.22;
/// Fraction of a tier's width, from the right, that counts as right face.
pub const RIGHT_FACE_FRACTION: f64 = 0.38;
/// Extra fade where nearer geometry casts a shadow (standard mode).
pub const CAST_FADE: f64 = 0.3;

/// The three window pattern families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PatternKind {
    /// Continuous horizontal bands.
    Bands,
    /// One-column vertical stripes.
    Stripes,
    /// Separate windows in a grid.
    Grid,
}

/// Spacing and density of one building's windows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowPattern {
    /// Pattern family.
    pub kind: PatternKind,
    /// Column repeat in cells.
    pub x_pitch: i32,
    /// Window width in cells within each column repeat.
    pub x_width: i32,
    /// Row repeat in cells; windows are one row tall.
    pub y_pitch: i32,
    /// Chance that each window should end up lit.
    pub target_ratio: f64,
}

/// Chooses a building's window pattern. Consumes 2 to 4 RNG values.
pub fn choose_pattern(rng: &mut Mulberry32) -> WindowPattern {
    let roll = rng.next_f64();
    if roll < 0.20 {
        let y_pitch = if rng.next_f64() < 0.6 { 2 } else { 3 };
        WindowPattern {
            kind: PatternKind::Bands,
            x_pitch: 1,
            x_width: 1,
            y_pitch,
            target_ratio: ratio(rng, 0.70, 0.92),
        }
    } else if roll < 0.42 {
        WindowPattern {
            kind: PatternKind::Stripes,
            x_pitch: 2,
            x_width: 1,
            y_pitch: 1,
            target_ratio: ratio(rng, 0.70, 0.92),
        }
    } else {
        let x_pitch = if rng.next_f64() < 0.75 { 2 } else { 3 };
        let x_width = if x_pitch == 3 && rng.next_f64() < 0.5 {
            2
        } else {
            1
        };
        let y_pitch = if rng.next_f64() < 0.7 { 2 } else { 3 };
        WindowPattern {
            kind: PatternKind::Grid,
            x_pitch,
            x_width,
            y_pitch,
            target_ratio: ratio(rng, 0.45, 0.80),
        }
    }
}

fn ratio(rng: &mut Mulberry32, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * rng.next_f64()
}

/// Window fade before quantization:
/// `min(0.9, layerFade + 0.6 * (1 - h / max(1, roofH))^1.6 + rightFace)`.
///
/// `right_face` is 0.22 on the rightmost 38% of a tier at least four cells
/// wide, otherwise 0.
pub fn window_shade(layer: u8, h: i32, roof_h: i32, tier: &Tier, x: i32) -> f64 {
    let layer_fade = DEPTH_FADES[usize::from(layer.min(3))];
    let height_term = (1.0 - f64::from(h) / f64::from(roof_h.max(1))).max(0.0);
    let right_face = if tier.width() >= 4 && is_right_face(tier, x) {
        RIGHT_FACE_SHADE
    } else {
        0.0
    };
    (layer_fade + 0.6 * height_term.powf(1.6) + right_face).min(MAX_SHADE)
}

fn is_right_face(tier: &Tier, x: i32) -> bool {
    let face = (f64::from(tier.width()) * RIGHT_FACE_FRACTION).ceil() as i32;
    x >= tier.x1 - face
}

/// Quantizes a fade in `0..=0.9` to one of the 16 shade levels.
pub fn quantize_shade(shade: f64) -> u8 {
    (js_round(shade.clamp(0.0, 1.0) * f64::from(SHADE_LEVELS - 1)) as u8).min(SHADE_LEVELS - 1)
}

/// The fade represented by a shade level.
pub fn shade_from_level(level: u8) -> f64 {
    f64::from(level.min(SHADE_LEVELS - 1)) / f64::from(SHADE_LEVELS - 1)
}

/// Adds the standard-mode cast fade to a quantized shade, capped at 0.9.
pub fn with_cast_fade(level: u8) -> u8 {
    quantize_shade((shade_from_level(level) + CAST_FADE).min(MAX_SHADE))
}

/// Places windows on every non-dark tier of `building`, replacing any
/// existing windows. Consumes RNG values per window (target and tone).
///
/// Rules: no windows on the ground row; none on exposed top edges of
/// ordinary tiers; none on exterior side columns when the tier is at least
/// four cells wide; each cell belongs to the topmost tier covering it.
/// O(footprint area × tiers).
pub fn generate_windows(building: &Building, rng: &mut Mulberry32) -> WindowSet {
    let pattern = choose_pattern(rng);
    let roof_h = building.roof_height();
    let mut windows = WindowSet::default();
    for (index, tier) in building.tiers.iter().enumerate() {
        if tier.dark {
            continue;
        }
        for h in tier.h0.max(1)..tier.h1 {
            for x in tier.x0..tier.x1 {
                if owner(&building.tiers, x, h) != Some(index) {
                    continue;
                }
                let target_chance = if tier.solid {
                    if (h - tier.h0) % 2 != 0 {
                        continue;
                    }
                    DENSE_TARGET_CHANCE
                } else {
                    let exposed_top = !building.covers(x, h + 1);
                    let side_edge = tier.width() >= 4
                        && (!building.covers(x - 1, h) || !building.covers(x + 1, h));
                    let column =
                        (x - building.x0 - 1).rem_euclid(pattern.x_pitch) < pattern.x_width;
                    let row = (h - 1).rem_euclid(pattern.y_pitch) == 0;
                    if exposed_top || side_edge || !column || !row {
                        continue;
                    }
                    pattern.target_ratio
                };
                let target = rng.next_f64() < target_chance;
                let tone = if rng.next_f64() < INDEPENDENT_TONE_CHANCE {
                    rng.rand_int(0, 255) as u8
                } else {
                    building.tone
                };
                let shade = quantize_shade(window_shade(building.layer, h, roof_h, tier, x));
                windows.push(x, h, shade, tone, target);
            }
        }
    }
    windows
}

/// Index of the topmost (last) tier covering `(x, h)`.
fn owner(tiers: &[Tier], x: i32, h: i32) -> Option<usize> {
    tiers.iter().rposition(|tier| tier.contains(x, h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::building::{BuildingKind, KIND_WEIGHTS};
    use crate::model::shapes::build_tiers;
    use std::collections::HashMap;

    fn building_for(kind: BuildingKind, width: i32, height: i32, seed: u32) -> Building {
        let mut rng = Mulberry32::new(seed);
        let (tiers, used) = build_tiers(kind, 3, width, height, 191, &mut rng);
        Building {
            layer: (seed % 4) as u8,
            x0: 3,
            x1: 3 + width,
            kind: used,
            tiers,
            body_index: 0,
            tone: 77,
            accent_roll: 0.0,
            windows: WindowSet::default(),
            mast: None,
            decoration: None,
        }
    }

    #[test]
    fn pattern_mix_matches_spec_shares() {
        let mut rng = Mulberry32::new(31);
        let mut counts: HashMap<PatternKind, u32> = HashMap::new();
        for _ in 0..20_000 {
            let pattern = choose_pattern(&mut rng);
            *counts.entry(pattern.kind).or_default() += 1;
            match pattern.kind {
                PatternKind::Bands => {
                    assert_eq!((pattern.x_pitch, pattern.x_width), (1, 1));
                    assert!([2, 3].contains(&pattern.y_pitch));
                    assert!((0.70..0.92).contains(&pattern.target_ratio));
                }
                PatternKind::Stripes => {
                    assert_eq!(
                        (pattern.x_pitch, pattern.x_width, pattern.y_pitch),
                        (2, 1, 1)
                    );
                    assert!((0.70..0.92).contains(&pattern.target_ratio));
                }
                PatternKind::Grid => {
                    assert!([2, 3].contains(&pattern.x_pitch));
                    assert!(pattern.x_width == 1 || (pattern.x_pitch, pattern.x_width) == (3, 2));
                    assert!([2, 3].contains(&pattern.y_pitch));
                    assert!((0.45..0.80).contains(&pattern.target_ratio));
                }
            }
        }
        let share = |kind| f64::from(counts[&kind]) / 20_000.0;
        assert!((0.18..0.22).contains(&share(PatternKind::Bands)));
        assert!((0.20..0.24).contains(&share(PatternKind::Stripes)));
        assert!((0.56..0.60).contains(&share(PatternKind::Grid)));
    }

    #[test]
    fn windows_obey_placement_rules_for_every_kind() {
        for (kind, _) in KIND_WEIGHTS {
            for seed in 0..12 {
                let width = kind.min_width().max(14);
                let mut building = building_for(kind, width, 70, seed);
                let mut rng = Mulberry32::new(seed + 100);
                building.windows = generate_windows(&building, &mut rng);
                assert_eq!(building.validate(191), Ok(()), "{kind:?} seed {seed}");
                for i in 0..building.windows.len() {
                    let (x, h) = (building.windows.x[i], building.windows.h[i]);
                    assert!(h >= 1, "{kind:?}: window on the ground row");
                    let tier = building.tiers[owner(&building.tiers, x, h).unwrap()];
                    assert!(!tier.dark, "{kind:?}: window in a dark tier");
                    if !tier.solid {
                        assert!(building.covers(x, h + 1), "{kind:?}: window on roof edge");
                    }
                    assert!(building.windows.shade[i] < SHADE_LEVELS);
                }
            }
        }
    }

    #[test]
    fn solid_tiers_get_dense_lit_stripes() {
        let building = building_for(BuildingKind::ParkingGarage, 20, 40, 2);
        let mut rng = Mulberry32::new(9);
        let windows = generate_windows(&building, &mut rng);
        let lit_share = windows.target_count() as f64 / windows.len() as f64;
        assert!(windows.len() > 40, "decks are densely windowed");
        assert!(lit_share > 0.8, "dense windows mostly lit: {lit_share}");
    }

    #[test]
    fn about_twelve_percent_of_windows_have_their_own_tone() {
        let mut own = 0;
        let mut total = 0;
        for seed in 0..40 {
            let building = building_for(BuildingKind::Box, 20, 80, seed);
            let windows = generate_windows(&building, &mut Mulberry32::new(seed));
            total += windows.len();
            own += windows.tone.iter().filter(|&&tone| tone != 77).count();
        }
        let share = own as f64 / total as f64;
        // An independent tone equals 77 one time in 256, so the share is
        // slightly below 12%.
        assert!(
            (0.10..0.13).contains(&share),
            "independent tone share {share}"
        );
    }

    #[test]
    fn shade_formula_and_quantization() {
        let tier = Tier::new(0, 10, 0, 50).unwrap();
        // Layer 3 (no depth fade), at the roof, left face: no shade at all.
        assert_eq!(window_shade(3, 50, 50, &tier, 0), 0.0);
        // Right face (rightmost ceil(3.8) = 4 columns) adds 0.22.
        assert!((window_shade(3, 50, 50, &tier, 6) - 0.22).abs() < 1e-12);
        assert_eq!(window_shade(3, 50, 50, &tier, 5), 0.0);
        // Ground level in the farthest layer is capped at 0.9.
        assert_eq!(window_shade(0, 0, 50, &tier, 9), MAX_SHADE);
        // Narrow tiers have no right face.
        let narrow = Tier::new(0, 3, 0, 50).unwrap();
        assert_eq!(window_shade(3, 50, 50, &narrow, 2), 0.0);

        assert_eq!(quantize_shade(0.0), 0);
        assert_eq!(quantize_shade(MAX_SHADE), 14); // round(13.5) = 14
        assert_eq!(quantize_shade(1.0), 15);
        assert_eq!(shade_from_level(15), 1.0);
        assert_eq!(with_cast_fade(0), quantize_shade(0.3));
        assert_eq!(with_cast_fade(14), 14, "cast fade is capped at 0.9");
    }

    #[test]
    fn same_seed_gives_same_windows() {
        let building = building_for(BuildingKind::Stepped, 15, 60, 6);
        let a = generate_windows(&building, &mut Mulberry32::new(5));
        let b = generate_windows(&building, &mut Mulberry32::new(5));
        assert_eq!(a, b);
        assert!(!a.is_empty());
    }
}
