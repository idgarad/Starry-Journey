//! Building, tier, and window records (Rebuild Prompt, "Portable records" and
//! "Deterministic Generation").
//!
//! These are plain data plus invariant checks. Generators in later modules
//! fill them in; the rasterizer reads them. Masts and decorations are added
//! by their own features.

use super::rng::Mulberry32;

/// Building silhouette kinds.
///
/// The first 19 come from the specification. The rest are Starry Journey
/// extensions (see `docs/product.md`, "Expanded buildings").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuildingKind {
    /// Plain slab.
    Box,
    /// Setback tower.
    Stepped,
    /// Setbacks topped by a spire.
    Spire,
    /// Pyramidal roof.
    Pointed,
    /// Many shallow setbacks.
    Ziggurat,
    /// Gabled roof.
    Gable,
    /// Rounded roof.
    Arch,
    /// Slanted roof.
    Slant,
    /// Block with a bright crown.
    Crownblock,
    /// Needle with an observation deck.
    Needle,
    /// Spherical TV tower.
    Tvtower,
    /// Linked twin towers.
    Twin,
    /// Onion dome.
    Onion,
    /// Rooftop water tank.
    Watertank,
    /// Inverted pyramid.
    Invpyramid,
    /// Clock tower.
    Clocktower,
    /// Offset stack.
    Jenga,
    /// Sawtooth factory.
    Factory,
    /// Pagoda eaves.
    Pagoda,
    /// Extension: lit retail podium with five residential floors above.
    FiveOverOne,
    /// Extension: wide, low warehouse with loading docks.
    Warehouse,
    /// Extension: half-cylinder Quonset hut or hangar.
    Quonset,
    /// Extension: grain silos or tanks beside a taller headhouse.
    Silos,
    /// Extension: big-box store with a lit sign.
    BigBox,
    /// Extension: parking garage with lit open decks.
    ParkingGarage,
    /// Extension: glass tower tapering steadily upward.
    Taper,
    /// Extension: tower with an upper block overhanging one side.
    Cantilever,
    /// Extension: setbacks on one side only.
    Terraced,
}

/// Every kind with its weight: the 19 specification kinds (total 143),
/// then the extensions (total 16, about 10% of all picks in the nearest
/// layer).
pub const KIND_WEIGHTS: [(BuildingKind, u32); 28] = [
    (BuildingKind::Box, 18),
    (BuildingKind::Stepped, 18),
    (BuildingKind::Spire, 12),
    (BuildingKind::Pointed, 12),
    (BuildingKind::Ziggurat, 6),
    (BuildingKind::Gable, 8),
    (BuildingKind::Arch, 8),
    (BuildingKind::Slant, 8),
    (BuildingKind::Crownblock, 10),
    (BuildingKind::Needle, 4),
    (BuildingKind::Tvtower, 3),
    (BuildingKind::Twin, 5),
    (BuildingKind::Onion, 4),
    (BuildingKind::Watertank, 5),
    (BuildingKind::Invpyramid, 4),
    (BuildingKind::Clocktower, 4),
    (BuildingKind::Jenga, 5),
    (BuildingKind::Factory, 5),
    (BuildingKind::Pagoda, 4),
    (BuildingKind::FiveOverOne, 3),
    (BuildingKind::Warehouse, 2),
    (BuildingKind::Quonset, 1),
    (BuildingKind::Silos, 1),
    (BuildingKind::BigBox, 2),
    (BuildingKind::ParkingGarage, 1),
    (BuildingKind::Taper, 2),
    (BuildingKind::Cantilever, 2),
    (BuildingKind::Terraced, 2),
];

/// Number of kinds defined by the specification (the first entries of
/// [`KIND_WEIGHTS`]).
pub const SPEC_KIND_COUNT: usize = 19;

/// Index of the nearest (road-side) layer.
pub const NEAR_LAYER: u8 = 3;

impl BuildingKind {
    /// True for kinds added by Starry Journey beyond the specification.
    pub fn is_extension(self) -> bool {
        KIND_WEIGHTS
            .iter()
            .position(|(kind, _)| *kind == self)
            .is_some_and(|index| index >= SPEC_KIND_COUNT)
    }

    /// True for wide, low buildings that belong beside the road.
    pub fn is_low_rise(self) -> bool {
        use BuildingKind::*;
        matches!(
            self,
            Warehouse | Quonset | Silos | BigBox | ParkingGarage | FiveOverOne
        )
    }

    /// Whether this kind may be generated in `layer` (0 = farthest).
    ///
    /// Low-rise extensions stay in the nearest layer; 5-over-1 apartments
    /// may also appear one layer back.
    pub fn allowed_in_layer(self, layer: u8) -> bool {
        match self {
            BuildingKind::FiveOverOne => layer >= NEAR_LAYER - 1,
            kind if kind.is_low_rise() => layer == NEAR_LAYER,
            _ => true,
        }
    }

    /// Sum of the weights of kinds allowed in `layer`.
    pub fn total_weight(layer: u8) -> u32 {
        KIND_WEIGHTS
            .iter()
            .filter(|(kind, _)| kind.allowed_in_layer(layer))
            .map(|(_, weight)| weight)
            .sum()
    }

    /// Picks a kind allowed in `layer` by weight, consuming one RNG value.
    /// O(number of kinds).
    pub fn pick(rng: &mut Mulberry32, layer: u8) -> Self {
        let target = rng.next_f64() * f64::from(Self::total_weight(layer));
        let mut cumulative = 0.0;
        let mut last = BuildingKind::Box;
        for (kind, weight) in KIND_WEIGHTS {
            if !kind.allowed_in_layer(layer) {
                continue;
            }
            cumulative += f64::from(weight);
            last = kind;
            if target < cumulative {
                return kind;
            }
        }
        last
    }

    /// Smallest footprint width (cells) at which this kind keeps its shape.
    ///
    /// Generic setback kinds need 7, gable/arch/slant need 5, and landmarks
    /// need 7, 9, or 10 (assumption AS-R002 lists which kinds count as
    /// generic setbacks).
    pub fn min_width(self) -> i32 {
        use BuildingKind::*;
        match self {
            Box => 1,
            Gable | Arch | Slant => 5,
            Stepped | Spire | Pointed | Ziggurat | Crownblock => 7,
            Tvtower | Onion | Watertank | Clocktower => 7,
            Needle | Twin | Invpyramid | Jenga | Pagoda => 9,
            Factory => 10,
            Taper | Terraced => 7,
            Cantilever | Quonset => 9,
            Warehouse | Silos | ParkingGarage => 10,
            FiveOverOne | BigBox => 12,
        }
    }

    /// The kind's own footprint width range in `layer`, if it overrides the
    /// layer's normal width sampling. Near-road low-rise buildings (and
    /// factories in the nearest layer) may be up to 50 cells wide.
    pub fn own_width_range(self, layer: u8) -> Option<(i64, i64)> {
        use BuildingKind::*;
        match self {
            FiveOverOne => Some((16, 40)),
            Warehouse | BigBox => Some((20, 50)),
            Quonset | Silos => Some((12, 30)),
            ParkingGarage => Some((16, 40)),
            Factory if layer == NEAR_LAYER => Some((10, 50)),
            _ => None,
        }
    }

    /// Returns this kind, or [`BuildingKind::Box`] when `width` is too
    /// narrow for it.
    pub fn fit_to_width(self, width: i32) -> Self {
        if width >= self.min_width() {
            self
        } else {
            BuildingKind::Box
        }
    }
}

/// One rectangular tier: `[x0, x1) x [h0, h1)` in world columns and height
/// cells above ground.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tier {
    /// First column (inclusive).
    pub x0: i32,
    /// End column (exclusive).
    pub x1: i32,
    /// Bottom height (inclusive); 0 is the ground row.
    pub h0: i32,
    /// Top height (exclusive).
    pub h1: i32,
    /// Densely lit (for example a bright crown).
    pub solid: bool,
    /// Has no windows.
    pub dark: bool,
}

impl Tier {
    /// Creates a plain tier, or `None` if it would be empty.
    pub fn new(x0: i32, x1: i32, h0: i32, h1: i32) -> Option<Self> {
        (x1 > x0 && h1 > h0).then_some(Self {
            x0,
            x1,
            h0,
            h1,
            solid: false,
            dark: false,
        })
    }

    /// Width in columns.
    pub fn width(&self) -> i32 {
        self.x1 - self.x0
    }

    /// Height in cells.
    pub fn height(&self) -> i32 {
        self.h1 - self.h0
    }

    /// True if the cell `(x, h)` lies inside this tier.
    pub fn contains(&self, x: i32, h: i32) -> bool {
        (self.x0..self.x1).contains(&x) && (self.h0..self.h1).contains(&h)
    }

    /// True if this tier rests on the ground or on top of `other`, sharing
    /// at least one column.
    fn rests_on(&self, other: &Tier) -> bool {
        other.h1 == self.h0 && other.x0 < self.x1 && self.x0 < other.x1
    }
}

/// Window cells of one building, stored as parallel arrays so the rasterizer
/// can scan them cheaply. Index `i` describes one window across all arrays.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowSet {
    /// World column.
    pub x: Vec<i32>,
    /// Height above ground.
    pub h: Vec<i32>,
    /// Quantized shade byte (fade toward the background).
    pub shade: Vec<u8>,
    /// Palette-independent tone byte, mapped to a color by the palette.
    pub tone: Vec<u8>,
    /// Whether this window should end up lit.
    pub target: Vec<bool>,
    /// Whether this window is lit now.
    pub lit: Vec<bool>,
}

impl WindowSet {
    /// Adds one window, initially unlit.
    pub fn push(&mut self, x: i32, h: i32, shade: u8, tone: u8, target: bool) {
        self.x.push(x);
        self.h.push(h);
        self.shade.push(shade);
        self.tone.push(tone);
        self.target.push(target);
        self.lit.push(false);
    }

    /// Number of windows.
    pub fn len(&self) -> usize {
        self.x.len()
    }

    /// True when there are no windows.
    pub fn is_empty(&self) -> bool {
        self.x.is_empty()
    }

    /// Number of windows that should end up lit. O(n).
    pub fn target_count(&self) -> usize {
        self.target.iter().filter(|&&on| on).count()
    }

    /// Number of windows lit now. O(n).
    pub fn lit_count(&self) -> usize {
        self.lit.iter().filter(|&&on| on).count()
    }

    /// Lights every window whose target is on, as in a fully built city.
    pub fn light_to_target(&mut self) {
        self.lit.clone_from(&self.target);
    }

    /// True if all six arrays have the same length.
    pub fn is_consistent(&self) -> bool {
        let n = self.x.len();
        [
            self.h.len(),
            self.shade.len(),
            self.tone.len(),
            self.target.len(),
            self.lit.len(),
        ]
        .iter()
        .all(|&len| len == n)
    }
}

/// One generated building.
#[derive(Debug, Clone, PartialEq)]
pub struct Building {
    /// Parallax layer, 0 (farthest) to 3 (nearest).
    pub layer: u8,
    /// Footprint start column (inclusive).
    pub x0: i32,
    /// Footprint end column (exclusive).
    pub x1: i32,
    /// Silhouette kind after any width fallback.
    pub kind: BuildingKind,
    /// Tiers, from the base upward.
    pub tiers: Vec<Tier>,
    /// Index into the palette's six body colors.
    pub body_index: u8,
    /// Building-wide window tone byte.
    pub tone: u8,
    /// Roll compared with the palette's accent chance.
    pub accent_roll: f64,
    /// Window cells.
    pub windows: WindowSet,
}

/// A broken building invariant, reported by [`Building::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildingError {
    /// The footprint is empty.
    EmptyFootprint,
    /// The layer is outside 0..=3.
    BadLayer(u8),
    /// The body index is outside 0..=5.
    BadBodyIndex(u8),
    /// The building has no tiers.
    NoTiers,
    /// Tier `index` is empty or extends outside the footprint.
    TierOutsideFootprint(usize),
    /// Tier `index` reaches above the height limit.
    TierTooTall(usize),
    /// Tier `index` neither touches the ground nor rests on another tier.
    TierUnsupported(usize),
    /// Window `index` is not inside any tier.
    WindowOutsideTiers(usize),
    /// The window arrays have different lengths.
    InconsistentWindows,
}

impl Building {
    /// Footprint width in columns.
    pub fn width(&self) -> i32 {
        self.x1 - self.x0
    }

    /// Highest tier top, or 0 without tiers.
    pub fn roof_height(&self) -> i32 {
        self.tiers.iter().map(|tier| tier.h1).max().unwrap_or(0)
    }

    /// True if any tier covers the cell `(x, h)`. O(tiers).
    pub fn covers(&self, x: i32, h: i32) -> bool {
        self.tiers.iter().any(|tier| tier.contains(x, h))
    }

    /// Checks the structural invariants from the specification against a
    /// height limit `max_h`. O(tiers² + windows × tiers).
    ///
    /// # Errors
    ///
    /// Returns the first broken invariant found.
    pub fn validate(&self, max_h: i32) -> Result<(), BuildingError> {
        if self.x1 <= self.x0 {
            return Err(BuildingError::EmptyFootprint);
        }
        if self.layer > 3 {
            return Err(BuildingError::BadLayer(self.layer));
        }
        if usize::from(self.body_index) >= super::palette::BODY_COUNT {
            return Err(BuildingError::BadBodyIndex(self.body_index));
        }
        if self.tiers.is_empty() {
            return Err(BuildingError::NoTiers);
        }
        for (index, tier) in self.tiers.iter().enumerate() {
            if tier.width() <= 0 || tier.height() <= 0 || tier.x0 < self.x0 || tier.x1 > self.x1 {
                return Err(BuildingError::TierOutsideFootprint(index));
            }
            if tier.h0 < 0 || tier.h1 > max_h {
                return Err(BuildingError::TierTooTall(index));
            }
            let supported = tier.h0 == 0 || self.tiers.iter().any(|other| tier.rests_on(other));
            if !supported {
                return Err(BuildingError::TierUnsupported(index));
            }
        }
        if !self.windows.is_consistent() {
            return Err(BuildingError::InconsistentWindows);
        }
        for index in 0..self.windows.len() {
            if !self.covers(self.windows.x[index], self.windows.h[index]) {
                return Err(BuildingError::WindowOutsideTiers(index));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn building(tiers: Vec<Tier>) -> Building {
        Building {
            layer: 2,
            x0: 10,
            x1: 20,
            kind: BuildingKind::Box,
            tiers,
            body_index: 0,
            tone: 0,
            accent_roll: 0.5,
            windows: WindowSet::default(),
        }
    }

    #[test]
    fn kind_weights_keep_spec_total_and_add_about_ten_percent() {
        let spec: u32 = KIND_WEIGHTS[..SPEC_KIND_COUNT].iter().map(|(_, w)| w).sum();
        let extension: u32 = KIND_WEIGHTS[SPEC_KIND_COUNT..].iter().map(|(_, w)| w).sum();
        assert_eq!(spec, 143, "specification weights are unchanged");
        assert_eq!(extension, 16);
        let share = f64::from(extension) / f64::from(spec + extension);
        assert!((0.09..0.11).contains(&share), "extension share {share}");
        let unique: std::collections::HashSet<_> =
            KIND_WEIGHTS.iter().map(|(kind, _)| kind).collect();
        assert_eq!(unique.len(), KIND_WEIGHTS.len());
        assert!(!BuildingKind::Factory.is_extension());
        assert!(BuildingKind::Warehouse.is_extension());
    }

    #[test]
    fn layer_eligibility_keeps_low_rise_near_the_road() {
        assert_eq!(BuildingKind::total_weight(NEAR_LAYER), 159);
        assert_eq!(BuildingKind::total_weight(2), 152); // + 5-over-1 + towers
        assert_eq!(BuildingKind::total_weight(0), 149); // + towers only
        assert!(BuildingKind::Warehouse.allowed_in_layer(3));
        assert!(!BuildingKind::Warehouse.allowed_in_layer(2));
        assert!(BuildingKind::FiveOverOne.allowed_in_layer(2));
        assert!(!BuildingKind::FiveOverOne.allowed_in_layer(1));
        assert!(BuildingKind::Taper.allowed_in_layer(0));
        let mut rng = Mulberry32::new(8);
        for _ in 0..5_000 {
            assert!(!BuildingKind::pick(&mut rng, 0).is_low_rise());
        }
    }

    #[test]
    fn own_width_ranges_cover_wide_low_rise() {
        assert_eq!(BuildingKind::Warehouse.own_width_range(3), Some((20, 50)));
        assert_eq!(BuildingKind::Factory.own_width_range(3), Some((10, 50)));
        assert_eq!(BuildingKind::Factory.own_width_range(1), None);
        assert_eq!(BuildingKind::Box.own_width_range(3), None);
        for (kind, _) in KIND_WEIGHTS {
            if let Some((lo, hi)) = kind.own_width_range(NEAR_LAYER) {
                assert!(lo >= i64::from(kind.min_width()), "{kind:?}");
                assert!(hi <= 50, "{kind:?}");
            }
        }
    }

    #[test]
    fn kind_pick_is_deterministic_and_roughly_weighted() {
        let mut a = Mulberry32::new(42);
        let mut b = Mulberry32::new(42);
        let first: Vec<_> = (0..50).map(|_| BuildingKind::pick(&mut a, 3)).collect();
        let second: Vec<_> = (0..50).map(|_| BuildingKind::pick(&mut b, 3)).collect();
        assert_eq!(first, second);

        let mut rng = Mulberry32::new(1);
        let draws = 159_000;
        let mut counts: HashMap<BuildingKind, u32> = HashMap::new();
        for _ in 0..draws {
            *counts
                .entry(BuildingKind::pick(&mut rng, NEAR_LAYER))
                .or_default() += 1;
        }
        for (kind, weight) in KIND_WEIGHTS {
            let expected = f64::from(weight) * 1000.0;
            let actual = f64::from(counts[&kind]);
            assert!(
                (actual - expected).abs() < expected * 0.15,
                "{kind:?}: expected about {expected}, got {actual}"
            );
        }
    }

    #[test]
    fn narrow_footprints_fall_back_to_box() {
        assert_eq!(BuildingKind::Gable.fit_to_width(5), BuildingKind::Gable);
        assert_eq!(BuildingKind::Gable.fit_to_width(4), BuildingKind::Box);
        assert_eq!(BuildingKind::Stepped.fit_to_width(6), BuildingKind::Box);
        assert_eq!(BuildingKind::Onion.fit_to_width(7), BuildingKind::Onion);
        assert_eq!(BuildingKind::Needle.fit_to_width(8), BuildingKind::Box);
        assert_eq!(BuildingKind::Pagoda.fit_to_width(9), BuildingKind::Pagoda);
        assert_eq!(BuildingKind::Factory.fit_to_width(9), BuildingKind::Box);
        assert_eq!(
            BuildingKind::Factory.fit_to_width(10),
            BuildingKind::Factory
        );
        assert_eq!(BuildingKind::Box.fit_to_width(1), BuildingKind::Box);
    }

    #[test]
    fn tier_rejects_empty_rectangles() {
        assert!(Tier::new(0, 0, 0, 5).is_none());
        assert!(Tier::new(0, 5, 3, 3).is_none());
        assert!(Tier::new(5, 0, 0, 5).is_none());
        let tier = Tier::new(2, 6, 1, 4).unwrap();
        assert_eq!((tier.width(), tier.height()), (4, 3));
        assert!(tier.contains(2, 1));
        assert!(!tier.contains(6, 1), "x1 is exclusive");
        assert!(!tier.contains(2, 4), "h1 is exclusive");
    }

    #[test]
    fn window_set_tracks_counts_and_lighting() {
        let mut windows = WindowSet::default();
        assert!(windows.is_empty());
        windows.push(1, 2, 0, 9, true);
        windows.push(3, 2, 0, 9, false);
        windows.push(5, 2, 0, 9, true);
        assert_eq!((windows.len(), windows.target_count()), (3, 2));
        assert_eq!(windows.lit_count(), 0);
        windows.light_to_target();
        assert_eq!(windows.lit_count(), 2);
        assert!(windows.is_consistent());
        windows.lit.pop();
        assert!(!windows.is_consistent());
    }

    #[test]
    fn validate_accepts_supported_stack() {
        let base = Tier::new(10, 20, 0, 30).unwrap();
        let setback = Tier::new(12, 18, 30, 40).unwrap();
        let mut b = building(vec![base, setback]);
        b.windows.push(11, 2, 0, 0, true);
        b.windows.push(13, 35, 0, 0, true);
        assert_eq!(b.validate(40), Ok(()));
        assert_eq!(b.roof_height(), 40);
        assert_eq!(b.width(), 10);
    }

    #[test]
    fn validate_reports_each_broken_invariant() {
        let base = Tier::new(10, 20, 0, 30).unwrap();

        let floating = Tier::new(12, 18, 31, 40).unwrap();
        assert_eq!(
            building(vec![base, floating]).validate(50),
            Err(BuildingError::TierUnsupported(1))
        );

        let beside = Tier::new(20, 22, 30, 40).unwrap(); // touches no column of base
        let mut wide = building(vec![base, beside]);
        wide.x1 = 22;
        assert_eq!(wide.validate(50), Err(BuildingError::TierUnsupported(1)));

        assert_eq!(
            building(vec![base]).validate(29),
            Err(BuildingError::TierTooTall(0))
        );

        let outside = Tier::new(8, 12, 0, 5).unwrap();
        assert_eq!(
            building(vec![outside]).validate(50),
            Err(BuildingError::TierOutsideFootprint(0))
        );

        assert_eq!(building(vec![]).validate(50), Err(BuildingError::NoTiers));

        let mut stray = building(vec![base]);
        stray.windows.push(25, 2, 0, 0, true);
        assert_eq!(
            stray.validate(50),
            Err(BuildingError::WindowOutsideTiers(0))
        );

        let mut bad_body = building(vec![base]);
        bad_body.body_index = 6;
        assert_eq!(bad_body.validate(50), Err(BuildingError::BadBodyIndex(6)));

        let mut bad_layer = building(vec![base]);
        bad_layer.layer = 4;
        assert_eq!(bad_layer.validate(50), Err(BuildingError::BadLayer(4)));

        let mut empty = building(vec![base]);
        empty.x1 = empty.x0;
        assert_eq!(empty.validate(50), Err(BuildingError::EmptyFootprint));
    }
}
