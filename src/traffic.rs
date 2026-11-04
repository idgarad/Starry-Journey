//! Street traffic with headlights (Rebuild Prompt, road paragraph; Starry
//! Journey extensions: traffic in Normal mode once the city is finished,
//! and headlights that actually light the road at night).
//!
//! Two opposing lanes run along the seven-row road: the upper lane (rows
//! `ground + 1..=ground + 2`) drives left, the lower lane (rows
//! `ground + 4..=ground + 5`) drives right. Cars are 4 to 5 cells long,
//! trucks 7 to 9 (20% of vehicles); speeds are 9 to 18 cells per second
//! times the speed setting. Vehicles enter from off-screen, never overlap
//! in a lane (a faster vehicle drops back behind a slower one), and are
//! replaced with fresh ones after leaving the screen.
//!
//! Positions are world columns. In Normal mode the view stays at 0, so
//! world and screen columns agree; in Endless mode [`Traffic::set_view`]
//! follows the road camera, vehicles drive relative to the road, and they
//! enter and leave relative to the moving view.

use crate::model::grid::Grid;
use crate::model::palette::{Palette, Rgb};
use crate::model::rng::{ambient_seed, js_round, Mulberry32};
use crate::raster::mix;
use crate::street::{CellPaint, Span};

/// Seed salt for the traffic stream (kept apart from the star stream that
/// also derives from the ambient seed).
pub const TRAFFIC_SALT: u32 = 0x3c6e_f372;
/// Fewest and most vehicles on screen.
pub const VEHICLE_COUNT: (usize, usize) = (2, 6);
/// Screen columns per vehicle when choosing the count (assumption AS-R009;
/// the spec says only "2..6 based on viewport width").
pub const COLUMNS_PER_VEHICLE: f64 = 100.0;
/// Inclusive car and truck length ranges in cells.
pub const CAR_LENGTH: (i64, i64) = (4, 5);
/// Inclusive truck length range in cells.
pub const TRUCK_LENGTH: (i64, i64) = (7, 9);
/// Chance a vehicle is a truck.
pub const TRUCK_CHANCE: f64 = 0.2;
/// Speed range in cells per second at 1x.
pub const SPEED_RANGE: (f64, f64) = (9.0, 18.0);
/// Smallest gap kept between vehicles in a lane, in cells.
pub const FOLLOW_GAP: f64 = 2.0;
/// Largest motion step per frame in seconds (Rebuild Prompt: at most 1/30).
pub const MAX_MOTION_DT: f64 = 1.0 / 30.0;
/// Headlight beam strength by distance ahead of the car (1 to 4 cells).
pub const BEAM: [f64; 4] = [0.55, 0.38, 0.22, 0.1];
/// Beam cells from this distance on also light the vehicle's upper row.
pub const BEAM_SPREAD_FROM: usize = 2;
/// How far ahead headlights light up roadside trees, in cells.
pub const GLOW_REACH: i32 = 8;
/// Tree tint toward the headlight color just ahead of the bumper.
pub const GLOW_STRENGTH: f64 = 0.45;
/// How many rows the upper lane's cone rises per cell ahead, so it reaches
/// the background trees above the road (stylized, owner request 2026-11-03).
pub const UPPER_CONE_TILT: f64 = 0.75;

/// Which row of roadside trees a glow query is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verge {
    /// Background trees along the top edge of the road: lit by the upper
    /// lane only.
    Back,
    /// Foreground trees at the bottom of the screen: lit by the lower lane
    /// only.
    Front,
}

impl Lane {
    /// The verge this lane's headlights light.
    pub fn verge(self) -> Verge {
        match self {
            Lane::Upper => Verge::Back,
            Lane::Lower => Verge::Front,
        }
    }

    /// Rows the cone's center rises per cell ahead.
    pub fn cone_tilt(self) -> f64 {
        match self {
            Lane::Upper => UPPER_CONE_TILT,
            Lane::Lower => 0.0,
        }
    }
}

/// Direction of travel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    /// Upper (far) lane, driving left.
    Upper,
    /// Lower (near) lane, driving right.
    Lower,
}

impl Lane {
    /// Row offset of the lane's top row below the ground row.
    pub fn top_offset(self) -> i32 {
        match self {
            Lane::Upper => 1,
            Lane::Lower => 4,
        }
    }

    /// +1 for rightward, -1 for leftward.
    pub fn direction(self) -> f64 {
        match self {
            Lane::Upper => -1.0,
            Lane::Lower => 1.0,
        }
    }
}

/// One vehicle.
#[derive(Debug, Clone, PartialEq)]
pub struct Vehicle {
    /// Lane and direction.
    pub lane: Lane,
    /// Left edge in cells (continuous).
    pub x: f64,
    /// Length in cells.
    pub length: i32,
    /// True for trucks.
    pub truck: bool,
    /// Speed in cells per second at 1x.
    pub speed: f64,
    /// Index into the palette's vehicle colors.
    pub color: usize,
}

impl Vehicle {
    /// Column of the front (headlight) cell.
    pub fn front(&self) -> i32 {
        let left = js_round(self.x) as i32;
        match self.lane {
            Lane::Upper => left,
            Lane::Lower => left + self.length - 1,
        }
    }

    /// Column of the rear (taillight) cell.
    pub fn rear(&self) -> i32 {
        let left = js_round(self.x) as i32;
        match self.lane {
            Lane::Upper => left + self.length - 1,
            Lane::Lower => left,
        }
    }
}

/// All traffic on one road.
#[derive(Debug, Clone, PartialEq)]
pub struct Traffic {
    rng: Mulberry32,
    cols: i32,
    /// World column at the screen's left edge (the road camera).
    view_x: f64,
    /// Vehicles currently on (or approaching) the road.
    pub vehicles: Vec<Vehicle>,
}

impl Traffic {
    /// Starts traffic for a road `cols` wide. Every vehicle begins
    /// off-screen and drives in, staggered.
    pub fn new(scene_seed: u32, cols: i32) -> Self {
        let mut traffic = Self {
            rng: Mulberry32::new(ambient_seed(scene_seed) ^ TRAFFIC_SALT),
            cols: cols.max(1),
            view_x: 0.0,
            vehicles: Vec::new(),
        };
        let count = vehicle_count(cols);
        for index in 0..count {
            // Alternate lanes so both lanes get traffic.
            let lane = if index % 2 == 0 {
                Lane::Upper
            } else {
                Lane::Lower
            };
            let vehicle = traffic.spawn(lane);
            traffic.vehicles.push(vehicle);
        }
        traffic.keep_gaps();
        traffic
    }

    /// Moves the view (the road camera) to world column `view_x`. Vehicles
    /// keep their world positions; entries and exits follow the view.
    pub fn set_view(&mut self, view_x: f64) {
        if view_x.is_finite() {
            self.view_x = view_x;
        }
    }

    /// The world column at the screen's left edge.
    pub fn view_x(&self) -> f64 {
        self.view_x
    }

    /// A fresh vehicle waiting off-screen at the lane's entry, staggered by
    /// up to half a screen so arrivals are spread out.
    fn spawn(&mut self, lane: Lane) -> Vehicle {
        let rng = &mut self.rng;
        let truck = rng.next_f64() < TRUCK_CHANCE;
        let (lo, hi) = if truck { TRUCK_LENGTH } else { CAR_LENGTH };
        let length = rng.rand_int(lo, hi) as i32;
        let speed = SPEED_RANGE.0 + (SPEED_RANGE.1 - SPEED_RANGE.0) * rng.next_f64();
        let color = rng.rand_int(0, 255) as usize;
        let stagger = rng.next_f64() * f64::from(self.cols) / 2.0;
        let x = match lane {
            Lane::Upper => self.view_x + f64::from(self.cols) + 1.0 + stagger,
            Lane::Lower => self.view_x - f64::from(length) - 1.0 - stagger,
        };
        Vehicle {
            lane,
            x,
            length,
            truck,
            speed,
            color,
        }
    }

    /// Moves traffic by `dt` seconds (clamped to 1/30 s) at `speed`
    /// multiplier, keeps gaps, and replaces vehicles that left the screen.
    /// O(vehicles²), with at most 6 vehicles.
    pub fn step(&mut self, dt: f64, speed: f64) {
        let dt = dt.clamp(0.0, MAX_MOTION_DT);
        for vehicle in &mut self.vehicles {
            vehicle.x += vehicle.lane.direction() * vehicle.speed * speed * dt;
        }
        self.keep_gaps();
        for index in 0..self.vehicles.len() {
            let vehicle = &self.vehicles[index];
            let gone = match vehicle.lane {
                Lane::Upper => vehicle.x + f64::from(vehicle.length) < self.view_x - 1.0,
                Lane::Lower => vehicle.x > self.view_x + f64::from(self.cols) + 1.0,
            };
            if gone {
                let lane = vehicle.lane;
                self.vehicles[index] = self.spawn(lane);
            }
        }
        self.keep_gaps();
    }

    /// Pulls back any vehicle that would overlap the one ahead of it in its
    /// lane, so it follows at [`FOLLOW_GAP`] and matches the leader's pace.
    fn keep_gaps(&mut self) {
        for lane in [Lane::Upper, Lane::Lower] {
            let mut order: Vec<usize> = (0..self.vehicles.len())
                .filter(|&i| self.vehicles[i].lane == lane)
                .collect();
            // Leader first: furthest along in the direction of travel.
            order.sort_by(|&a, &b| {
                let (xa, xb) = (self.vehicles[a].x, self.vehicles[b].x);
                match lane {
                    Lane::Lower => xb.total_cmp(&xa),
                    Lane::Upper => xa.total_cmp(&xb),
                }
            });
            for pair in order.windows(2) {
                let (leader, follower) = (pair[0], pair[1]);
                let lead = &self.vehicles[leader];
                let (lead_x, lead_len) = (lead.x, f64::from(lead.length));
                let follower = &mut self.vehicles[follower];
                match lane {
                    Lane::Lower => {
                        let limit = lead_x - FOLLOW_GAP - f64::from(follower.length);
                        follower.x = follower.x.min(limit);
                    }
                    Lane::Upper => {
                        let limit = lead_x + lead_len + FOLLOW_GAP;
                        follower.x = follower.x.max(limit);
                    }
                }
            }
        }
    }

    /// How strongly headlights light the tree cell `(x, row)` on `verge`,
    /// from 0 to [`GLOW_STRENGTH`]: the brightest cone among the vehicles in
    /// the lane that lights that verge (upper lane for background trees,
    /// lower lane for foreground trees).
    ///
    /// Each cone starts at the headlight and reaches [`GLOW_REACH`] cells
    /// forward, widening by one row on each side every two cells. The upper
    /// lane's cone also rises [`UPPER_CONE_TILT`] rows per cell so it reaches
    /// the trees above the road. Strength falls off with distance ahead and
    /// with distance from the cone's center. Nothing behind a vehicle is lit.
    /// O(vehicles).
    pub fn glow(&self, grid: &Grid, x: i32, row: i32, verge: Verge) -> f64 {
        self.vehicles
            .iter()
            .filter(|vehicle| vehicle.lane.verge() == verge)
            .map(|vehicle| {
                let beam_row = grid.ground_row + vehicle.lane.top_offset() + 1;
                let ahead = (x - vehicle.front()) * vehicle.lane.direction() as i32;
                if !(1..=GLOW_REACH).contains(&ahead) {
                    return 0.0;
                }
                let center = f64::from(beam_row) - vehicle.lane.cone_tilt() * f64::from(ahead);
                let spread = f64::from(1 + ahead / 2);
                let off_axis = (f64::from(row) - center).abs();
                if off_axis > spread {
                    return 0.0;
                }
                let along = 1.0 - f64::from(ahead - 1) / f64::from(GLOW_REACH);
                let across = 1.0 - off_axis / (spread + 1.0);
                GLOW_STRENGTH * along * across
            })
            .fold(0.0, f64::max)
    }

    /// Paints headlight beams, then vehicles, onto `paint` (cells inside the
    /// grid only). Beams are drawn first so vehicles stay on top of them.
    pub fn paint(&self, grid: &Grid, palette: &Palette, paint: &mut CellPaint) {
        self.paint_span(grid, Span::screen(grid), palette, paint);
    }

    /// Like [`Traffic::paint`], into a buffer covering the world strip
    /// `span` (cells outside it are skipped).
    pub fn paint_span(&self, grid: &Grid, span: Span, palette: &Palette, paint: &mut CellPaint) {
        let mut push = |x: i32, row: i32, color: Rgb| {
            if let Some(cell) = span.index(x, row) {
                paint.push((cell, color));
            }
        };
        for vehicle in &self.vehicles {
            let top = grid.ground_row + vehicle.lane.top_offset();
            let bottom = top + 1;
            let ahead = vehicle.lane.direction() as i32;
            for (step, strength) in BEAM.iter().enumerate() {
                let x = vehicle.front() + ahead * (step as i32 + 1);
                let lit = mix(palette.road, palette.headlight, *strength);
                push(x, bottom, lit);
                if step >= BEAM_SPREAD_FROM {
                    push(x, top, mix(palette.road, palette.headlight, strength * 0.6));
                }
            }
        }
        for vehicle in &self.vehicles {
            let top = grid.ground_row + vehicle.lane.top_offset();
            let bottom = top + 1;
            let body = palette.vehicles[vehicle.color % palette.vehicles.len()];
            let left = js_round(vehicle.x) as i32;
            for x in left..left + vehicle.length {
                push(x, bottom, body);
            }
            // Upper row: a cabin inset from both ends for cars, the full
            // trailer except the front cell for trucks.
            let (front, rear) = (vehicle.front(), vehicle.rear());
            for x in left..left + vehicle.length {
                let cabin = if vehicle.truck {
                    x != front
                } else {
                    x != front && x != rear
                };
                if cabin {
                    push(x, top, body);
                }
            }
            push(front, bottom, palette.headlight);
            push(rear, bottom, palette.taillight);
        }
    }
}

/// Vehicles for a road `cols` wide: `clamp(round(cols / 100), 2, 6)`.
pub fn vehicle_count(cols: i32) -> usize {
    let wanted = js_round(f64::from(cols) / COLUMNS_PER_VEHICLE) as usize;
    wanted.clamp(VEHICLE_COUNT.0, VEHICLE_COUNT.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::grid::CellScaling;
    use crate::model::palette::palette_by_key;

    fn grid() -> Grid {
        Grid::with_scaling(1920, 1080, true, CellScaling::Fixed(3))
    }

    fn run(traffic: &mut Traffic, seconds: f64) {
        for _ in 0..(seconds * 60.0) as usize {
            traffic.step(1.0 / 60.0, 1.0);
        }
    }

    #[test]
    fn vehicle_count_follows_width_within_two_to_six() {
        assert_eq!(vehicle_count(50), 2);
        assert_eq!(vehicle_count(427), 4);
        assert_eq!(vehicle_count(640), 6);
        assert_eq!(vehicle_count(2000), 6);
    }

    #[test]
    fn traffic_starts_off_screen_in_both_lanes_and_drives_in() {
        let grid = grid();
        let mut traffic = Traffic::new(7, grid.cols);
        assert_eq!(traffic.vehicles.len(), 6);
        assert!(traffic.vehicles.iter().any(|v| v.lane == Lane::Upper));
        assert!(traffic.vehicles.iter().any(|v| v.lane == Lane::Lower));
        let on_screen = |t: &Traffic| {
            t.vehicles
                .iter()
                .filter(|v| v.x + f64::from(v.length) > 0.0 && v.x < f64::from(grid.cols))
                .count()
        };
        assert_eq!(on_screen(&traffic), 0, "nothing on the road at first");
        run(&mut traffic, 20.0);
        assert!(on_screen(&traffic) > 0, "cars have arrived");
    }

    #[test]
    fn lanes_drive_in_opposite_directions_at_spec_speeds() {
        let mut traffic = Traffic::new(3, 640);
        let before: Vec<f64> = traffic.vehicles.iter().map(|v| v.x).collect();
        traffic.step(1.0 / 60.0, 1.0);
        for (vehicle, x) in traffic.vehicles.iter().zip(before) {
            assert!((SPEED_RANGE.0..SPEED_RANGE.1).contains(&vehicle.speed));
            let moved = vehicle.x - x;
            match vehicle.lane {
                Lane::Upper => assert!(moved <= 0.0),
                Lane::Lower => assert!(moved >= 0.0),
            }
        }
    }

    #[test]
    fn traffic_keeps_flowing_past_a_moving_view() {
        // The Endless road camera at 4x: 8 cells per second to the right.
        let cols = 640;
        let mut traffic = Traffic::new(13, cols);
        let mut view = 0.0;
        let mut on_screen_frames = 0;
        for _ in 0..(10 * 60 * 60) {
            view += 8.0 / 60.0;
            traffic.set_view(view);
            traffic.step(1.0 / 60.0, 4.0);
            assert_eq!(traffic.vehicles.len(), 6);
            let visible = traffic
                .vehicles
                .iter()
                .any(|v| v.x + f64::from(v.length) > view && v.x < view + f64::from(cols));
            on_screen_frames += usize::from(visible);
            for vehicle in &traffic.vehicles {
                // Waiting vehicles stay within a screen and a half of the
                // view; none is left behind for good.
                assert!(vehicle.x > view - 1.5 * f64::from(cols), "{}", vehicle.x);
                assert!(vehicle.x < view + 2.5 * f64::from(cols), "{}", vehicle.x);
            }
        }
        assert!(
            on_screen_frames > 10 * 60 * 60 * 9 / 10,
            "{on_screen_frames}"
        );
        // A new vehicle enters just off-screen relative to the view.
        let mut fresh = Traffic::new(14, cols);
        fresh.set_view(1000.0);
        let spawned = fresh.spawn(Lane::Upper);
        assert!(spawned.x > 1000.0 + f64::from(cols));
        let spawned = fresh.spawn(Lane::Lower);
        assert!(spawned.x + f64::from(spawned.length) < 1000.0);
        fresh.set_view(f64::NAN);
        assert_eq!(fresh.view_x(), 1000.0, "a bad view is ignored");
    }

    #[test]
    fn vehicles_never_overlap_in_a_lane_and_the_count_is_kept() {
        let mut traffic = Traffic::new(11, 640);
        for _ in 0..(120 * 60) {
            traffic.step(1.0 / 60.0, 4.0);
            assert_eq!(traffic.vehicles.len(), 6);
            for lane in [Lane::Upper, Lane::Lower] {
                let mut spans: Vec<(f64, f64)> = traffic
                    .vehicles
                    .iter()
                    .filter(|v| v.lane == lane)
                    .map(|v| (v.x, v.x + f64::from(v.length)))
                    .collect();
                spans.sort_by(|a, b| a.0.total_cmp(&b.0));
                for pair in spans.windows(2) {
                    assert!(
                        pair[1].0 - pair[0].1 >= FOLLOW_GAP - 1e-9,
                        "overlap: {pair:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn sizes_and_truck_share_follow_the_spec() {
        let mut traffic = Traffic::new(5, 640);
        let mut trucks = 0;
        let total = 5_000;
        for _ in 0..total {
            let vehicle = traffic.spawn(Lane::Lower);
            let range = if vehicle.truck {
                TRUCK_LENGTH
            } else {
                CAR_LENGTH
            };
            assert!((range.0..=range.1).contains(&i64::from(vehicle.length)));
            trucks += usize::from(vehicle.truck);
        }
        let share = trucks as f64 / f64::from(total);
        assert!((0.17..0.23).contains(&share), "truck share {share}");
    }

    #[test]
    fn long_frames_are_clamped_and_speed_scales_motion() {
        let mut a = Traffic::new(9, 640);
        let mut b = a.clone();
        let mut c = a.clone();
        a.step(1.0, 1.0); // clamped to 1/30 s
        b.step(1.0 / 30.0, 1.0);
        assert_eq!(a, b);
        c.step(1.0 / 60.0, 2.0);
        let mut d = Traffic::new(9, 640);
        d.step(1.0 / 30.0, 1.0);
        for (x, y) in c.vehicles.iter().zip(&d.vehicles) {
            assert!((x.x - y.x).abs() < 1e-9, "2x for 1/60 s = 1x for 1/30 s");
        }
    }

    #[test]
    fn headlights_lead_taillights_trail_and_beams_light_the_road_ahead() {
        let grid = grid();
        let classic = palette_by_key("classic").unwrap();
        let mut traffic = Traffic::new(1, grid.cols);
        traffic.vehicles = vec![
            Vehicle {
                lane: Lane::Lower,
                x: 100.0,
                length: 5,
                truck: false,
                speed: 10.0,
                color: 0,
            },
            Vehicle {
                lane: Lane::Upper,
                x: 300.0,
                length: 8,
                truck: true,
                speed: 10.0,
                color: 1,
            },
        ];
        let mut paint = CellPaint::new();
        traffic.paint(&grid, classic, &mut paint);
        let mut frame = std::collections::HashMap::new();
        for (cell, color) in paint {
            frame.insert(cell, color); // later paints win, as in the frame
        }
        let at = |x: i32, row: i32| {
            frame
                .get(&(row as usize * grid.cols as usize + x as usize))
                .copied()
        };
        let lower_bottom = grid.ground_row + 5;
        let upper_bottom = grid.ground_row + 2;
        // Right-driving car: headlight at the right end, taillight at the left.
        assert_eq!(at(104, lower_bottom), Some(classic.headlight));
        assert_eq!(at(100, lower_bottom), Some(classic.taillight));
        assert_eq!(
            at(105, lower_bottom),
            Some(mix(classic.road, classic.headlight, BEAM[0]))
        );
        assert_eq!(
            at(108, lower_bottom),
            Some(mix(classic.road, classic.headlight, BEAM[3]))
        );
        assert!(at(105, lower_bottom - 1).is_none(), "beam starts low");
        assert!(
            at(107, lower_bottom - 1).is_some(),
            "then spreads to both rows"
        );
        assert!(at(99, lower_bottom).is_none(), "nothing lit behind");
        // Left-driving truck: headlight at the left end.
        assert_eq!(at(300, upper_bottom), Some(classic.headlight));
        assert_eq!(at(307, upper_bottom), Some(classic.taillight));
        assert_eq!(
            at(299, upper_bottom),
            Some(mix(classic.road, classic.headlight, BEAM[0]))
        );
        assert_eq!(
            at(301, upper_bottom - 1),
            Some(classic.vehicles[1]),
            "trailer roof"
        );
        assert!(
            at(300, upper_bottom - 1).is_none(),
            "cab front is one row lower"
        );
    }

    #[test]
    fn headlights_glow_on_cells_ahead_but_not_behind_or_far_off() {
        let grid = grid();
        let mut traffic = Traffic::new(1, grid.cols);
        traffic.vehicles = vec![Vehicle {
            lane: Lane::Lower,
            x: 100.0,
            length: 5,
            truck: false,
            speed: 10.0,
            color: 0,
        }];
        let beam_row = grid.ground_row + 5;
        let front = 104;
        let near = traffic.glow(&grid, front + 1, beam_row, Verge::Front);
        let far = traffic.glow(&grid, front + GLOW_REACH, beam_row, Verge::Front);
        assert!(
            near > far && far > 0.0,
            "fades with distance: {near} > {far}"
        );
        assert!(
            (near - GLOW_STRENGTH).abs() < 1e-9,
            "full strength at the bumper"
        );
        assert_eq!(
            traffic.glow(&grid, front + GLOW_REACH + 1, beam_row, Verge::Front),
            0.0,
            "out of reach"
        );
        assert_eq!(
            traffic.glow(&grid, front, beam_row, Verge::Front),
            0.0,
            "not the car itself"
        );
        assert_eq!(
            traffic.glow(&grid, 99, beam_row, Verge::Front),
            0.0,
            "nothing behind"
        );
        // The cone widens: off-axis rows light up further ahead.
        assert_eq!(
            traffic.glow(&grid, front + 1, beam_row - 3, Verge::Front),
            0.0
        );
        assert!(traffic.glow(&grid, front + 6, beam_row - 3, Verge::Front) > 0.0);
        assert!(
            traffic.glow(&grid, front + 6, beam_row - 3, Verge::Front)
                < traffic.glow(&grid, front + 6, beam_row, Verge::Front),
            "weaker off the beam's row"
        );
        // The lower lane never lights the background trees.
        assert_eq!(traffic.glow(&grid, front + 4, beam_row, Verge::Back), 0.0);

        // Left-driving upper-lane vehicles light to their left, angled up
        // toward the background trees, and never the foreground trees.
        traffic.vehicles[0].lane = Lane::Upper;
        let upper_row = grid.ground_row + 2;
        let upper_front = 100;
        assert!(traffic.glow(&grid, upper_front - 1, upper_row, Verge::Back) > 0.0);
        assert_eq!(
            traffic.glow(&grid, upper_front + 6, upper_row, Verge::Back),
            0.0,
            "behind"
        );
        assert_eq!(
            traffic.glow(&grid, upper_front - 4, upper_row, Verge::Front),
            0.0
        );
        // At full reach the cone is centered 6 rows up, on the middle of a
        // background tree (rows ground - 7 .. ground - 1).
        let mid_tree = grid.ground_row - 4;
        let reach = upper_front - GLOW_REACH;
        assert!(traffic.glow(&grid, reach, mid_tree, Verge::Back) > 0.0);
        assert!(
            traffic.glow(&grid, reach, mid_tree, Verge::Back)
                > traffic.glow(&grid, reach, upper_row + 1, Verge::Back),
            "aimed up, not along the road"
        );
    }

    #[test]
    fn same_seed_gives_the_same_traffic() {
        let mut a = Traffic::new(42, 640);
        let mut b = Traffic::new(42, 640);
        run(&mut a, 30.0);
        run(&mut b, 30.0);
        assert_eq!(a, b);
    }
}
