//! Rooftop masts and beacons (Rebuild Prompt, "Deterministic Generation",
//! masts paragraph).
//!
//! The tower kinds the Python reference gives a structural mast (spire,
//! pointed, needle, TV tower, twin, onion, clock tower, pagoda) always get
//! one; other buildings at least 45% of `max_h` tall get one 30% of the
//! time (GitHub issue #2; owner decision, 2026-11-05: match Python).
//! Factories and the low-rise extension kinds never do. A mast is 4 to 16
//! cells long, clipped to the headroom under `max_h`, and is centered over
//! the highest tier.

use super::building::{BuildingKind, Mast, Tier};
use super::rng::Mulberry32;

/// Chance that an eligible tall building gets a mast.
pub const MAST_CHANCE: f64 = 0.3;
/// A building is "tall" when its roof reaches this fraction of `max_h`
/// (the Python reference's 45%; was 50% under assumption AS-R004).
pub const TALL_FRACTION: f64 = 0.45;
/// Inclusive mast length range in cells, before clipping to headroom.
pub const MAST_LENGTH: (i64, i64) = (4, 16);
/// Beacon blink period range in seconds.
pub const BEACON_PERIOD: (f64, f64) = (1.2, 2.5);

/// How a kind relates to masts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MastRule {
    /// Always gets a mast when there is headroom.
    Always,
    /// Gets one with [`MAST_CHANCE`] when tall.
    WhenTall,
    /// Never gets a mast.
    Never,
}

/// Mast rule for a kind: the Python reference's tower kinds always get one
/// (replacing assumption AS-R003, which had only needles and TV towers).
pub fn mast_rule(kind: BuildingKind) -> MastRule {
    use BuildingKind::*;
    match kind {
        Spire | Pointed | Needle | Tvtower | Twin | Onion | Clocktower | Pagoda => MastRule::Always,
        BuildingKind::Factory => MastRule::Never,
        kind if kind.is_low_rise() => MastRule::Never,
        _ => MastRule::WhenTall,
    }
}

/// Decides and places a mast for a building with these tiers.
///
/// Consumes RNG values only when the building is eligible: one roll for
/// tall buildings, then length, period, and phase. O(tiers).
pub fn plan_mast(
    kind: BuildingKind,
    tiers: &[Tier],
    max_h: i32,
    rng: &mut Mulberry32,
) -> Option<Mast> {
    // Highest tier; on a tie the first one wins, so results are stable.
    let top = tiers.iter().fold(None::<&Tier>, |best, tier| match best {
        Some(best) if best.h1 >= tier.h1 => Some(best),
        _ => Some(tier),
    })?;
    let roof = top.h1;
    match mast_rule(kind) {
        MastRule::Never => return None,
        MastRule::WhenTall => {
            if f64::from(roof) < f64::from(max_h) * TALL_FRACTION {
                return None;
            }
            if rng.next_f64() >= MAST_CHANCE {
                return None;
            }
        }
        MastRule::Always => {}
    }
    let length = rng.rand_int(MAST_LENGTH.0, MAST_LENGTH.1) as i32;
    let headroom = max_h - roof;
    if headroom <= 0 {
        return None;
    }
    let beacon_period = BEACON_PERIOD.0 + (BEACON_PERIOD.1 - BEACON_PERIOD.0) * rng.next_f64();
    let beacon_phase = rng.next_f64() * beacon_period;
    Some(Mast {
        x: top.x0 + (top.width() - 1) / 2,
        h0: roof,
        h1: roof + length.min(headroom),
        beacon_period,
        beacon_phase,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::building::{Building, WindowSet, KIND_WEIGHTS};
    use crate::model::shapes::build_tiers;

    #[test]
    fn rules_follow_spec_and_extensions() {
        use BuildingKind::*;
        for kind in [
            Spire, Pointed, Needle, Tvtower, Twin, Onion, Clocktower, Pagoda,
        ] {
            assert_eq!(mast_rule(kind), MastRule::Always, "{kind:?}");
        }
        for kind in [
            Stepped, Ziggurat, Gable, Arch, Slant, Crownblock, Watertank, Invpyramid, Jenga,
        ] {
            assert_eq!(mast_rule(kind), MastRule::WhenTall, "{kind:?}");
        }
        assert_eq!(mast_rule(BuildingKind::Factory), MastRule::Never);
        assert_eq!(mast_rule(BuildingKind::Warehouse), MastRule::Never);
        assert_eq!(mast_rule(BuildingKind::FiveOverOne), MastRule::Never);
        assert_eq!(mast_rule(BuildingKind::Taper), MastRule::WhenTall);
        assert_eq!(mast_rule(BuildingKind::Box), MastRule::WhenTall);
    }

    #[test]
    fn every_tower_kind_gets_a_mast_when_there_is_headroom() {
        use BuildingKind::*;
        for kind in [
            Spire, Pointed, Needle, Tvtower, Twin, Onion, Clocktower, Pagoda,
        ] {
            for seed in 0..10 {
                let mut rng = Mulberry32::new(seed);
                let (tiers, used) = build_tiers(kind, 0, 13, 40, 120, &mut rng);
                assert_eq!(used, kind);
                let mast = plan_mast(kind, &tiers, 120, &mut rng).expect("a mast");
                let roof = tiers.iter().map(|t| t.h1).max().unwrap();
                assert_eq!(mast.h0, roof, "{kind:?} on its top");
                assert!((4..=16).contains(&(mast.h1 - mast.h0)), "{kind:?}");
            }
        }
    }

    #[test]
    fn tall_means_45_percent_of_max_height() {
        // A 45-row box under max 100 may get a mast; a 44-row one never.
        let roll = |height: i32| {
            (0..200).any(|seed| {
                let tiers = vec![Tier::new(0, 9, 0, height).unwrap()];
                plan_mast(BuildingKind::Box, &tiers, 100, &mut Mulberry32::new(seed)).is_some()
            })
        };
        assert!(roll(45));
        assert!(!roll(44));
    }

    #[test]
    fn mast_is_centered_on_highest_tier_and_clipped() {
        let tiers = vec![
            Tier::new(0, 11, 0, 30).unwrap(),
            Tier::new(3, 8, 30, 40).unwrap(),
        ];
        let mut rng = Mulberry32::new(4);
        let mast = plan_mast(BuildingKind::Needle, &tiers, 44, &mut rng).unwrap();
        assert_eq!((mast.x, mast.h0), (5, 40));
        assert!(
            mast.h1 > 40 && mast.h1 <= 44,
            "clipped to headroom: {mast:?}"
        );
        assert!((1.2..2.5).contains(&mast.beacon_period));
        assert!((0.0..mast.beacon_period).contains(&mast.beacon_phase));

        let mut rng = Mulberry32::new(4);
        assert!(plan_mast(BuildingKind::Needle, &tiers, 40, &mut rng).is_none());
        assert!(plan_mast(BuildingKind::Box, &[], 40, &mut rng).is_none());
    }

    #[test]
    fn short_buildings_never_get_masts_and_tall_ones_about_thirty_percent() {
        let short = vec![Tier::new(0, 9, 0, 20).unwrap()];
        let tall = vec![Tier::new(0, 9, 0, 60).unwrap()];
        let mut rng = Mulberry32::new(17);
        let mut masted = 0;
        for _ in 0..10_000 {
            assert!(plan_mast(BuildingKind::Box, &short, 100, &mut rng).is_none());
            if plan_mast(BuildingKind::Box, &tall, 100, &mut rng).is_some() {
                masted += 1;
            }
        }
        let share = f64::from(masted) / 10_000.0;
        assert!((0.27..0.33).contains(&share), "mast share {share}");
    }

    #[test]
    fn planned_masts_always_validate() {
        let max_h = 120;
        for (kind, _) in KIND_WEIGHTS {
            for seed in 0..40 {
                let mut rng = Mulberry32::new(seed);
                let width = kind.min_width().max(12);
                let (tiers, used) = build_tiers(kind, 5, width, 90, max_h, &mut rng);
                let mast = plan_mast(used, &tiers, max_h, &mut rng);
                let building = Building {
                    layer: 1,
                    x0: 5,
                    x1: 5 + width,
                    kind: used,
                    tiers,
                    body_index: 0,
                    tone: 0,
                    accent_roll: 0.0,
                    windows: WindowSet::default(),
                    mast,
                    decoration: None,
                };
                assert_eq!(building.validate(max_h), Ok(()), "{kind:?} seed {seed}");
            }
        }
    }
}
