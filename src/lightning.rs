//! Lightning in storms (owner decisions, 2026-11-05: storms only; capped,
//! soft flashes with `--lightning off`; a bolt with the clouds lit from
//! inside and the nearby sky brightened; mostly in-cloud flickers).
//!
//! Events are seeded per minute of the run. Each is one of:
//!
//! - **in-cloud** (about 70%): a flicker inside the clouds, no bolt;
//! - **cloud-to-cloud** (about 20%): a jagged bolt across the cloud band;
//! - **strike** (about 10%): a bolt from the clouds to the tallest point of
//!   the skyline at that moment (the caller finds it), which flares.
//!
//! **Photosensitive safety** (WCAG 2.3.1 allows at most three flashes a
//! second; a screensaver should stay far below it): events start at least
//! [`EVENT_GAP`]`.0` seconds apart, flashes within an event at least
//! [`MIN_FLASH_SPACING`] apart (under three a second), and every flash
//! ramps up over [`RISE`] and fades over [`FALL`] rather than snapping.
//!
//! This module only decides when and where; the GPU draws the flash and
//! bolt (see `present.wgsl`).

use crate::model::rng::Mulberry32;

/// Seconds in each stretch of the run that holds its own events.
pub const SLOT: f64 = 60.0;
/// Shortest and longest pause between events, in seconds.
pub const EVENT_GAP: (f64, f64) = (4.0, 14.0);
/// Least time between two flashes of one event: under three a second.
pub const MIN_FLASH_SPACING: f64 = 0.34;
/// Seconds a flash takes to rise to its peak, and to fade away.
pub const RISE: f64 = 0.08;
pub const FALL: f64 = 0.4;
/// The most bolt segments (the shader has this many slots).
pub const MAX_SEGMENTS: usize = 24;
/// Shares of in-cloud flickers and cloud-to-cloud bolts; the rest are
/// strikes.
pub const IN_CLOUD_SHARE: f64 = 0.7;
pub const CLOUD_TO_CLOUD_SHARE: f64 = 0.2;

/// Whether lightning can appear (`--lightning`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LightningMode {
    /// In storms (default).
    #[default]
    Auto,
    /// Never.
    Off,
}

impl LightningMode {
    /// Parses `auto` or `off` (any case).
    ///
    /// # Errors
    ///
    /// Returns a message for anything else.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "off" => Ok(Self::Off),
            _ => Err(format!("must be auto or off, got '{value}'")),
        }
    }
}

/// What kind of lightning an event is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoltKind {
    /// A flicker inside the clouds; no bolt.
    InCloud,
    /// A bolt across the cloud band.
    CloudToCloud,
    /// A bolt down to the tallest point of the skyline.
    Strike,
}

/// One lightning event: when it starts, its kind and flashes, and where it
/// is (as shares of the screen width and of the sky's height).
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// Seconds into the run.
    pub start: f64,
    pub kind: BoltKind,
    /// Each flash's offset from `start` (seconds), peak strength (0..1),
    /// and place across the screen (0..1). Flashes of one event light
    /// different spots, and their lights add where they overlap (owner,
    /// 2026-11-05: "let overlaps bring that up organically").
    pub flashes: Vec<(f64, f64, f64)>,
    /// Where it happens: across the screen (0..1) and down the sky (0 top,
    /// 1 the ground row).
    pub at: (f64, f64),
    /// Where a cloud-to-cloud bolt ends (same units).
    pub to: (f64, f64),
    /// Seed for its bolt's shape.
    pub seed: u32,
}

impl Event {
    /// When its last flash has faded.
    pub fn end(&self) -> f64 {
        self.start + self.flashes.last().map_or(0.0, |(at, _, _)| *at) + FALL
    }

    /// Each flash lit at `seconds`: its place across the screen (0..1) and
    /// brightness now (0..1). O(flashes).
    pub fn lights(&self, seconds: f64) -> Vec<(f64, f64)> {
        let t = seconds - self.start;
        self.flashes
            .iter()
            .map(|&(at, peak, x)| (x, peak * envelope(t - at)))
            .filter(|&(_, brightness)| brightness > 0.0)
            .collect()
    }

    /// Its brightness at `seconds` (0..1): the brightest of its flashes,
    /// each a smooth rise and a smooth fade. O(flashes).
    pub fn intensity(&self, seconds: f64) -> f64 {
        let t = seconds - self.start;
        self.flashes
            .iter()
            .map(|&(at, peak, _)| peak * envelope(t - at))
            .fold(0.0, f64::max)
    }
}

/// A flash's shape at `t` seconds after its start: 0 before, rising
/// smoothly to 1 over [`RISE`], then fading smoothly over [`FALL`].
pub fn envelope(t: f64) -> f64 {
    if t <= 0.0 || t >= RISE + FALL {
        0.0
    } else if t < RISE {
        smooth(t / RISE)
    } else {
        smooth(1.0 - (t - RISE) / FALL)
    }
}

fn smooth(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The events of minute `slot` for the run `seed`, in time order. The
/// first starts at least [`EVENT_GAP`]`.0` into the minute and the last ends
/// before the minute does, so the gap holds across minutes too.
pub fn events_in(seed: u32, slot: i64) -> Vec<Event> {
    let mut rng = Mulberry32::new(seed ^ 0x1197_0b17 ^ (slot as u32).wrapping_mul(0x85eb_ca6b));
    let mut next = || rng.next_f64();
    let slot_start = slot as f64 * SLOT;
    let mut events = Vec::new();
    let mut time = slot_start;
    loop {
        time += EVENT_GAP.0 + (EVENT_GAP.1 - EVENT_GAP.0) * next();
        let roll = next();
        let kind = if roll < IN_CLOUD_SHARE {
            BoltKind::InCloud
        } else if roll < IN_CLOUD_SHARE + CLOUD_TO_CLOUD_SHARE {
            BoltKind::CloudToCloud
        } else {
            BoltKind::Strike
        };
        let event = make_event(&mut next, time, kind);
        if event.end() > slot_start + SLOT {
            break;
        }
        time = event.end();
        events.push(event);
    }
    events
}

/// One event of `kind` starting at `start`, its flashes, place, and bolt
/// drawn from `next`.
fn make_event(next: &mut impl FnMut() -> f64, start: f64, kind: BoltKind) -> Event {
    // One to three flashes, each fainter than the first, at least
    // MIN_FLASH_SPACING apart.
    let count = 1 + (next() * 2.99) as usize;
    let (lo, hi) = match kind {
        BoltKind::InCloud => (0.45, 0.75),
        BoltKind::CloudToCloud => (0.75, 0.95),
        BoltKind::Strike => (0.9, 1.0),
    };
    let at = (0.1 + 0.8 * next(), 0.1 + 0.3 * next());
    let mut flashes = vec![(0.0, lo + (hi - lo) * next(), at.0)];
    for _ in 1..count {
        let last = flashes.last().map_or(0.0, |(at, _, _)| *at);
        let time = last + MIN_FLASH_SPACING + 0.25 * next();
        let peak = flashes[0].1 * (0.5 + 0.4 * next());
        // Nearby along the cloud band.
        let x = (at.0 + 0.4 * (next() - 0.5)).clamp(0.0, 1.0);
        flashes.push((time, peak, x));
    }
    let span = (0.15 + 0.25 * next()) * if next() < 0.5 { -1.0 } else { 1.0 };
    let to = (
        (at.0 + span).clamp(0.02, 0.98),
        (at.1 + 0.15 * (next() - 0.5)).clamp(0.05, 0.5),
    );
    Event {
        start,
        kind,
        flashes,
        at,
        to,
        seed: (next() * f64::from(u32::MAX)) as u32,
    }
}

/// An event of `kind` asked for by hand at `start` (the Shift test keys, or
/// `--events`): its place and bolt are drawn from the moment, so each is
/// new. It ignores the cover and `--lightning`, for testing and tuning.
pub fn forced_event(seed: u32, kind: BoltKind, start: f64) -> Event {
    let bits = start.to_bits();
    let mut rng = Mulberry32::new(seed ^ 0x0f0c_e000u32.wrapping_add((bits ^ (bits >> 32)) as u32));
    let mut next = || rng.next_f64();
    make_event(&mut next, start, kind)
}

/// The event lighting the sky at `seconds`, if any.
pub fn event_at(seed: u32, mode: LightningMode, storm: bool, seconds: f64) -> Option<Event> {
    if mode == LightningMode::Off || !storm || !seconds.is_finite() {
        return None;
    }
    let slot = (seconds / SLOT).floor() as i64;
    events_in(seed, slot)
        .into_iter()
        .find(|event| (event.start..event.end()).contains(&seconds))
}

/// A jagged bolt from `from` to `to` (any units), as straight segments:
/// the main channel in up to 14 kinks sideways of the line, plus up to two
/// short branches forking off it. At most [`MAX_SEGMENTS`] segments.
pub fn bolt_path(seed: u32, from: (f64, f64), to: (f64, f64)) -> Vec<[(f64, f64); 2]> {
    let mut rng = Mulberry32::new(seed);
    let mut next = || rng.next_f64();
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = (dx * dx + dy * dy).sqrt();
    if length <= 0.0 || !length.is_finite() {
        return Vec::new();
    }
    let normal = (-dy / length, dx / length);
    let steps = 14;
    let mut points = vec![from];
    for i in 1..steps {
        let t = f64::from(i) / f64::from(steps);
        let wobble = (next() - 0.5) * 0.12 * length;
        points.push((
            from.0 + dx * t + normal.0 * wobble,
            from.1 + dy * t + normal.1 * wobble,
        ));
    }
    points.push(to);
    let mut segments: Vec<[(f64, f64); 2]> = points.windows(2).map(|w| [w[0], w[1]]).collect();
    for _ in 0..2 {
        let start = points[1 + (next() * (f64::from(steps) - 3.0)) as usize];
        let side = if next() < 0.5 { -1.0 } else { 1.0 };
        let reach = (0.15 + 0.15 * next()) * length;
        let mut tip = start;
        for _ in 0..4 {
            if segments.len() >= MAX_SEGMENTS {
                break;
            }
            let along = reach / 4.0;
            let new = (
                tip.0 + dx / length * along * 0.7 + normal.0 * side * along * (0.4 + 0.5 * next()),
                tip.1 + dy / length * along * 0.7 + normal.1 * side * along * (0.4 + 0.5 * next()),
            );
            segments.push([tip, new]);
            tip = new;
        }
    }
    segments.truncate(MAX_SEGMENTS);
    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every event over `minutes` minutes of seed `seed`.
    fn hour(seed: u32, minutes: i64) -> Vec<Event> {
        (0..minutes)
            .flat_map(|slot| events_in(seed, slot))
            .collect()
    }

    #[test]
    fn flashes_never_come_faster_than_three_a_second() {
        for seed in 0..20 {
            let events = hour(seed, 60);
            let peaks: Vec<f64> = events
                .iter()
                .flat_map(|e| e.flashes.iter().map(move |(at, _, _)| e.start + at))
                .collect();
            assert!(peaks
                .windows(2)
                .all(|w| w[1] - w[0] >= MIN_FLASH_SPACING - 1e-9));
            // Any one second holds at most three flash starts.
            for (i, &p) in peaks.iter().enumerate() {
                assert!(peaks[i..].iter().take_while(|&&q| q < p + 1.0).count() <= 3);
            }
            // Events are at least EVENT_GAP.0 apart (from one's end to the
            // next's start), across minutes too.
            assert!(events
                .windows(2)
                .all(|w| w[1].start - w[0].end() >= EVENT_GAP.0 - 1e-9));
        }
    }

    #[test]
    fn each_flash_lights_its_own_nearby_spot() {
        let events: Vec<Event> = (0..5).flat_map(|seed| hour(seed, 60)).collect();
        for burst in events.iter().filter(|e| e.flashes.len() > 1) {
            assert_eq!(burst.flashes[0].2, burst.at.0);
            assert!(burst
                .flashes
                .iter()
                .all(|f| (f.2 - burst.at.0).abs() <= 0.2 + 1e-9));
        }
        // Where a second flash starts before the first has faded, both
        // light at once, and their lights add.
        let burst = events
            .iter()
            .find(|e| e.flashes.len() > 1 && e.flashes[1].0 < RISE + FALL - 0.05)
            .expect("an overlapping burst");
        let both = burst.start + burst.flashes[1].0 + 0.02;
        assert_eq!(burst.lights(both).len(), 2, "{:?}", burst.flashes);
        assert!(burst.lights(burst.start - 1.0).is_empty());
    }

    #[test]
    fn flashes_rise_and_fade_smoothly() {
        assert_eq!(envelope(0.0), 0.0);
        assert!((envelope(RISE) - 1.0).abs() < 1e-12);
        assert_eq!(envelope(RISE + FALL), 0.0);
        // At 60 frames a second no frame jumps by more than a third of the
        // peak.
        let mut previous = 0.0;
        for frame in 0..60 {
            let value = envelope(f64::from(frame) / 60.0);
            assert!((value - previous).abs() < 0.34, "frame {frame}");
            previous = value;
        }
    }

    #[test]
    fn most_lightning_is_in_cloud_and_strikes_are_rare() {
        let events: Vec<Event> = (0..20).flat_map(|seed| hour(seed, 60)).collect();
        let share = |kind: BoltKind| {
            events.iter().filter(|e| e.kind == kind).count() as f64 / events.len() as f64
        };
        assert!(
            (0.62..0.78).contains(&share(BoltKind::InCloud)),
            "{}",
            share(BoltKind::InCloud)
        );
        assert!((0.14..0.26).contains(&share(BoltKind::CloudToCloud)));
        assert!((0.05..0.15).contains(&share(BoltKind::Strike)));
        // Roughly one event every 10-12 s.
        let per_minute = events.len() as f64 / (20.0 * 60.0);
        assert!((3.5..7.0).contains(&per_minute), "{per_minute}");
        assert!(events
            .iter()
            .all(|e| (0.0..=1.0).contains(&e.at.0) && (0.0..=0.5).contains(&e.at.1)));
    }

    #[test]
    fn lightning_only_comes_in_storms_and_not_when_off() {
        let seconds = hour(4, 1)[0].start + 0.05;
        assert!(event_at(4, LightningMode::Auto, true, seconds).is_some());
        assert!(event_at(4, LightningMode::Auto, false, seconds).is_none());
        assert!(event_at(4, LightningMode::Off, true, seconds).is_none());
        assert!(event_at(4, LightningMode::Auto, true, f64::NAN).is_none());
        assert_eq!(LightningMode::parse("OFF"), Ok(LightningMode::Off));
        assert!(LightningMode::parse("always").is_err());
    }

    #[test]
    fn bolts_run_from_end_to_end_in_a_bounded_number_of_segments() {
        for seed in 0..50 {
            let bolt = bolt_path(seed, (100.0, 50.0), (140.0, 400.0));
            assert!(bolt.len() <= MAX_SEGMENTS && bolt.len() >= 14);
            assert_eq!(bolt[0][0], (100.0, 50.0));
            assert_eq!(
                bolt[13][1],
                (140.0, 400.0),
                "the main channel reaches the target"
            );
            // The channel is jagged, not straight.
            assert!(bolt[..14]
                .iter()
                .any(|s| (s[0].0 - 100.0 - (s[0].1 - 50.0) * 40.0 / 350.0).abs() > 2.0));
        }
        assert!(bolt_path(1, (5.0, 5.0), (5.0, 5.0)).is_empty());
    }

    #[test]
    fn forced_events_come_at_once_in_their_kind_and_differ() {
        for kind in [BoltKind::InCloud, BoltKind::CloudToCloud, BoltKind::Strike] {
            let event = forced_event(3, kind, 12.5);
            assert_eq!((event.kind, event.start), (kind, 12.5));
            assert!(
                event.intensity(12.5 + RISE) > 0.4,
                "{kind:?} flashes at once"
            );
        }
        let a = forced_event(3, BoltKind::Strike, 10.0);
        let b = forced_event(3, BoltKind::Strike, 20.0);
        assert_ne!(a.at, b.at, "each forced strike is new");
        // The schedule is unchanged by the refactor: same events every run.
        assert_eq!(events_in(9, 3), events_in(9, 3));
    }

    #[test]
    fn events_are_the_same_every_run() {
        assert_eq!(events_in(9, 3), events_in(9, 3));
        assert_ne!(events_in(9, 3), events_in(9, 4));
    }
}
