//! Deterministic random streams (Rebuild Prompt, "Deterministic Generation").
//!
//! Mulberry32 is specified for cross-language compatibility with the original
//! JavaScript model, so every operation uses explicit wrapping 32-bit
//! arithmetic and logical shifts. Stream-seed helpers keep the city, ambient,
//! cloud, and tree streams independent of each other.

/// Seed XOR for the root ambient/traffic stream.
pub const AMBIENT_SEED_XOR: u32 = 0x9e37_79b9;
/// Base added to the layer index before XOR for endless child layers.
pub const LAYER_SEED_BASE: u32 = 0x85eb_ca6b;
/// Seed XOR for the cloud stream.
pub const CLOUD_SEED_XOR: u32 = 0xc2b2_ae35;
/// Multiplier applied to a tree slot index when hashing its seed.
pub const TREE_SLOT_MULTIPLIER: u32 = 0x045d_9f3b;
/// Side constant for background-verge tree slots.
pub const TREE_BACKGROUND_XOR: u32 = 0x27d4_eb2d;
/// Side constant for foreground-verge tree slots.
pub const TREE_FOREGROUND_XOR: u32 = 0x632b_e5ab;

/// Mulberry32 pseudo-random generator.
///
/// Produces the same sequence as the canonical JavaScript implementation
/// for the same `u32` seed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mulberry32 {
    state: u32,
}

impl Mulberry32 {
    /// Creates a generator from a 32-bit seed.
    pub fn new(seed: u32) -> Self {
        Self { state: seed }
    }

    /// Returns the next raw 32-bit output. O(1).
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_add(0x6d2b_79f5);
        let mut t = self.state;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        t ^ (t >> 14)
    }

    /// Returns the next value in `[0, 1)`, matching JavaScript's
    /// `uint32 / 4294967296`. O(1).
    pub fn next_f64(&mut self) -> f64 {
        f64::from(self.next_u32()) / 4_294_967_296.0
    }

    /// Returns an integer in the inclusive range `[lo, hi]` as
    /// `lo + floor(next() * (hi - lo + 1))`.
    ///
    /// # Panics
    ///
    /// Panics if `hi < lo`, which would be a programming error in a caller.
    pub fn rand_int(&mut self, lo: i64, hi: i64) -> i64 {
        assert!(hi >= lo, "rand_int requires lo <= hi (got {lo}..={hi})");
        let span = (hi - lo + 1) as f64;
        lo + (self.next_f64() * span).floor() as i64
    }
}

/// Seed for the root ambient/traffic stream: `seed XOR 0x9e3779b9`.
pub fn ambient_seed(scene_seed: u32) -> u32 {
    scene_seed ^ AMBIENT_SEED_XOR
}

/// Seed for endless child layer `layer`: `seed XOR (0x85ebca6b + layer)`.
pub fn layer_seed(scene_seed: u32, layer: u32) -> u32 {
    scene_seed ^ LAYER_SEED_BASE.wrapping_add(layer)
}

/// Seed for the cloud stream: `seed XOR 0xc2b2ae35`.
pub fn cloud_seed(scene_seed: u32) -> u32 {
    scene_seed ^ CLOUD_SEED_XOR
}

/// Seed for one roadside-tree slot. Negative slots are valid; the slot is
/// reinterpreted as its 32-bit two's-complement pattern, as JavaScript's
/// `Math.imul` does.
pub fn tree_slot_seed(scene_seed: u32, slot: i64, foreground: bool) -> u32 {
    let side = if foreground {
        TREE_FOREGROUND_XOR
    } else {
        TREE_BACKGROUND_XOR
    };
    scene_seed ^ (slot as i32 as u32).wrapping_mul(TREE_SLOT_MULTIPLIER) ^ side
}

/// JavaScript-style `Math.round` for model calculations: `floor(x + 0.5)`.
///
/// Rust's `f64::round` rounds halves away from zero and differs for negative
/// halves (`-2.5` gives `-3`, JavaScript gives `-2`).
pub fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_four(seed: u32) -> [u32; 4] {
        let mut rng = Mulberry32::new(seed);
        [
            rng.next_u32(),
            rng.next_u32(),
            rng.next_u32(),
            rng.next_u32(),
        ]
    }

    // Reference vectors from an independent emulation of the canonical
    // JavaScript implementation (Math.imul and >>> semantics).
    #[test]
    fn mulberry32_matches_reference_vectors() {
        assert_eq!(
            first_four(0),
            [0x4434_b462, 0x0015_9c37, 0x3928_5b08, 0x256d_8104]
        );
        assert_eq!(
            first_four(1),
            [0xa087_eaf3, 0x00b3_49c9, 0x8706_c4eb, 0xfb26_27fd]
        );
        assert_eq!(
            first_four(0xdead_beef),
            [0xf0fd_995a, 0x4466_f0cf, 0xc5a3_fa66, 0x5bb0_6c70]
        );
        assert_eq!(
            first_four(u32::MAX),
            first_four(u32::MAX),
            "same seed must reproduce"
        );
    }

    #[test]
    fn next_f64_is_uint32_over_two_pow_32() {
        let value = Mulberry32::new(0).next_f64();
        assert_eq!(value, f64::from(0x4434_b462_u32) / 4_294_967_296.0);
        assert!((0.0..1.0).contains(&value));
    }

    #[test]
    fn rand_int_stays_in_inclusive_range() {
        let mut rng = Mulberry32::new(7);
        let mut seen = [false; 3];
        for _ in 0..1_000 {
            let value = rng.rand_int(-1, 1);
            assert!((-1..=1).contains(&value));
            seen[(value + 1) as usize] = true;
        }
        assert_eq!(seen, [true; 3], "every value in the range should occur");
        assert_eq!(Mulberry32::new(3).rand_int(5, 5), 5);
    }

    #[test]
    #[should_panic(expected = "lo <= hi")]
    fn rand_int_rejects_inverted_range() {
        Mulberry32::new(0).rand_int(2, 1);
    }

    #[test]
    fn stream_seeds_follow_specified_constants() {
        assert_eq!(ambient_seed(0), 0x9e37_79b9);
        assert_eq!(cloud_seed(0), 0xc2b2_ae35);
        assert_eq!(layer_seed(0, 0), 0x85eb_ca6b);
        assert_eq!(layer_seed(0, 3), 0x85eb_ca6e);
        assert_eq!(layer_seed(0x85eb_ca6b, 0), 0);
    }

    #[test]
    fn tree_slot_seed_handles_sides_and_negative_slots() {
        assert_eq!(tree_slot_seed(0, 0, false), TREE_BACKGROUND_XOR);
        assert_eq!(tree_slot_seed(0, 0, true), TREE_FOREGROUND_XOR);
        assert_eq!(
            tree_slot_seed(0, 1, false),
            TREE_SLOT_MULTIPLIER ^ TREE_BACKGROUND_XOR
        );
        // Math.imul(-1, 0x45d9f3b) == -0x45d9f3b, i.e. its two's complement.
        assert_eq!(
            tree_slot_seed(0, -1, false),
            TREE_SLOT_MULTIPLIER.wrapping_neg() ^ TREE_BACKGROUND_XOR
        );
    }

    #[test]
    fn js_round_rounds_halves_upward() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(-2.6), -3.0);
        assert_eq!(js_round(0.49), 0.0);
    }
}
