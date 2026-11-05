//! The ArtDeco palette's material (Rebuild Prompt, "ArtDeco material,
//! before compositing"): a warm sheen and a cool edge reflection across each
//! building's footprint, a fine horizontal grain, and a faint spill of light
//! from lit windows onto the surface beside them.
//!
//! These are the pure color formulas. The rasterizer (`raster.rs`) applies
//! them only to building body and unlit-window cells, never to lit windows,
//! masts, sky, stars, clouds, the road, or the message box. They depend only
//! on where a cell is on its building, so they move with the buildings and
//! have no clock.

use crate::model::building::Building;
use crate::model::palette::Rgb;

/// Where the warm sheen peaks across a footprint (0 left, 1 right), and
/// its width.
pub const SHEEN_CENTER: f64 = 0.3;
pub const SHEEN_WIDTH: f64 = 0.18;
/// Where the cool edge reflection peaks, and its width.
pub const REFLECTION_CENTER: f64 = 0.8;
pub const REFLECTION_WIDTH: f64 = 0.25;
/// The body's RGB scale: `BODY_BASE + sheen * BODY_SHEEN`.
pub const BODY_BASE: f64 = 0.96;
pub const BODY_SHEEN: f64 = 0.22;
/// The warm sheen's color and share, added.
pub const WARM: [f64; 3] = [226.0, 196.0, 146.0];
pub const WARM_SHARE: f64 = 0.055;
/// The cool reflection's color and share, added.
pub const COOL: [f64; 3] = [138.0, 155.0, 177.0];
pub const COOL_SHARE: f64 = 0.018;
/// The grain's period in rows and its largest swing in encoded levels.
pub const GRAIN_PERIOD: i32 = 17;
pub const GRAIN_LEVELS: f64 = 0.7;
/// The most a lit window spills onto a neighboring surface: this share of
/// its shaded RGB, for the brightest windows.
pub const SPILL_SHARE: f64 = 0.025;
/// Windows dimmer than this (brightest channel / 255) spill nothing; the
/// spill grows linearly to full at 1.
pub const SPILL_THRESHOLD: f64 = 0.55;

/// A building's footprint: its leftmost column and the end column
/// (exclusive) over all tiers. O(tiers).
pub fn footprint(building: &Building) -> (i32, i32) {
    let x0 = building.tiers.iter().map(|t| t.x0).min().unwrap_or(0);
    let x1 = building.tiers.iter().map(|t| t.x1).max().unwrap_or(x0 + 1);
    (x0, x1.max(x0 + 1))
}

/// How far across `footprint` the center of column `x` is: 0 at the left
/// edge to 1 at the right.
pub fn across(footprint: (i32, i32), x: i32) -> f64 {
    let (x0, x1) = footprint;
    (f64::from(x - x0) + 0.5) / f64::from(x1 - x0)
}

/// The warm sheen at `u` across the footprint: `exp(-((u-0.3)/0.18)^2)`.
pub fn sheen(u: f64) -> f64 {
    (-((u - SHEEN_CENTER) / SHEEN_WIDTH).powi(2)).exp()
}

/// The cool edge reflection at `u`: `exp(-((u-0.8)/0.25)^2)`.
pub fn reflection(u: f64) -> f64 {
    (-((u - REFLECTION_CENTER) / REFLECTION_WIDTH).powi(2)).exp()
}

/// The grain at building height `height` (0 at the ground), in encoded
/// levels within ±[`GRAIN_LEVELS`]. It is anchored to height, not screen
/// rows, so it moves with the building. The phase steps by 7 of 17 per
/// row (an agent choice: the spec gives only "a normalized modulo-17
/// phase"), so neighboring rows differ and it reads as grain, not a ramp.
pub fn grain(height: i32) -> f64 {
    let phase = f64::from((height * 7).rem_euclid(GRAIN_PERIOD)) / f64::from(GRAIN_PERIOD - 1);
    (phase * 2.0 - 1.0) * GRAIN_LEVELS
}

/// A surface cell's shaded color before window spill, unrounded: the body
/// scaled by the sheen, plus the warm and cool tints and the grain.
pub fn shade(body: Rgb, u: f64, height: i32) -> [f64; 3] {
    let s = sheen(u);
    let r = reflection(u);
    let g = grain(height);
    let scale = BODY_BASE + s * BODY_SHEEN;
    [0, 1, 2].map(|c| {
        f64::from(body.0[c]) * scale + WARM[c] * s * WARM_SHARE + COOL[c] * r * COOL_SHARE + g
    })
}

/// What a lit window whose shaded color is `window` spills onto a
/// neighboring surface: up to [`SPILL_SHARE`] of it, scaled by
/// `max(0, (max(window)/255 - 0.55) / 0.45)`.
pub fn spill(window: Rgb) -> [f64; 3] {
    let brightest = f64::from(*window.0.iter().max().unwrap_or(&0)) / 255.0;
    let strength = ((brightest - SPILL_THRESHOLD) / (1.0 - SPILL_THRESHOLD)).max(0.0);
    [0, 1, 2].map(|c| f64::from(window.0[c]) * SPILL_SHARE * strength)
}

/// The final color: `shaded` plus the strongest `spills` per channel (not
/// summed), rounded and clamped to 0..255.
pub fn finish(shaded: [f64; 3], spills: &[[f64; 3]]) -> Rgb {
    Rgb([0, 1, 2].map(|c| {
        let strongest = spills.iter().map(|s| s[c]).fold(0.0, f64::max);
        (shaded[c] + strongest).round().clamp(0.0, 255.0) as u8
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheen_and_reflection_peak_where_the_spec_puts_them() {
        assert!((sheen(0.3) - 1.0).abs() < 1e-12);
        assert!((reflection(0.8) - 1.0).abs() < 1e-12);
        assert!(sheen(0.3 + 0.18) - (-1.0f64).exp() < 1e-12);
        assert!(sheen(1.0) < 0.001);
    }

    #[test]
    fn shade_at_the_sheen_peak_brightens_and_warms() {
        let body = Rgb([100, 100, 100]);
        let lit = shade(body, 0.3, 0);
        // 100 * 1.18 + warm * 0.055 + a little cool + grain.
        for c in 0..3 {
            let expected = 100.0 * 1.18 + WARM[c] * 0.055 + COOL[c] * reflection(0.3) * 0.018;
            assert!((lit[c] - expected - grain(0)).abs() < 1e-9);
        }
        assert!(lit[0] > lit[2], "warm: more red than blue");
        // Far from both peaks the body is a little darker (0.96).
        let plain = shade(body, 0.0, 0);
        assert!(plain[1] < lit[1]);
    }

    #[test]
    fn grain_stays_within_its_bound_and_repeats_every_17_rows() {
        for h in -40..200 {
            assert!(grain(h).abs() <= GRAIN_LEVELS + 1e-12);
            assert_eq!(grain(h), grain(h + 17));
        }
        assert_ne!(grain(0), grain(1));
    }

    #[test]
    fn only_bright_windows_spill_and_the_strongest_wins_per_channel() {
        assert_eq!(spill(Rgb([140, 100, 60])), [0.0; 3]); // 140/255 < 0.55
        let full = spill(Rgb([255, 200, 0]));
        assert!((full[0] - 255.0 * 0.025).abs() < 1e-9);
        let a = [2.0, 0.0, 0.0];
        let b = [1.0, 3.0, 0.0];
        assert_eq!(finish([10.0, 10.0, 10.0], &[a, b]), Rgb([12, 13, 10]));
        assert_eq!(finish([300.0, -5.0, 0.4], &[]), Rgb([255, 0, 0]));
    }

    #[test]
    fn across_runs_from_the_left_edge_to_the_right() {
        assert!((across((10, 20), 10) - 0.05).abs() < 1e-12);
        assert!((across((10, 20), 19) - 0.95).abs() < 1e-12);
    }
}
