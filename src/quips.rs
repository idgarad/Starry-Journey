//! Silly status lines shown in the message box while an Endless city is
//! first being built (owner request, 2026-11-03). Each line shows for
//! [`QUIP_SECONDS`]; the order is a seeded shuffle, so every city tells its
//! own story and a line never repeats until all have been shown.

use crate::model::rng::Mulberry32;

/// How long each line stays up, in seconds of real time.
pub const QUIP_SECONDS: f64 = 4.0;

/// Seed salt for the quip order, separate from every city stream.
const QUIP_SALT: u32 = 0x7a3b_10c5;

/// The lines, in no particular order.
pub const QUIPS: &[&str] = &[
    "Calibrating zebra stripes",
    "Unpacking packaging materials",
    "Polishing the moon",
    "Counting the stars twice",
    "Inflating skyscrapers",
    "Teaching pigeons to commute",
    "Untangling power lines",
    "Alphabetizing the windows",
    "Herding taxis",
    "Installing elevators, mostly vertically",
    "Negotiating with the zoning board",
    "Folding the street map",
    "Charging the streetlights",
    "Waiting for the building inspector",
    "Tuning the traffic lights",
    "Rounding up stray cranes",
    "Watering the rooftop gardens",
    "Hiding the cables",
    "Warming up the neon",
    "Reassuring nervous foundations",
    "Stacking floors carefully",
    "Leveling the ground (mostly)",
    "Assigning window lights at random",
    "Convincing the moon to rise",
    "Laying fresh asphalt",
    "Sweeping up spare bricks",
    "Rehearsing rush hour",
    "Looking for the keys to the city",
    "Ironing out the potholes",
    "Dusting off the antennas",
    "Straightening the fire escapes",
    "Asking the night shift to clock in",
];

/// A seeded order over all of [`QUIPS`] (Fisher-Yates). O(n).
pub fn quip_order(seed: u32) -> Vec<usize> {
    let mut rng = Mulberry32::new(seed ^ QUIP_SALT);
    let mut order: Vec<usize> = (0..QUIPS.len()).collect();
    for i in (1..order.len()).rev() {
        let j = rng.rand_int(0, i as i64) as usize;
        order.swap(i, j);
    }
    order
}

/// The line to show `seconds` after construction started, for a city with
/// `seed`. Cycles through the whole shuffled list before repeating.
pub fn quip_at(seed: u32, seconds: f64) -> &'static str {
    let order = quip_order(seed);
    let step = if seconds.is_finite() && seconds > 0.0 {
        (seconds / QUIP_SECONDS) as usize
    } else {
        0
    };
    QUIPS[order[step % order.len()]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_is_a_seeded_permutation() {
        let order = quip_order(42);
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..QUIPS.len()).collect::<Vec<_>>());
        assert_eq!(order, quip_order(42));
        assert_ne!(order, quip_order(43));
    }

    #[test]
    fn each_line_holds_for_four_seconds_then_changes() {
        let first = quip_at(7, 0.0);
        assert_eq!(quip_at(7, QUIP_SECONDS - 0.01), first);
        assert_ne!(quip_at(7, QUIP_SECONDS), first);
    }

    #[test]
    fn every_line_shows_once_before_any_repeats() {
        let shown: Vec<&str> = (0..QUIPS.len())
            .map(|i| quip_at(9, i as f64 * QUIP_SECONDS))
            .collect();
        let mut unique = shown.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), QUIPS.len());
        assert_eq!(quip_at(9, QUIPS.len() as f64 * QUIP_SECONDS), shown[0]);
    }

    #[test]
    fn lines_fit_the_message_limit_and_have_no_surrounding_space() {
        for quip in QUIPS {
            assert!(!quip.is_empty() && quip.trim() == *quip);
            assert!(quip.encode_utf16().count() <= crate::message::MESSAGE_LIMIT_UTF16);
        }
    }

    #[test]
    fn bad_times_show_the_first_line() {
        assert_eq!(quip_at(1, -5.0), quip_at(1, 0.0));
        assert_eq!(quip_at(1, f64::NAN), quip_at(1, 0.0));
    }
}
