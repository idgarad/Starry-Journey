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
        Warehouse | Quonset | BigBox => 5,
        ParkingGarage => 6,
        Terraced => 8,
        FiveOverOne | Silos => 10,
        Taper | Cantilever => 12,
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
        FiveOverOne => five_over_one(&mut stack, x0, width, height, rng),
        Warehouse => warehouse(&mut stack, x0, width, height, rng),
        Quonset => quonset(&mut stack, x0, width, height),
        Silos => silos(&mut stack, x0, width, height, rng),
        BigBox => big_box(&mut stack, x0, width, height, rng),
        ParkingGarage => parking_garage(&mut stack, x0, width, height, rng),
        Taper => taper(&mut stack, x0, width, height),
        Cantilever => cantilever(&mut stack, x0, width, height, rng),
        Terraced => terraced(&mut stack, x0, width, height, rng),
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

/// A base with one or two setbacks; the spire itself is its structural
/// mast (see `mast.rs`), as in the Python reference (GitHub issue #2).
fn spire(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let base = portion(h, frac(rng, 0.5, 0.65), 2, h - 3);
    stack.push(x0, x0 + w, 0, base);
    let mid_w = match_parity(w - 4, w, 3);
    let (a, b) = centered(x0, w, mid_w);
    let mid_top = base + ((h - base) / 2).max(1);
    let top_w = match_parity(mid_w - 4, w, 1);
    if rng.next_f64() < 0.5 && top_w >= 3 {
        stack.push(a, b, base, mid_top);
        let (ta, tb) = centered(x0, w, top_w);
        stack.push(ta, tb, mid_top, h);
    } else {
        stack.push(a, b, base, h);
    }
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

/// A windowed body from `floor` to `top` over `[a, b)` with a steep dark
/// roof (one column in per side every two rows), as on a pointed tower.
fn pointed_top(stack: &mut Stack, a: i32, b: i32, floor: i32, top: i32) {
    let w = b - a;
    let roof_rows = ((w - 1) / 2 * 2).min((top - floor) / 3).max(1);
    let body_top = top - roof_rows;
    stack.push(a, b, floor, body_top);
    for row in 0..roof_rows {
        let inset = row / 2 + 1;
        if w - 2 * inset < 1 {
            break;
        }
        stack.push_dark(a + inset, b - inset, body_top + row, body_top + row + 1);
    }
}

/// Two separate pointed towers joined by a lit sky bridge; one tower may
/// be shorter (GitHub issue #2: "Two separate pointed towers joined by a
/// solid lit skybridge").
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
    stack.push_solid(x0, x0 + w, bridge, bridge + 2);
    pointed_top(stack, left.0, left.1, bridge + 2, left_top);
    pointed_top(stack, right.0, right.1, bridge + 2, right_top);
}

/// Body, a narrower windowed drum, and a rounded dome overhanging the
/// drum; the tip is the mast (GitHub issue #2: "Base, narrower drum,
/// rounded overhanging dome and mast").
fn onion(stack: &mut Stack, x0: i32, w: i32, h: i32) {
    let dome_w = match_parity((w - 2).min(9), w, 3);
    let drum_w = match_parity(dome_w - 4, w, 1).max(1);
    let mut widths = vec![dome_w - 2, dome_w, dome_w, dome_w - 2];
    let mut next = dome_w - 4;
    while next >= 1 {
        widths.push(next);
        next -= 2;
    }
    let drum_rows = 2;
    let max_rows = (h * 9) / 20;
    widths.truncate((max_rows - drum_rows).max(1) as usize);
    let dome_floor = h - widths.len() as i32;
    let drum_floor = (dome_floor - drum_rows).max(1);
    stack.push(x0, x0 + w, 0, drum_floor);
    let (da, db) = centered(x0, w, drum_w);
    stack.push(da, db, drum_floor, dome_floor);
    for (row, row_w) in widths.iter().enumerate() {
        let (a, b) = centered(x0, w, (*row_w).max(1));
        let row = dome_floor + row as i32;
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

/// Body, a narrower shaft, a lit clock section overhanging the shaft, a
/// belfry, and a pointed cap with the mast above (GitHub issue #2).
fn clocktower(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let inset = if w >= 9 { rng.rand_int(1, 2) as i32 } else { 1 };
    let tower_w = (w - 2 * inset).max(3);
    let (ta, tb) = centered(x0, w, tower_w);
    let clock_w = (tower_w + 2).min(w);
    let (ca, cb) = centered(x0, w, clock_w);
    let belfry_w = match_parity(tower_w - 2, w, 1).max(1);
    let (ba, bb) = centered(x0, w, belfry_w);
    let belfry_rows = 2;
    let cap_rows = ((belfry_w - 1) / 2).min(h / 6).max(1);
    let clock_floor = h - cap_rows - belfry_rows - 3;
    let body_top = portion(h, frac(rng, 0.35, 0.5), 2, clock_floor - 1);
    stack.push(x0, x0 + w, 0, body_top);
    stack.push(ta, tb, body_top, clock_floor);
    stack.push_solid(ca, cb, clock_floor, clock_floor + 3);
    let belfry_floor = clock_floor + 3;
    stack.push(ba, bb, belfry_floor, belfry_floor + belfry_rows);
    for row in 0..cap_rows {
        let row_inset = row + 1;
        if belfry_w - 2 * row_inset < 1 {
            break;
        }
        let floor = belfry_floor + belfry_rows + row;
        stack.push_dark(ba + row_inset, bb - row_inset, floor, floor + 1);
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

/// Stacked storeys, each with wider dark eaves, and a small pointed cap
/// under the mast (GitHub issue #2: "pointed cap and mast").
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
    let cap = match_parity(3, w, 1);
    let (ca, cb) = centered(x0, w, cap);
    stack.push_dark(ca, cb, floor, (floor + 1).min(h));
    let tip = match_parity(1, w, 1);
    let (ta, tb) = centered(x0, w, tip);
    stack.push_dark(ta, tb, floor + 1, (floor + 2).min(h));
}

// ----- Starry Journey extensions (docs/product.md, "Expanded buildings") -----
//
// Low-rise kinds pick their own modest height and treat `h` only as an upper
// bound, so they stay low however tall the layer's height sample is.

/// Lit retail podium, five residential floors, a broken parapet, and a
/// taller corner element, as in a typical "5-over-1" apartment block.
fn five_over_one(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let mut podium = 3;
    let mut floor_rows = rng.rand_int(2, 3) as i32;
    let corner_rows = rng.rand_int(2, 3) as i32;
    let total = |podium: i32, floor_rows: i32| podium + 5 * floor_rows + 1 + corner_rows;
    if total(podium, floor_rows) > h {
        floor_rows = 2;
    }
    if total(podium, floor_rows) > h {
        podium = 2;
    }
    // Squeeze floors further only on very short sites.
    let floors = ((h - podium - 1 - corner_rows) / floor_rows).clamp(1, 5);
    let roof = podium + floors * floor_rows;
    stack.push_solid(x0, x0 + w, 0, podium);
    stack.push(x0, x0 + w, podium, roof);

    // Parapet: segments alternate between a raised dark lip and nothing.
    let segments = rng.rand_int(2, 4) as i32;
    let segment_w = w / segments;
    for index in 0..segments {
        let a = x0 + index * segment_w;
        let b = if index + 1 == segments {
            x0 + w
        } else {
            a + segment_w
        };
        if rng.next_f64() < 0.6 {
            stack.push_dark(a, b, roof, roof + 1);
        }
    }

    // Corner element: a slightly taller block at one end.
    let corner_w = rng.rand_int(3, 5) as i32;
    let (a, b) = if rng.next_f64() < 0.5 {
        (x0, x0 + corner_w)
    } else {
        (x0 + w - corner_w, x0 + w)
    };
    stack.push(a, b, roof, (roof + corner_rows).min(h));
}

/// Wide, low warehouse: dark loading-dock doors along the ground, a body,
/// and either a flat roof or a very shallow gable.
fn warehouse(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let low = h.min(rng.rand_int(6, 12) as i32);
    let gable_rows = if rng.next_f64() < 0.5 { 2 } else { 0 };
    let body_top = (low - gable_rows).max(3);
    // Dock row: alternating wall and door segments, each 2 rows tall.
    let door_w = rng.rand_int(2, 3) as i32;
    let mut x = x0;
    let mut is_door = false;
    while x < x0 + w {
        let span = if is_door {
            door_w
        } else {
            rng.rand_int(2, 5) as i32
        };
        let end = (x + span).min(x0 + w);
        if is_door {
            stack.push_dark(x, end, 0, 2);
        } else {
            stack.push(x, end, 0, 2);
        }
        x = end;
        is_door = !is_door;
    }
    stack.push(x0, x0 + w, 2, body_top);
    let step = (w / 8).max(2);
    for row in 0..gable_rows {
        let inset = (row + 1) * step;
        if body_top + row < h {
            stack.push_dark(
                x0 + inset,
                x0 + w - inset,
                body_top + row,
                body_top + row + 1,
            );
        }
    }
}

/// Half-cylinder Quonset hut: windowed lower walls under a dark curved roof.
fn quonset(stack: &mut Stack, x0: i32, w: i32, h: i32) {
    let rows = (w / 3).clamp(4, 10).min(h);
    let walls = (rows * 2) / 5;
    for row in 0..rows {
        let y = (f64::from(row) + 0.5) / f64::from(rows);
        let row_w = js_round(f64::from(w) * (1.0 - y * y).sqrt()) as i32;
        let row_w = match_parity(row_w.min(w), w, 1);
        let (a, b) = centered(x0, w, row_w);
        if row < walls {
            stack.push(a, b, row, row + 1);
        } else {
            stack.push_dark(a, b, row, row + 1);
        }
    }
}

/// Row of silos or tanks beside a taller headhouse, with either domed caps
/// or a conveyor gallery across the tops.
fn silos(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let head_w = rng.rand_int(3, 4) as i32;
    let head_top = h.min(rng.rand_int(12, 20) as i32);
    let silo_top = (head_top - rng.rand_int(2, 5) as i32).max(6).min(head_top);
    let head_left = rng.next_f64() < 0.5;
    let (head_a, head_b) = if head_left {
        (x0, x0 + head_w)
    } else {
        (x0 + w - head_w, x0 + w)
    };
    stack.push(head_a, head_b, 0, head_top);

    let silo_w = rng.rand_int(3, 4) as i32;
    let (mut x, end) = if head_left {
        (head_b + 1, x0 + w)
    } else {
        (x0, head_a - 1)
    };
    let gallery = rng.next_f64() < 0.5 && silo_top < h;
    let mut first = i32::MAX;
    let mut last = i32::MIN;
    while x + silo_w <= end {
        stack.push_dark(x, x + silo_w, 0, silo_top);
        if !gallery && silo_top < h {
            stack.push_dark(x + 1, x + silo_w - 1, silo_top, silo_top + 1);
        }
        first = first.min(x);
        last = last.max(x + silo_w);
        x += silo_w + 1;
    }
    if gallery && first < last {
        // The gallery reaches the headhouse so it rests on real structure.
        let (a, b) = if head_left {
            (head_a, last)
        } else {
            (first, head_b)
        };
        stack.push_dark(a, b, silo_top, silo_top + 1);
    }
}

/// Big-box store: low body, dark parapet, and a lit sign over the entrance.
fn big_box(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let low = (h - 2).min(rng.rand_int(6, 9) as i32).max(3);
    stack.push(x0, x0 + w, 0, low - 1);
    stack.push_dark(x0, x0 + w, low - 1, low);
    let sign_w = (w / 4).clamp(3, 10);
    let offset = rng.rand_int(0, i64::from(w - sign_w)) as i32;
    stack.push_solid(x0 + offset, x0 + offset + sign_w, low, (low + 2).min(h));
}

/// Parking garage: dark slabs between lit open decks, plus a stair core.
fn parking_garage(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let decks = (rng.rand_int(3, 6) as i32).min((h - 2) / 2).max(1);
    for deck in 0..decks {
        stack.push_dark(x0, x0 + w, deck * 2, deck * 2 + 1);
        stack.push_solid(x0, x0 + w, deck * 2 + 1, deck * 2 + 2);
    }
    let roof = decks * 2;
    stack.push_dark(x0, x0 + w, roof, roof + 1);
    let core_w = rng.rand_int(2, 3) as i32;
    let core_x = if rng.next_f64() < 0.5 {
        x0 + 1
    } else {
        x0 + w - 1 - core_w
    };
    stack.push_dark(core_x, core_x + core_w, roof + 1, (roof + 3).min(h));
}

/// Glass tower whose sides step in one column at a time all the way up.
fn taper(stack: &mut Stack, x0: i32, w: i32, h: i32) {
    let steps = ((w - 3) / 2).max(1);
    let rows_per_step = (h / (steps + 1)).max(2);
    let (mut a, mut b, mut floor) = (x0, x0 + w, 0);
    while floor < h {
        let narrowest = b - a - 2 < 3;
        let top = if narrowest {
            h
        } else {
            (floor + rows_per_step).min(h)
        };
        stack.push(a, b, floor, top);
        floor = top;
        if !narrowest {
            a += 1;
            b -= 1;
        }
    }
}

/// Tower with an upper block that overhangs one side.
fn cantilever(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let base_w = portion(w, frac(rng, 0.55, 0.7), 3, w - 2);
    let block_rows = (rng.rand_int(4, 8) as i32).min(h / 3).max(2);
    let base_top = portion(h, frac(rng, 0.5, 0.65), 2, h - block_rows);
    let base_left = rng.next_f64() < 0.5;
    let (a, b) = if base_left {
        (x0, x0 + base_w)
    } else {
        (x0 + w - base_w, x0 + w)
    };
    stack.push(a, b, 0, base_top);
    stack.push(x0, x0 + w, base_top, base_top + block_rows);
    stack.push(a, b, base_top + block_rows, h);
}

/// Setbacks that step down on one side only, like a terraced hillside.
fn terraced(stack: &mut Stack, x0: i32, w: i32, h: i32, rng: &mut Mulberry32) {
    let base = portion(h, frac(rng, 0.3, 0.45), 2, h - 1);
    stack.push(x0, x0 + w, 0, base);
    let from_left = rng.next_f64() < 0.5;
    let (mut a, mut b, mut floor) = (x0, x0 + w, base);
    while floor < h {
        let cut = rng.rand_int(2, 3) as i32;
        if b - a - cut < 3 {
            break;
        }
        if from_left {
            a += cut;
        } else {
            b -= cut;
        }
        let top = (floor + rng.rand_int(2, 4) as i32).min(h);
        stack.push(a, b, floor, top);
        floor = top;
    }
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

    /// Tiers for `kind` at `width x height`, from a fixed seed.
    fn shape(kind: BuildingKind, width: i32, height: i32, seed: u32) -> Vec<Tier> {
        let mut rng = Mulberry32::new(seed);
        let (tiers, used) = build_tiers(kind, 0, width, height, 200, &mut rng);
        assert_eq!(used, kind, "{kind:?} fits {width}x{height}");
        tiers
    }

    #[test]
    fn spires_have_no_dark_spike_their_mast_is_the_spire() {
        for seed in 0..20 {
            let tiers = shape(BuildingKind::Spire, 11, 60, seed);
            assert!(tiers.iter().all(|t| !t.dark), "seed {seed}");
            assert!((2..=3).contains(&tiers.len()), "1-2 setbacks: {tiers:?}");
            assert_eq!(tiers.iter().map(|t| t.h1).max(), Some(60));
        }
    }

    #[test]
    fn twin_towers_are_pointed_and_joined_by_a_lit_bridge() {
        for seed in 0..20 {
            let tiers = shape(BuildingKind::Twin, 15, 60, seed);
            let bridge: Vec<&Tier> = tiers.iter().filter(|t| t.solid).collect();
            assert_eq!(bridge.len(), 1, "seed {seed}");
            assert_eq!((bridge[0].x0, bridge[0].x1), (0, 15), "spans both towers");
            // Each tower ends in dark roof rows narrowing to its middle.
            for half in [0..7, 8..15] {
                let roof: Vec<&Tier> = tiers
                    .iter()
                    .filter(|t| t.dark && half.contains(&t.x0))
                    .collect();
                assert!(roof.len() >= 2, "seed {seed}: {tiers:?}");
                assert!(roof.windows(2).all(|r| r[1].width() <= r[0].width()));
            }
        }
    }

    #[test]
    fn onion_domes_overhang_a_narrower_drum() {
        let tiers = shape(BuildingKind::Onion, 11, 50, 1);
        let dome_widest = tiers
            .iter()
            .filter(|t| t.dark)
            .map(Tier::width)
            .max()
            .unwrap();
        let drum = tiers
            .iter()
            .filter(|t| !t.dark && t.width() < 11)
            .min_by_key(|t| t.width())
            .expect("a drum");
        assert!(dome_widest > drum.width(), "the dome overhangs the drum");
        let top = tiers.iter().max_by_key(|t| t.h1).unwrap();
        assert!(top.dark && top.width() >= 1);
    }

    #[test]
    fn clock_sections_overhang_the_shaft_under_a_belfry() {
        for seed in 0..10 {
            let tiers = shape(BuildingKind::Clocktower, 11, 60, seed);
            let clock = tiers.iter().find(|t| t.solid).expect("a clock section");
            let below = tiers.iter().find(|t| t.h1 == clock.h0).expect("the shaft");
            let above = tiers.iter().find(|t| t.h0 == clock.h1).expect("the belfry");
            assert!(
                clock.width() > below.width(),
                "seed {seed}: overhangs the shaft"
            );
            assert!(above.width() < clock.width() && !above.dark && !above.solid);
        }
    }

    #[test]
    fn pagodas_end_in_a_small_pointed_cap() {
        let tiers = shape(BuildingKind::Pagoda, 13, 40, 2);
        let mut top: Vec<&Tier> = tiers.iter().filter(|t| t.dark).collect();
        top.sort_by_key(|t| t.h0);
        let (cap, tip) = (top[top.len() - 2], top[top.len() - 1]);
        assert_eq!((cap.width(), tip.width(), tip.h0), (3, 1, cap.h1));
    }

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
            mast: None,
            decoration: None,
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
        assert_eq!(cases, KIND_WEIGHTS.len() * 50 * 12 * 6);
    }

    #[test]
    fn kinds_keep_their_shape_when_they_fit() {
        for (kind, _) in KIND_WEIGHTS {
            let width = kind
                .min_width()
                .max(9)
                .max(kind.own_width_range(3).map_or(0, |range| range.0 as i32));
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
