//! Tier builders: turn a building kind, footprint, and height into tiers
//! (Rebuild Prompt, "Deterministic Generation", shape contracts).
//!
//! The original tier dimensions are source-dependent, so these builders keep
//! the shape contracts (behaviorally faithful) rather than the exact
//! geometry:
//! - every tier is non-empty and lies inside the footprint;
//! - no tier reaches above the requested height, which is at most `max_h`;
//! - every tier is grounded or rests on another tier.
//!
//! Each builder works for footprints up to [`MAX_SHAPE_WIDTH`] cells. When a
//! kind cannot fit its footprint or height, it falls back to a plain box.
//! Roofs, eaves, domes, and spires are marked `dark` (no windows); crowns,
//! observation decks, and clock faces are marked `solid` (densely lit).

use super::building::{BuildingKind, Tier};
use super::rng::{js_round, Mulberry32};

/// Widest footprint the builders are designed and tested for.
pub const MAX_SHAPE_WIDTH: i32 = 50;

/// Accumulates tiers, silently skipping empty rectangles.
#[derive(Default)]
struct Stack {
    tiers: Vec<Tier>,
}

impl Stack {
    fn push(&mut self, x0: i32, x1: i32, h0: i32, h1: i32) {
        self.push_flags(x0, x1, h0, h1, false, false);
    }

    fn push_dark(&mut self, x0: i32, x1: i32, h0: i32, h1: i32) {
        self.push_flags(x0, x1, h0, h1, false, true);
    }

    fn push_solid(&mut self, x0: i32, x1: i32, h0: i32, h1: i32) {
        self.push_flags(x0, x1, h0, h1, true, false);
    }

    fn push_flags(&mut self, x0: i32, x1: i32, h0: i32, h1: i32, solid: bool, dark: bool) {
        if let Some(mut tier) = Tier::new(x0, x1, h0, h1) {
            tier.solid = solid;
            tier.dark = dark;
            self.tiers.push(tier);
        }
    }
}

/// Returns `[a, b)` of width `w` centered in `[x0, x0 + width)`.
fn centered(x0: i32, width: i32, w: i32) -> (i32, i32) {
    let a = x0 + (width - w) / 2;
    (a, a + w)
}

/// Adjusts `w` (down, never below `min`) to have the same parity as
/// `width`, so a centered span is exactly symmetric.
fn match_parity(w: i32, width: i32, min: i32) -> i32 {
    let w = if (width - w) % 2 != 0 { w - 1 } else { w };
    if w < min {
        min + (width - min) % 2
    } else {
        w
    }
}

/// Uniform float in `[lo, hi)`, consuming one RNG value.
fn frac(rng: &mut Mulberry32, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * rng.next_f64()
}

/// Rounds `height * fraction` and clamps it to `[lo, hi]`.
fn portion(height: i32, fraction: f64, lo: i32, hi: i32) -> i32 {
    (js_round(f64::from(height) * fraction) as i32).clamp(lo, hi.max(lo))
}

/// Smallest total height (cells) at which a kind keeps its shape.
pub fn min_height(kind: BuildingKind) -> i32 {
    use BuildingKind::*;
    match kind {
        Box => 1,
        Stepped | Gable | Arch | Slant | Crownblock | Factory => 5,
        Pointed | Ziggurat | Jenga | Pagoda => 8,
        Spire | Twin | Onion => 10,
        Watertank => 9,
        Needle | Invpyramid => 12,
        Tvtower | Clocktower => 14,
    }
}

/// Builds the tiers for one building.
///
/// `kind` should already be fitted to `width` with
/// [`BuildingKind::fit_to_width`]. `height` is the target total height and
/// is clamped to `1..=max_h`. Returns the tiers and the kind actually used,
/// which is [`BuildingKind::Box`] when the shape could not fit.
/// O(width + height) time and tiers.
pub fn build_tiers(
    kind: BuildingKind,
    x0: i32,
    width: i32,
    height: i32,
    max_h: i32,
    rng: &mut Mulberry32,
) -> (Vec<Tier>, BuildingKind) {
    let width = width.max(1);
    let height = height.clamp(1, max_h.max(1));
    let kind = kind.fit_to_width(width);
    let kind = if height < min_height(kind) {
        BuildingKind::Box
    } else {
        kind
    };
    let mut stack = Stack::default();
    let x1 = x0 + width;
    use BuildingKind::*;
    match kind {
        Box => stack.push(x0, x1, 0, height),
        Stepped => stepped(&mut stack, x0, width, height, rng),
        Spire => spire(&mut stack, x0, width, height, rng),
        Pointed => sloped_roof(&mut stack, x0, width, height, 2, 0.4, rng),
        Ziggurat => ziggurat(&mut stack, x0, width, height, rng),
        Gable => sloped_roof(&mut stack, x0, width, height, 1, 0.4, rng),
        Arch => arch(&mut stack, x0, width, height),
        Slant => slant(&mut stack, x0, width, height, rng),
        Crownblock => crownblock(&mut stack, x0, width, height, rng),
        Needle => needle(&mut stack, x0, width, height, rng),
        Tvtower => tvtower(&mut stack, x0, width, height, rng),
        Twin => twin(&mut stack, x0, width, height, rng),
        Onion => onion(&mut stack, x0, width, height),
        Watertank => watertank(&mut stack, x0, width, height, rng),
        Invpyramid => invpyramid(&mut stack, x0, width, height, rng),
        Clocktower => clocktower(&mut stack, x0, width, height, rng),
        Jenga => jenga(&mut stack, x0, width, height, rng),
        Factory => factory(&mut stack, x0, width, height, rng),
        Pagoda => pagoda(&mut stack, x0, width, height, rng),
    }
    if stack.tiers.is_empty() {
        return (
            vec![Tier::new(x0, x1, 0, height).expect("non-empty box")],
            Box,
        );
    }
    (stack.tiers, kind)
}

/// Setback tower: a tall base with one or two inset tiers.
fn stepped(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let base = portion(h, frac(rng, 0.5, 0.72), 2, h - 1);
    stack.push(x0, x0 + w, 0, base);
    let setbacks = if w >= 11 && h >= 12 {
        1 + rng.rand_int(0, 1) as i32
    } else {
        1
    };
    let (mut a, mut b, mut floor) = (x0, x0 + w, base);
    for index in 0..setbacks {
        let inset = rng.rand_int(1, 2) as i32;
        if b - a - 2 * inset < 3 || floor >= h {
            break;
        }
        a += inset;
        b -= inset;
        let top = if index + 1 == setbacks {
            h
        } else {
            floor + ((h - floor) / 2).max(1)
        };
        stack.push(a, b, floor, top);
        floor = top;
    }
}

/// Setbacks crowned by a thin dark spire.
fn spire(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let base = portion(h, frac(rng, 0.5, 0.65), 2, h - 3);
    stack.push(x0, x0 + w, 0, base);
    let mid_w = match_parity(w - 4, w, 3);
    let (a, b) = centered(x0, w, mid_w);
    let mid_top = base + ((h - base) / 2).max(1);
    stack.push(a, b, base, mid_top);
    let spire_w = match_parity(1, w, 1);
    let (sa, sb) = centered(x0, w, spire_w);
    stack.push_dark(sa, sb, mid_top, h);
}

/// Body with a stepped roof that insets one column per side every
/// `rows_per_step` rows. Gables use 1 (shallow), pointed towers use 2 (steep).
fn sloped_roof(
    stack: &mut Stack,
    x0: i32,
    w: i32,
    h: i32,
    rows_per_step: i32,
    max_fraction: f64,
    rng: &mut Mulberry32,
) {
    let steps = (w - 1) / 2;
    let wanted = steps * rows_per_step;
    let cap = portion(h, frac(rng, max_fraction * 0.75, max_fraction), 1, h - 2);
    let roof_rows = wanted.min(cap).max(1);
    let body_top = h - roof_rows;
    stack.push(x0, x0 + w, 0, body_top);
    for row in 0..roof_rows {
        let inset = row / rows_per_step + 1;
        let row_w = w - 2 * inset;
        if row_w < 1 {
            break;
        }
        stack.push_dark(
            x0 + inset,
            x0 + w - inset,
            body_top + row,
            body_top + row + 1,
        );
    }
}

/// Many shallow setbacks, one column per side each.
fn ziggurat(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let base = portion(h, frac(rng, 0.35, 0.5), 2, h - 1);
    stack.push(x0, x0 + w, 0, base);
    let (mut a, mut b, mut floor) = (x0, x0 + w, base);
    while floor < h && b - a - 2 >= 3 {
        a += 1;
        b -= 1;
        let top = (floor + rng.rand_int(2, 4) as i32).min(h);
        stack.push(a, b, floor, top);
        floor = top;
    }
}

/// Body with a rounded (half-ellipse) dark roof.
fn arch(stack: &mut Stack, x0: i32, w: i32, h: i32) {
    let rows = ((w + 1) / 2).min((h * 2) / 5).max(1);
    let body_top = h - rows;
    stack.push(x0, x0 + w, 0, body_top);
    for row in 0..rows {
        let y = (f64::from(row) + 0.5) / f64::from(rows);
        let row_w = js_round(f64::from(w) * (1.0 - y * y).sqrt()) as i32;
        let row_w = match_parity(row_w.min(w), w, 1);
        let (a, b) = centered(x0, w, row_w);
        stack.push_dark(a, b, body_top + row, body_top + row + 1);
    }
}

/// Body with a roof slanting down to one side.
fn slant(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let rows = (w - w / 3).min((h * 2) / 5).max(1);
    let body_top = h - rows;
    stack.push(x0, x0 + w, 0, body_top);
    let falls_right = rng.next_f64() < 0.5;
    for row in 0..rows {
        let row_w = w - (row + 1);
        if row_w < 1 {
            break;
        }
        let (a, b) = if falls_right {
            (x0, x0 + row_w)
        } else {
            (x0 + w - row_w, x0 + w)
        };
        stack.push_dark(a, b, body_top + row, body_top + row + 1);
    }
}

/// Block topped by a bright, densely lit crown.
fn crownblock(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let crown = (rng.rand_int(2, 3) as i32).min(h - 2);
    let body_top = h - crown;
    stack.push(x0, x0 + w, 0, body_top);
    let inset = if w >= 9 { 1 } else { 0 };
    stack.push_solid(x0 + inset, x0 + w - inset, body_top, h);
}

/// Narrow shaft with a wide, lit observation deck near the top.
fn needle(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let shaft = match_parity(2, w, 2);
    let (sa, sb) = centered(x0, w, shaft);
    let deck_rows = rng.rand_int(2, 3) as i32;
    let deck_floor = portion(h, frac(rng, 0.68, 0.78), 2, h - deck_rows - 1);
    let deck_w = match_parity((w - 2).min(9), w, shaft);
    let (da, db) = centered(x0, w, deck_w);
    stack.push(sa, sb, 0, deck_floor);
    stack.push_solid(da, db, deck_floor, deck_floor + deck_rows);
    stack.push_dark(sa, sb, deck_floor + deck_rows, h);
}

/// Shaft carrying a sphere, with the shaft continuing above it.
fn tvtower(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let shaft = match_parity(2, w, 2);
    let (sa, sb) = centered(x0, w, shaft);
    let diameter = match_parity(w.min(9), w, shaft);
    let rows = diameter.min(h / 3).max(3);
    let center = portion(h, frac(rng, 0.62, 0.72), 2, h - 2);
    let sphere_floor = (center - rows / 2).clamp(2, h - rows - 1);
    stack.push(sa, sb, 0, sphere_floor);
    for row in 0..rows {
        let y = (f64::from(row) + 0.5) / f64::from(rows) * 2.0 - 1.0;
        let row_w = js_round(f64::from(diameter) * (1.0 - y * y).sqrt()) as i32;
        let row_w = match_parity(row_w.max(shaft), w, shaft);
        let (a, b) = centered(x0, w, row_w);
        stack.push(a, b, sphere_floor + row, sphere_floor + row + 1);
    }
    stack.push_dark(sa, sb, sphere_floor + rows, h);
}

/// Two towers joined by a sky bridge; one tower may be shorter.
fn twin(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let gap = rng.rand_int(1, 3) as i32;
    let tower = (w - gap) / 2;
    let left = (x0, x0 + tower);
    let right = (x0 + w - tower, x0 + w);
    let shorter = h - rng.rand_int(0, i64::from(h / 6)) as i32;
    let (left_top, right_top) = if rng.next_f64() < 0.5 {
        (h, shorter)
    } else {
        (shorter, h)
    };
    let bridge = portion(h, frac(rng, 0.4, 0.6), 2, left_top.min(right_top) - 3);
    stack.push(left.0, left.1, 0, bridge);
    stack.push(right.0, right.1, 0, bridge);
    stack.push_dark(x0, x0 + w, bridge, bridge + 2);
    stack.push(left.0, left.1, bridge + 2, left_top);
    stack.push(right.0, right.1, bridge + 2, right_top);
}

/// Body topped by an onion dome and a thin tip.
fn onion(stack: &mut Stack, x0: i32, w: i32, h: i32) {
    let dome_w = match_parity((w - 2).min(9), w, 3);
    let mut widths = vec![dome_w - 2, dome_w, dome_w, dome_w - 2];
    let mut next = dome_w - 4;
    while next >= 1 {
        widths.push(next);
        next -= 2;
    }
    let tip = match_parity(1, w, 1);
    widths.push(tip);
    let max_rows = (h * 9) / 20;
    widths.truncate(max_rows.max(1) as usize);
    let body_top = h - widths.len() as i32;
    stack.push(x0, x0 + w, 0, body_top);
    for (row, row_w) in widths.iter().enumerate() {
        let (a, b) = centered(x0, w, (*row_w).max(1));
        let row = body_top + row as i32;
        stack.push_dark(a, b, row, row + 1);
    }
}

/// Body with a rooftop water tank on two legs.
fn watertank(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let body_top = h - 5;
    stack.push(x0, x0 + w, 0, body_top);
    let tank_w = rng.rand_int(3, 4) as i32;
    let start = x0 + 1 + rng.rand_int(0, i64::from(w - tank_w - 2)) as i32;
    let end = start + tank_w;
    stack.push_dark(start, start + 1, body_top, body_top + 2);
    stack.push_dark(end - 1, end, body_top, body_top + 2);
    stack.push_dark(start, end, body_top + 2, body_top + 4);
    stack.push_dark(start + 1, end - 1, body_top + 4, body_top + 5);
}

/// Narrow base widening upward to the full footprint.
fn invpyramid(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let base_w = match_parity(w / 3, w, 3);
    let base_top = portion(h, frac(rng, 0.25, 0.35), 2, h - 4);
    let (a, b) = centered(x0, w, base_w);
    stack.push(a, b, 0, base_top);
    let steps = (w - base_w) / 2;
    let rows = (h - base_top - 2).max(1);
    let per_row = (steps + rows - 1) / rows;
    let (mut a, mut b, mut floor) = (a, b, base_top);
    while b - a < w && floor < h - 2 {
        a = (a - per_row).max(x0);
        b = (b + per_row).min(x0 + w);
        stack.push(a, b, floor, floor + 1);
        floor += 1;
    }
    stack.push(x0, x0 + w, floor, h);
}

/// Body, narrower tower with a lit clock band, and a pointed cap.
fn clocktower(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let inset = if w >= 9 { rng.rand_int(1, 2) as i32 } else { 1 };
    let tower_w = (w - 2 * inset).max(3);
    let (ta, tb) = centered(x0, w, tower_w);
    let cap_rows = ((tower_w - 1) / 2).min(h / 5).max(1);
    let clock_floor = h - cap_rows - 3;
    let body_top = portion(h, frac(rng, 0.35, 0.5), 2, clock_floor - 1);
    stack.push(x0, x0 + w, 0, body_top);
    stack.push(ta, tb, body_top, clock_floor);
    stack.push_solid(ta, tb, clock_floor, clock_floor + 3);
    for row in 0..cap_rows {
        let row_inset = row + 1;
        if tower_w - 2 * row_inset < 1 {
            break;
        }
        let floor = clock_floor + 3 + row;
        stack.push_dark(ta + row_inset, tb - row_inset, floor, floor + 1);
    }
}

/// Offset stack of blocks, each overlapping the one below.
fn jenga(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    // Wider than half the footprint, so consecutive blocks always overlap.
    let block_w = portion(w, frac(rng, 0.6, 0.75), w / 2 + 1, w);
    let mut floor = 0;
    let mut from_left = rng.next_f64() < 0.5;
    while floor < h {
        let top = (floor + rng.rand_int(3, 5) as i32).min(h);
        let (a, b) = if from_left {
            (x0, x0 + block_w)
        } else {
            (x0 + w - block_w, x0 + w)
        };
        stack.push(a, b, floor, top);
        floor = top;
        from_left = !from_left;
    }
}

/// Low works with a sawtooth roof and a chimney.
fn factory(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let low = h.min(rng.rand_int(5, 10) as i32);
    let tooth_rows = rng.rand_int(2, 3) as i32;
    let body_top = (low - tooth_rows).max(2);
    stack.push(x0, x0 + w, 0, body_top);
    let tooth_w = rng.rand_int(3, 4) as i32;
    let mut start = x0;
    while start < x0 + w {
        let span = tooth_w.min(x0 + w - start);
        for row in 0..tooth_rows {
            let row_w = span - (row * span) / tooth_rows;
            if row_w >= 1 && body_top + row < h {
                stack.push_dark(start, start + row_w, body_top + row, body_top + row + 1);
            }
        }
        start += tooth_w;
    }
    let chimney_w = rng.rand_int(1, 2) as i32;
    let chimney_x = x0 + rng.rand_int(0, i64::from(w - chimney_w)) as i32;
    let chimney_top = (body_top + rng.rand_int(4, 9) as i32).min(h);
    stack.push_dark(chimney_x, chimney_x + chimney_w, body_top, chimney_top);
}

/// Stacked storeys, each with wider dark eaves, and a thin finial.
fn pagoda(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let mut body_w = w - 2;
    let mut floor = 0;
    while body_w >= 3 {
        let rows = rng.rand_int(2, 3) as i32;
        if floor + rows + 1 > h {
            break;
        }
        let (a, b) = centered(x0, w, body_w);
        stack.push(a, b, floor, floor + rows);
        let eave_w = (body_w + 2).min(w);
        let (ea, eb) = centered(x0, w, eave_w);
        stack.push_dark(ea, eb, floor + rows, floor + rows + 1);
        floor += rows + 1;
        body_w -= 2;
    }
    let tip = match_parity(1, w, 1);
    let (ta, tb) = centered(x0, w, tip);
    stack.push_dark(ta, tb, floor, (floor + 2).min(h));
}

/// Renders tiers as text rows, top row first, for previews and debugging.
///
/// `#` is a windowed tier, `-` a dark tier, `*` a solid (lit) tier, and `.`
/// empty sky. Later tiers draw over earlier ones. O(width × height × tiers).
pub fn ascii_rows(tiers: &[Tier], x0: i32, width: i32, height: i32) -> Vec<String> {
    (0..height)
        .rev()
        .map(|h| {
            (x0..x0 + width)
                .map(|x| {
                    tiers
                        .iter()
                        .rev()
                        .find(|tier| tier.contains(x, h))
                        .map_or('.', |tier| {
                            if tier.solid {
                                '*'
                            } else if tier.dark {
                                '-'
                            } else {
                                '#'
                            }
                        })
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::building::{Building, WindowSet, KIND_WEIGHTS};

    fn as_building(tiers: Vec<Tier>, kind: BuildingKind, x0: i32, width: i32) -> Building {
        Building {
            layer: 0,
            x0,
            x1: x0 + width,
            kind,
            tiers,
            body_index: 0,
            tone: 0,
            accent_roll: 0.0,
            windows: WindowSet::default(),
        }
    }

    #[test]
    fn every_kind_satisfies_shape_contracts_across_sizes_and_seeds() {
        let max_h = 191;
        let mut cases = 0;
        for (kind, _) in KIND_WEIGHTS {
            for width in 1..=MAX_SHAPE_WIDTH {
                for height in [1, 4, 5, 8, 9, 10, 12, 14, 20, 40, 100, 191] {
                    for seed in 0..6 {
                        let x0 = -7 + seed as i32 * 3; // include negative world x
                        let mut rng = Mulberry32::new(seed);
                        let (tiers, used) = build_tiers(kind, x0, width, height, max_h, &mut rng);
                        let building = as_building(tiers, used, x0, width);
                        assert_eq!(
                            building.validate(max_h),
                            Ok(()),
                            "{kind:?} w={width} h={height} seed={seed}"
                        );
                        assert!(
                            building.roof_height() <= height,
                            "{kind:?} w={width} h={height} seed={seed} too tall"
                        );
                        cases += 1;
                    }
                }
            }
        }
        assert_eq!(cases, 19 * 50 * 12 * 6);
    }

    #[test]
    fn kinds_keep_their_shape_when_they_fit() {
        for (kind, _) in KIND_WEIGHTS {
            let width = kind.min_width().max(9);
            let mut rng = Mulberry32::new(11);
            let (_, used) = build_tiers(kind, 0, width, 60, 191, &mut rng);
            assert_eq!(
                used, kind,
                "{kind:?} should not fall back at w={width} h=60"
            );
        }
    }

    #[test]
    fn too_narrow_or_too_short_falls_back_to_box() {
        let mut rng = Mulberry32::new(1);
        let (tiers, used) = build_tiers(BuildingKind::Needle, 0, 8, 60, 191, &mut rng);
        assert_eq!(used, BuildingKind::Box);
        assert_eq!(tiers, vec![Tier::new(0, 8, 0, 60).unwrap()]);
        let (_, used) = build_tiers(BuildingKind::Tvtower, 0, 9, 13, 191, &mut rng);
        assert_eq!(used, BuildingKind::Box);
    }

    #[test]
    fn height_is_clamped_to_max_h() {
        let mut rng = Mulberry32::new(3);
        let (tiers, _) = build_tiers(BuildingKind::Box, 0, 5, 500, 40, &mut rng);
        assert_eq!(tiers[0].h1, 40);
    }

    #[test]
    fn same_seed_gives_same_tiers() {
        for (kind, _) in KIND_WEIGHTS {
            let a = build_tiers(kind, 3, 15, 50, 191, &mut Mulberry32::new(77));
            let b = build_tiers(kind, 3, 15, 50, 191, &mut Mulberry32::new(77));
            assert_eq!(a, b, "{kind:?}");
        }
    }

    #[test]
    fn distinctive_features_are_present() {
        let mut rng = Mulberry32::new(5);
        let (tiers, _) = build_tiers(BuildingKind::Crownblock, 0, 12, 40, 191, &mut rng);
        assert!(tiers.iter().any(|tier| tier.solid), "crown is lit");

        let (tiers, _) = build_tiers(BuildingKind::Pointed, 0, 11, 60, 191, &mut rng);
        let top = tiers.iter().max_by_key(|tier| tier.h1).unwrap();
        assert!(
            top.dark && top.width() <= 3,
            "pointed roof narrows to a tip"
        );

        let (tiers, _) = build_tiers(BuildingKind::Twin, 0, 15, 60, 191, &mut rng);
        let grounded = tiers.iter().filter(|tier| tier.h0 == 0).count();
        assert_eq!(grounded, 2, "twin has two towers");

        let (tiers, _) = build_tiers(BuildingKind::Invpyramid, 0, 15, 60, 191, &mut rng);
        let base = tiers.iter().find(|tier| tier.h0 == 0).unwrap();
        let top = tiers.iter().max_by_key(|tier| tier.h1).unwrap();
        assert!(base.width() < top.width(), "inverted pyramid widens upward");

        let (tiers, _) = build_tiers(BuildingKind::Factory, 0, 20, 100, 191, &mut rng);
        let body = tiers.iter().find(|tier| tier.h0 == 0).unwrap();
        assert!(body.h1 <= 10, "factory stays low");
    }

    #[test]
    fn ascii_rows_draws_top_first_with_legend() {
        let mut tiers = vec![Tier::new(0, 3, 0, 2).unwrap()];
        let mut roof = Tier::new(1, 2, 2, 3).unwrap();
        roof.dark = true;
        tiers.push(roof);
        assert_eq!(ascii_rows(&tiers, 0, 3, 3), vec![".-.", "###", "###"]);
    }

    #[test]
    fn helpers_center_symmetrically() {
        assert_eq!(centered(0, 9, 3), (3, 6));
        assert_eq!(match_parity(4, 9, 1), 3);
        assert_eq!(match_parity(3, 9, 1), 3);
        assert_eq!(match_parity(0, 10, 1), 2);
        assert_eq!(portion(10, 0.5, 2, 8), 5);
        assert_eq!(portion(10, 0.99, 2, 8), 8);
        assert_eq!(
            portion(10, 0.5, 6, 3),
            6,
            "upper bound below lower clamps to lower"
        );
    }
}
