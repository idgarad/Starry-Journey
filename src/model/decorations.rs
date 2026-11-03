//! Landmark decorations: lit outlines on tall, wide buildings
//! (Rebuild Prompt, "Deterministic Generation", landmark decoration).
//!
//! A building is eligible when its main (widest grounded) tier is at least
//! five cells wide and its roof reaches 40% of `max_h`. Eligible buildings
//! are decorated 6% of the time; a Normal-mode city later makes sure it has
//! one to three landmarks. Every outline point lies on the building.

use super::building::{Building, Decoration, DecorationKind, LightMode, Tier};
use super::rng::{js_round, Mulberry32};

/// Chance that an eligible building is decorated during generation.
pub const DECORATION_CHANCE: f64 = 0.06;
/// Minimum main-tier width for a decoration.
pub const MIN_MAIN_WIDTH: i32 = 5;
/// Minimum roof height, as a fraction of `max_h`, for a decoration.
pub const MIN_HEIGHT_FRACTION: f64 = 0.4;
/// Rows of each side included in a crown outline, below the top row.
const CROWN_SIDE_ROWS: i32 = 2;

const KINDS: [DecorationKind; 4] = [
    DecorationKind::Crown,
    DecorationKind::Band,
    DecorationKind::Spire,
    DecorationKind::Roofline,
];

/// The widest tier that touches the ground (the first on a tie).
pub fn main_tier(building: &Building) -> Option<&Tier> {
    building
        .tiers
        .iter()
        .filter(|tier| tier.h0 == 0)
        .fold(None, |best: Option<&Tier>, tier| match best {
            Some(best) if best.width() >= tier.width() => Some(best),
            _ => Some(tier),
        })
}

/// True if `building` may carry a landmark decoration.
pub fn is_eligible(building: &Building, max_h: i32) -> bool {
    let wide = main_tier(building).is_some_and(|tier| tier.width() >= MIN_MAIN_WIDTH);
    let tall = f64::from(building.roof_height()) >= f64::from(max_h) * MIN_HEIGHT_FRACTION;
    wide && tall
}

/// Generation-time decoration: for eligible buildings, a 6% roll, then a
/// decoration. Consumes RNG values only when eligible.
pub fn maybe_decorate(building: &Building, max_h: i32, rng: &mut Mulberry32) -> Option<Decoration> {
    if !is_eligible(building, max_h) || rng.next_f64() >= DECORATION_CHANCE {
        return None;
    }
    make_decoration(building, rng)
}

/// Builds a decoration for `building` without the chance roll: random
/// outline kind, light mode, and hue. Returns `None` if the building has
/// no tiers. Consumes 3 to 4 RNG values.
pub fn make_decoration(building: &Building, rng: &mut Mulberry32) -> Option<Decoration> {
    let kind = KINDS[rng.rand_int(0, KINDS.len() as i64 - 1) as usize];
    let mode = if rng.next_f64() < 0.5 {
        LightMode::Cycle
    } else {
        LightMode::Chase
    };
    let hue = rng.next_f64() * 360.0;
    let points = match kind {
        DecorationKind::Crown => crown(highest_tier(building)?),
        DecorationKind::Band => band(main_tier(building)?, rng),
        DecorationKind::Spire => spire(highest_tier(building)?),
        DecorationKind::Roofline => roofline(building),
    };
    (!points.is_empty()).then_some(Decoration {
        kind,
        mode,
        hue,
        points,
    })
}

/// The highest tier (the first on a tie).
fn highest_tier(building: &Building) -> Option<&Tier> {
    building
        .tiers
        .iter()
        .fold(None, |best: Option<&Tier>, tier| match best {
            Some(best) if best.h1 >= tier.h1 => Some(best),
            _ => Some(tier),
        })
}

fn crown(tier: &Tier) -> Vec<i32> {
    let top = tier.h1 - 1;
    let mut points = Vec::new();
    for x in tier.x0..tier.x1 {
        points.extend([x, top]);
    }
    let lowest = (top - CROWN_SIDE_ROWS).max(tier.h0);
    for h in (lowest..top).rev() {
        points.extend([tier.x0, h]);
        if tier.width() > 1 {
            points.extend([tier.x1 - 1, h]);
        }
    }
    points
}

fn band(tier: &Tier, rng: &mut Mulberry32) -> Vec<i32> {
    let fraction = 0.55 + 0.25 * rng.next_f64();
    let row = tier.h0 + js_round(f64::from(tier.height()) * fraction) as i32;
    let row = row.clamp(tier.h0, tier.h1 - 1);
    (tier.x0..tier.x1).flat_map(|x| [x, row]).collect()
}

fn spire(tier: &Tier) -> Vec<i32> {
    let x = tier.x0 + (tier.width() - 1) / 2;
    (tier.h0..tier.h1).flat_map(|h| [x, h]).collect()
}

fn roofline(building: &Building) -> Vec<i32> {
    (building.x0..building.x1)
        .filter_map(|x| {
            building
                .tiers
                .iter()
                .filter(|tier| (tier.x0..tier.x1).contains(&x))
                .map(|tier| tier.h1 - 1)
                .max()
                .map(|top| [x, top])
        })
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::building::{BuildingKind, WindowSet, KIND_WEIGHTS};
    use crate::model::shapes::build_tiers;

    fn building(kind: BuildingKind, width: i32, height: i32, seed: u32) -> Building {
        let mut rng = Mulberry32::new(seed);
        let (tiers, used) = build_tiers(kind, -4, width, height, 100, &mut rng);
        Building {
            layer: 1,
            x0: -4,
            x1: -4 + width,
            kind: used,
            tiers,
            body_index: 0,
            tone: 0,
            accent_roll: 0.0,
            windows: WindowSet::default(),
            mast: None,
            decoration: None,
        }
    }

    #[test]
    fn eligibility_needs_width_and_height() {
        assert!(is_eligible(&building(BuildingKind::Box, 5, 40, 0), 100));
        assert!(
            !is_eligible(&building(BuildingKind::Box, 4, 90, 0), 100),
            "too narrow"
        );
        assert!(
            !is_eligible(&building(BuildingKind::Box, 12, 39, 0), 100),
            "too short"
        );
        // A needle's widest grounded tier is its 2- or 3-cell shaft.
        assert!(!is_eligible(
            &building(BuildingKind::Needle, 12, 90, 0),
            100
        ));
    }

    #[test]
    fn every_decoration_lies_on_its_building() {
        for (kind, _) in KIND_WEIGHTS {
            for seed in 0..30 {
                let mut b = building(kind, kind.min_width().max(12), 80, seed);
                let mut rng = Mulberry32::new(seed + 7);
                b.decoration = make_decoration(&b, &mut rng);
                let decoration = b.decoration.as_ref().expect("buildings have tiers");
                assert!(!decoration.points.is_empty());
                assert!((0.0..360.0).contains(&decoration.hue));
                assert_eq!(
                    b.validate(100),
                    Ok(()),
                    "{kind:?} seed {seed} {:?}",
                    decoration.kind
                );
            }
        }
    }

    #[test]
    fn outlines_have_expected_geometry() {
        let tower = building(BuildingKind::Box, 9, 50, 0);
        let crown_points = crown(&tower.tiers[0]);
        assert_eq!(crown_points.len() / 2, 9 + 2 * CROWN_SIDE_ROWS as usize);
        let spire_points = spire(&tower.tiers[0]);
        assert!(
            spire_points.chunks(2).all(|p| p[0] == 0),
            "center column of x -4..5"
        );
        assert_eq!(spire_points.len() / 2, 50);
        let roof = roofline(&tower);
        assert!(roof.chunks(2).all(|p| p[1] == 49));
        assert_eq!(roof.len() / 2, 9);

        let twin = building(BuildingKind::Twin, 15, 60, 3);
        let roof = roofline(&twin);
        assert!(roof.len() / 2 <= 15, "gap columns have no roof point");
    }

    #[test]
    fn generation_decorates_about_six_percent_of_eligible_buildings() {
        let tall = building(BuildingKind::Box, 10, 60, 0);
        let short = building(BuildingKind::Box, 10, 10, 0);
        let mut rng = Mulberry32::new(12);
        let mut decorated = 0;
        for _ in 0..20_000 {
            assert!(maybe_decorate(&short, 100, &mut rng).is_none());
            if maybe_decorate(&tall, 100, &mut rng).is_some() {
                decorated += 1;
            }
        }
        let share = f64::from(decorated) / 20_000.0;
        assert!((0.05..0.07).contains(&share), "decoration share {share}");
    }

    #[test]
    fn all_kinds_and_modes_occur() {
        let tower = building(BuildingKind::Stepped, 12, 60, 1);
        let mut rng = Mulberry32::new(2);
        let mut kinds = std::collections::HashSet::new();
        let mut modes = std::collections::HashSet::new();
        for _ in 0..200 {
            let decoration = make_decoration(&tower, &mut rng).unwrap();
            kinds.insert(decoration.kind);
            modes.insert(decoration.mode);
        }
        assert_eq!((kinds.len(), modes.len()), (4, 2));
    }
}
