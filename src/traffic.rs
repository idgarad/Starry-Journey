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

use crate::model::grid::Grid;
use crate::model::palette::{Palette, Rgb};
use crate::model::rng::{ambient_seed, js_round, Mulberry32};
use crate::raster::mix;
use crate::street::CellPaint;

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
            Lane::Upper => f64::from(self.cols) + 1.0 + stagger,
            Lane::Lower => -f64::from(length) - 1.0 - stagger,
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
                Lane::Upper => vehicle.x + f64::from(vehicle.length) < -1.0,
                Lane::Lower => vehicle.x > f64::from(self.cols) + 1.0,
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

    /// Paints headlight beams, then vehicles, onto `paint` (cells inside the
    /// grid only). Beams are drawn first so vehicles stay on top of them.
    pub fn paint(&self, grid: &Grid, palette: &Palette, paint: &mut CellPaint) {
        let mut push = |x: i32, row: i32, color: Rgb| {
            if (0..grid.cols).contains(&x) && (0..grid.rows).contains(&row) {
                paint.push((row as usize * grid.cols as usize + x as usize, color));
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
    fn same_seed_gives_the_same_traffic() {
        let mut a = Traffic::new(42, 640);
        let mut b = Traffic::new(42, 640);
        run(&mut a, 30.0);
        run(&mut b, 30.0);
        assert_eq!(a, b);
    }
}
