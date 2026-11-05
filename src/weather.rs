//! Rain, snow, and wind (owner decisions, 2026-11-05: "rain and wind so we
//! can complete the storm. We also want snow"; precipitation only in the
//! foreground, in 4 layers scaled slightly, with per-layer density by storm
//! strength; wind sets the angle it falls at and drives the clouds; no
//! searchlights while anything falls; lightning only in rainstorms).
//!
//! Precipitation is drawn by the GPU every frame (`present.wgsl`) at cell
//! resolution, so it looks like the rest of the pixel art: each layer is a
//! grid of tiles moving with the fall and the wind, and each tile holds at
//! most one seeded drop (a short streak of rain or a flake of snow). This
//! module keeps each layer's travel (wrapped, so it never loses precision)
//! and decides its look.
//!
//! For now the amount follows the cloud cover (light in overcast, heavy in
//! storms) and `--precip` / `W` choose rain, snow, or none. A later sprint
//! lets the weather drift on its own (owner: "Random weather").

use crate::clouds::Cover;

/// Precipitation layers, far to near.
pub const PRECIP_LAYERS: usize = 4;
/// Each layer's size (far to near): farther drops are smaller, slower, and
/// fainter.
pub const LAYER_SCALE: [f64; PRECIP_LAYERS] = [0.55, 0.7, 0.85, 1.0];
/// Tile size in cells at scale 1: at most one drop per tile.
pub const TILE: f64 = 9.0;
/// Share of tiles holding a drop at full strength, far to near: farther
/// layers are denser, so the distance reads as a haze of rain.
pub const DENSITY: [f64; PRECIP_LAYERS] = [0.36, 0.27, 0.19, 0.13];
/// Rain is far sparser than snow: a few comet drops read as rain, many as
/// a speckle (owner, 2026-11-05: "way too dense. It looked more like snow
/// than rain"). Share of tiles with a drop at full strength, far to near.
pub const RAIN_DENSITY: [f64; PRECIP_LAYERS] = [0.12, 0.09, 0.065, 0.045];
/// Snow flake size per layer, far to near, in cells: under 1 is a square
/// that many cells across, drawn in screen pixels (at least one); 1 is one
/// cell; [`SNOW_X`] is a 5-cell X (a cell and its four diagonal
/// neighbours).
pub const SNOW_FLAKE: [f64; PRECIP_LAYERS] = [1.0 / 3.0, 2.0 / 3.0, 1.0, SNOW_X];
/// The flake size code for a 5-cell X.
pub const SNOW_X: f64 = 2.0;
/// Fall speeds in cells per second at scale 1.
pub const RAIN_SPEED: f64 = 220.0;
pub const SNOW_SPEED: f64 = 14.0;
// Checked when building: snow falls at least 75% slower than rain (owner,
// 2026-11-05).
const _: () = assert!(SNOW_SPEED <= 0.25 * RAIN_SPEED);
/// How much the wind pushes each kind sideways, per cell per second of
/// wind: rain falls so fast that the real push would leave it vertical, so
/// it is exaggerated until a storm slants it clearly; snow drifts with
/// the wind as it is.
pub const RAIN_WIND_GAIN: f64 = 10.0;
pub const SNOW_WIND_GAIN: f64 = 1.0;
/// Tiles after which a layer's travel wraps (the shader's tile hash repeats
/// with the same period, so wrapping is seamless).
pub const WRAP_TILES: f64 = 4096.0;

/// What falls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrecipKind {
    Rain,
    Snow,
}

/// What `--precip` and the `W` key choose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrecipMode {
    /// Rain when the clouds call for it (default).
    #[default]
    Rain,
    /// Snow instead.
    Snow,
    /// Nothing falls.
    Off,
}

impl PrecipMode {
    /// Parses `rain`, `snow`, or `off` (any case).
    ///
    /// # Errors
    ///
    /// Returns a message for anything else.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "rain" => Ok(Self::Rain),
            "snow" => Ok(Self::Snow),
            "off" => Ok(Self::Off),
            _ => Err(format!("must be rain, snow, or off, got '{value}'")),
        }
    }

    /// The next mode for the `W` key: rain, snow, off, rain...
    pub fn next(self) -> Self {
        match self {
            Self::Rain => Self::Snow,
            Self::Snow => Self::Off,
            Self::Off => Self::Rain,
        }
    }

    /// The name shown when it changes.
    pub fn label(self) -> &'static str {
        match self {
            Self::Rain => "Rain",
            Self::Snow => "Snow",
            Self::Off => "No precipitation",
        }
    }

    /// What falls under `cover`, if anything.
    pub fn kind_under(self, cover: Cover) -> Option<PrecipKind> {
        if intensity_for(cover) <= 0.0 {
            return None;
        }
        match self {
            Self::Rain => Some(PrecipKind::Rain),
            Self::Snow => Some(PrecipKind::Snow),
            Self::Off => None,
        }
    }
}

/// How hard it falls under `cover`, 0 (nothing) to 1 (a storm).
pub fn intensity_for(cover: Cover) -> f64 {
    match cover {
        Cover::Clear | Cover::Puffs | Cover::Scattered | Cover::Broken => 0.0,
        Cover::Overcast => 0.35,
        Cover::Storm => 1.0,
    }
}

/// The steady wind under `cover`, in cells per second (positive blows to
/// the right): calm in fair weather, strong in storms.
pub fn wind_for(cover: Cover) -> f64 {
    match cover {
        Cover::Clear => 0.5,
        Cover::Puffs => 1.0,
        Cover::Scattered => 1.5,
        Cover::Broken => 2.5,
        Cover::Overcast => 4.0,
        Cover::Storm => 9.0,
    }
}

/// The wind the clouds' drift speeds were tuned for (the default cover's).
pub const CLOUD_WIND_REFERENCE: f64 = 1.5;

/// Which way the wind blows across the screen: -1, right to left. The
/// clouds drift that way (their views move right through the world, so
/// they slide left on screen), and rain and snow must slant with them
/// (owner, 2026-11-05: "the rain direction should match the wind").
pub const WIND_DIRECTION: f64 = -1.0;

/// The wind now under `cover`, in cells per second along the screen
/// (negative blows to the left): its steady strength times the gusts, in
/// [`WIND_DIRECTION`].
pub fn wind_now(cover: Cover, seed: u32, seconds: f64) -> f64 {
    WIND_DIRECTION * wind_for(cover) * gust(seed, seconds)
}

/// Gusts: a smooth factor of 0.55 to 1.45 on the steady wind at `seconds`,
/// seeded, changing over several seconds (two slow waves summed). O(1).
pub fn gust(seed: u32, seconds: f64) -> f64 {
    let phase = f64::from(seed % 1000) * 0.618;
    let slow = (seconds * std::f64::consts::TAU / 13.0 + phase).sin();
    let quick = (seconds * std::f64::consts::TAU / 5.3 + phase * 1.7).sin();
    1.0 + 0.3 * slow + 0.15 * quick
}

/// One precipitation layer's state for the shader.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrecipLayer {
    /// How far its tiles have moved (cells, wrapped): sideways with the
    /// wind and down with the fall.
    pub offset: (f64, f64),
    /// Tile size in cells.
    pub tile: f64,
    /// Share of tiles holding a drop now.
    pub density: f64,
    /// Rain: streak length in cells; snow: wobble in cells.
    pub length: f64,
    /// Rain: streak half-width; snow: flake radius (cells).
    pub size: f64,
    /// Opacity, 0..1.
    pub alpha: f64,
}

/// The falling layers' travel, advanced every frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Precipitation {
    offsets: [(f64, f64); PRECIP_LAYERS],
}

impl Precipitation {
    /// Moves every layer `dt` seconds with the fall of `kind` and `wind`
    /// (cells per second). Nearer layers move faster (parallax). O(layers).
    pub fn advance(&mut self, dt: f64, kind: PrecipKind, wind: f64) {
        if !dt.is_finite() || dt <= 0.0 || !wind.is_finite() {
            return;
        }
        let speed = fall_speed(kind);
        let push = wind * wind_gain(kind);
        for (k, offset) in self.offsets.iter_mut().enumerate() {
            let scale = LAYER_SCALE[k];
            let wrap = TILE * scale * WRAP_TILES;
            offset.0 = (offset.0 + push * scale * dt).rem_euclid(wrap);
            offset.1 = (offset.1 + speed * scale * dt).rem_euclid(wrap);
        }
    }

    /// Each layer's look for `kind` at `intensity` (0..1), far to near.
    pub fn layers(&self, kind: PrecipKind, intensity: f64) -> [PrecipLayer; PRECIP_LAYERS] {
        let intensity = intensity.clamp(0.0, 1.0);
        std::array::from_fn(|k| {
            let scale = LAYER_SCALE[k];
            let (length, size, alpha) = match kind {
                // Comet drops (owner, 2026-11-05): 3-4 cells long and one
                // cell wide, a bright head and a fading tail (see the
                // shader); brighter now they are sparse.
                PrecipKind::Rain => (2.5 + 1.5 * scale, 0.5, 0.35 + 0.45 * scale),
                // Flake sizes (owner, 2026-11-05): the nearest layer's are
                // a 5-cell X, the next one cell, and the far two smaller
                // than a cell (see SNOW_FLAKE and the shader).
                PrecipKind::Snow => (0.8 * scale, SNOW_FLAKE[k], 0.5 + 0.45 * scale),
            };
            PrecipLayer {
                offset: self.offsets[k],
                tile: TILE * scale,
                density: match kind {
                    PrecipKind::Rain => RAIN_DENSITY[k],
                    PrecipKind::Snow => DENSITY[k],
                } * intensity,
                length,
                size,
                alpha,
            }
        })
    }
}

/// How fast `kind` falls at scale 1, in cells per second.
pub fn fall_speed(kind: PrecipKind) -> f64 {
    match kind {
        PrecipKind::Rain => RAIN_SPEED,
        PrecipKind::Snow => SNOW_SPEED,
    }
}

/// How much the wind pushes `kind` sideways (see [`RAIN_WIND_GAIN`]).
pub fn wind_gain(kind: PrecipKind) -> f64 {
    match kind {
        PrecipKind::Rain => RAIN_WIND_GAIN,
        PrecipKind::Snow => SNOW_WIND_GAIN,
    }
}

/// The direction drops move (and rain streaks lie), as a unit vector with
/// y down: the wind's push against the fall. Snow slants more than rain.
pub fn fall_direction(kind: PrecipKind, wind: f64) -> (f64, f64) {
    let wind = if wind.is_finite() { wind } else { 0.0 };
    let (x, y) = (wind * wind_gain(kind), fall_speed(kind));
    let length = (x * x + y * y).sqrt();
    (x / length, y / length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precipitation_follows_the_clouds_and_the_mode() {
        assert_eq!(PrecipMode::Rain.kind_under(Cover::Scattered), None);
        assert_eq!(
            PrecipMode::Rain.kind_under(Cover::Storm),
            Some(PrecipKind::Rain)
        );
        assert_eq!(
            PrecipMode::Snow.kind_under(Cover::Overcast),
            Some(PrecipKind::Snow)
        );
        assert_eq!(PrecipMode::Off.kind_under(Cover::Storm), None);
        assert!(intensity_for(Cover::Storm) > intensity_for(Cover::Overcast));
        assert_eq!(PrecipMode::Rain.next().next().next(), PrecipMode::Rain);
        assert_eq!(PrecipMode::parse("SNOW"), Ok(PrecipMode::Snow));
        assert!(PrecipMode::parse("hail").is_err());
    }

    #[test]
    fn far_layers_are_smaller_denser_and_fainter() {
        let layers = Precipitation::default().layers(PrecipKind::Rain, 1.0);
        for pair in layers.windows(2) {
            let (far, near) = (pair[0], pair[1]);
            // Rain drops stay one cell wide in every layer (comet drops);
            // only their length grows.
            assert!(far.tile < near.tile && far.size == near.size && far.length < near.length);
            assert!(far.density > near.density && far.alpha < near.alpha);
        }
        let snow = Precipitation::default().layers(PrecipKind::Snow, 1.0);
        assert!(
            snow.windows(2).all(|pair| pair[0].alpha < pair[1].alpha),
            "far flakes are dimmer"
        );
        // Light rain has fewer drops in every layer.
        let light = Precipitation::default().layers(PrecipKind::Rain, 0.35);
        assert!(light
            .iter()
            .zip(&layers)
            .all(|(l, h)| l.density < h.density));
        assert!(Precipitation::default()
            .layers(PrecipKind::Snow, 0.0)
            .iter()
            .all(|l| l.density == 0.0));
    }

    #[test]
    fn snowflakes_grow_from_sub_cell_specks_to_a_5_cell_x() {
        let snow = Precipitation::default().layers(PrecipKind::Snow, 1.0);
        assert!(
            snow.windows(2).all(|pair| pair[0].size < pair[1].size),
            "far flakes are smaller"
        );
        assert!(
            snow[0].size < 1.0 && snow[1].size < 1.0,
            "the far two are under a cell"
        );
        assert_eq!(snow[2].size, 1.0);
        assert_eq!(snow[3].size, SNOW_X, "the nearest are a 5-cell X");
    }

    #[test]
    fn rain_is_comet_drops_far_sparser_than_snow() {
        let p = Precipitation::default();
        let (rain, snow) = (
            p.layers(PrecipKind::Rain, 1.0),
            p.layers(PrecipKind::Snow, 1.0),
        );
        for (r, s) in rain.iter().zip(&snow) {
            assert!(
                r.density * 2.5 < s.density,
                "{} vs {}",
                r.density,
                s.density
            );
            assert!((3.0..=4.0).contains(&r.length), "3-4 cells: {}", r.length);
            assert_eq!(r.size, 0.5, "one cell wide");
        }
    }

    #[test]
    fn wind_slants_snow_more_than_rain_and_storms_blow_harder() {
        let rain = fall_direction(PrecipKind::Rain, 9.0);
        let snow = fall_direction(PrecipKind::Snow, 9.0);
        assert!(snow.0 > rain.0, "snow slants more: {snow:?} vs {rain:?}");
        // A storm slants rain clearly (about 20 degrees from vertical).
        assert!(rain.0.atan2(rain.1).to_degrees() > 18.0);
        assert!((rain.0 * rain.0 + rain.1 * rain.1 - 1.0).abs() < 1e-12);
        assert_eq!(fall_direction(PrecipKind::Rain, 0.0), (0.0, 1.0));
        assert!(
            wind_for(Cover::Storm) > wind_for(Cover::Overcast)
                && wind_for(Cover::Overcast) > wind_for(Cover::Clear)
        );
    }

    #[test]
    fn rain_and_snow_slant_the_way_the_clouds_drift() {
        use crate::model::palette::palette_by_key;
        use crate::skyclouds::SkyClouds;
        // On screen the clouds slide the opposite way to their view.
        let mut sky = SkyClouds::new(1, 40, 30, Cover::Storm, palette_by_key("vga").unwrap());
        let before = sky.view(3);
        sky.advance(1.0 / 30.0, 1.0, 0.0, None);
        let cloud_screen_motion = -(sky.view(3) - before);
        assert!(cloud_screen_motion < 0.0, "the clouds drift left");
        let wind = wind_now(Cover::Storm, 3, 10.0);
        assert!(wind < 0.0);
        for kind in [PrecipKind::Rain, PrecipKind::Snow] {
            let (dx, _) = fall_direction(kind, wind);
            assert!(
                dx * cloud_screen_motion > 0.0,
                "{kind:?} slants with the clouds"
            );
            let mut p = Precipitation::default();
            p.advance(0.1, kind, wind);
            // Travel wraps, so compare against the wrap: moving left wraps
            // to just below it.
            let wrap = TILE * LAYER_SCALE[3] * WRAP_TILES;
            assert!(
                p.layers(kind, 1.0)[3].offset.0 > wrap / 2.0,
                "{kind:?} drifts left"
            );
        }
    }

    #[test]
    fn gusts_vary_smoothly_within_bounds() {
        let mut previous = gust(3, 0.0);
        let (mut low, mut high) = (f64::MAX, f64::MIN);
        for i in 1..6000 {
            let g = gust(3, f64::from(i) / 60.0);
            assert!((0.55..=1.45).contains(&g));
            assert!((g - previous).abs() < 0.01, "smooth at 60 fps");
            low = low.min(g);
            high = high.max(g);
            previous = g;
        }
        assert!(high - low > 0.5, "gusts really change: {low}..{high}");
    }

    #[test]
    fn layers_move_down_and_with_the_wind_nearer_faster_and_wrap() {
        let mut p = Precipitation::default();
        p.advance(0.5, PrecipKind::Rain, 4.0);
        let layers = p.layers(PrecipKind::Rain, 1.0);
        assert!(layers[3].offset.1 > layers[0].offset.1 && layers[0].offset.1 > 0.0);
        assert!(layers[3].offset.0 > layers[0].offset.0 && layers[0].offset.0 > 0.0);
        for _ in 0..100_000 {
            p.advance(1.0, PrecipKind::Rain, 9.0);
        }
        for (k, layer) in p.layers(PrecipKind::Rain, 1.0).iter().enumerate() {
            let wrap = TILE * LAYER_SCALE[k] * WRAP_TILES;
            assert!((0.0..wrap).contains(&layer.offset.0) && (0.0..wrap).contains(&layer.offset.1));
        }
        let before = p.clone();
        p.advance(f64::NAN, PrecipKind::Snow, 1.0);
        p.advance(-1.0, PrecipKind::Snow, 1.0);
        assert_eq!(p, before);
    }
}
