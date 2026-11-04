//! The procedural clouds ([`crate::cloudgen`]) in the running app.
//!
//! Generating a whole sky is too slow for every frame (about 0.5 s in a
//! release build, 2.7 s in debug), so clouds are rendered on a **background
//! thread** into a strip of cloud world wider than the screen by
//! [`MARGIN`] cells on each side. The GPU slides that strip by the view's
//! fractional offset as the wind (and, in Endless mode, a tenth of the
//! camera) moves the clouds. A fresh strip is requested when the view nears
//! the end of the margin, when the moonlight moves enough to matter, or when
//! the cover or palette changes; the worker always renders the latest
//! request and drops older ones. The frame rate never waits on it.
//!
//! **Moonlight** (GitHub issue #1): the moon is drawn behind the clouds and
//! each cloud cell lets `exp(-k × density)` of it through; clouds are lit by
//! a self-shadowing march toward the moon's real position, scaled by its
//! phase; edge light near the moon comes only through thin cloud. A strip
//! is relit when the moon has moved [`RELIGHT_CELLS`] cells or changed
//! phase, which is too gradual to see.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::JoinHandle;

use crate::cloudgen::{dithered_alpha, light_at, sky_fields, tone_index, tone_ramp, Light, LAYERS};
use crate::clouds::Cover;
use crate::endless::decode_srgb;
use crate::model::palette::Palette;

/// Wind drift in cells per second at 1x (Rebuild Prompt).
pub const WIND_SPEED: f64 = 0.35;
/// Fraction of the Endless root camera's motion the clouds follow.
pub const CLOUD_PARALLAX: f64 = 0.1;
/// Extra cloud-world columns rendered on each side of the screen.
pub const MARGIN: i32 = 96;
/// How far (cells) the moon moves before the clouds are relit.
pub const RELIGHT_CELLS: f64 = 4.0;
/// Longest simulated motion step per frame, in seconds.
pub const MAX_MOTION_STEP: f64 = 1.0 / 30.0;
/// The most opaque stacked clouds can be, so the moon always shows faintly
/// (owner, 2026-11-03), even where many layers overlap.
pub const MAX_STACKED_OPACITY: f64 = 0.94;

/// Everything needed to render one cloud strip.
#[derive(Debug, Clone, Copy)]
pub struct StripRequest {
    /// Increases with every request; results for older ones are dropped.
    pub generation: u64,
    pub seed: u32,
    pub cover: Cover,
    pub palette: &'static Palette,
    /// Rows of sky (the ground row).
    pub sky_rows: i32,
    /// Cloud-world column of the strip's first column.
    pub origin: i32,
    /// Strip width in columns.
    pub width: usize,
    /// The cloud-world column at the screen's left edge when the request
    /// was made: the moon's position is in screen space.
    pub view: f64,
    pub light: Option<Light>,
}

/// A rendered strip: straight-alpha, sRGB-encoded RGBA, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct StripImage {
    pub generation: u64,
    pub origin: i32,
    pub width: usize,
    pub rows: usize,
    pub rgba: Vec<u8>,
}

fn encode(c: f64) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let v = if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (v * 255.0).round() as u8
}

/// Renders a strip: every layer and sub-layer far to near, each cell lit by
/// [`light_at`], snapped to its layer's tones, hazed by distance, dithered
/// at the edges, and composited in linear light. Sky is transparent.
/// O(cells × sub-layers × march steps).
pub fn render_strip(request: &StripRequest) -> StripImage {
    let rows = request.sky_rows.max(1) as usize;
    let width = request.width.max(1);
    let fields = sky_fields(request.cover, request.sky_rows, request.seed);
    let sky = request
        .palette
        .bg
        .0
        .map(|v| decode_srgb(f64::from(v) / 255.0));
    // Premultiplied linear color and coverage per cell.
    let mut color = vec![[0.0f64; 3]; width * rows];
    let mut cover = vec![0.0f64; width * rows];
    for (field, layer) in fields.iter().zip(LAYERS) {
        if field.weather.coverage <= 0.0 {
            continue;
        }
        let ramp = tone_ramp(layer, request.cover, request.palette)
            .map(|c| c.0.map(|v| decode_srgb(f64::from(v) / 255.0)));
        for sub in (0..field.sublayers()).rev() {
            let haze = field.haze(sub);
            for row in 0..rows {
                let y = row as f64 + 0.5;
                for col in 0..width {
                    let world = request.origin + col as i32;
                    let x = f64::from(world) + 0.5;
                    let density = field.sublayer_density(x, y, sub);
                    if density <= 0.0 {
                        continue;
                    }
                    let alpha = dithered_alpha(field.opacity(density), world, row as i32);
                    if alpha <= 0.0 {
                        continue;
                    }
                    let lit = light_at(field, sub, x, y, x - request.view, request.light);
                    let tone = ramp[tone_index(lit)];
                    let cell = row * width + col;
                    for k in 0..3 {
                        let hazy = tone[k] + (sky[k] - tone[k]) * haze;
                        color[cell][k] = hazy * alpha + color[cell][k] * (1.0 - alpha);
                    }
                    cover[cell] = alpha + cover[cell] * (1.0 - alpha);
                }
            }
        }
    }
    let mut rgba = Vec::with_capacity(width * rows * 4);
    for (premultiplied, &a) in color.iter().zip(&cover) {
        if a <= 0.0 {
            rgba.extend([0, 0, 0, 0]);
            continue;
        }
        rgba.extend(premultiplied.map(|c| encode(c / a)));
        rgba.push((a.min(MAX_STACKED_OPACITY) * 255.0).round() as u8);
    }
    StripImage {
        generation: request.generation,
        origin: request.origin,
        width,
        rows,
        rgba,
    }
}

/// A background thread that renders strips. Requests queue up; the worker
/// always skips to the newest one.
pub struct CloudWorker {
    requests: Option<Sender<StripRequest>>,
    results: Receiver<StripImage>,
    thread: Option<JoinHandle<()>>,
}

impl CloudWorker {
    /// Starts the worker thread.
    pub fn spawn() -> Self {
        let (request_tx, request_rx) = channel::<StripRequest>();
        let (result_tx, result_rx) = channel::<StripImage>();
        let thread = std::thread::Builder::new()
            .name("cloud renderer".into())
            .spawn(move || {
                while let Ok(mut request) = request_rx.recv() {
                    // Skip straight to the newest request.
                    while let Ok(newer) = request_rx.try_recv() {
                        request = newer;
                    }
                    if result_tx.send(render_strip(&request)).is_err() {
                        break;
                    }
                }
            })
            .expect("the cloud thread starts");
        Self {
            requests: Some(request_tx),
            results: result_rx,
            thread: Some(thread),
        }
    }

    /// Queues a request.
    pub fn request(&self, request: StripRequest) {
        if let Some(tx) = &self.requests {
            // A closed worker just means no more clouds; ignore it.
            let _ = tx.send(request);
        }
    }

    /// The newest finished strip, if any arrived since the last call.
    pub fn latest(&self) -> Option<StripImage> {
        let mut newest = None;
        while let Ok(image) = self.results.try_recv() {
            newest = Some(image);
        }
        newest
    }
}

impl Drop for CloudWorker {
    fn drop(&mut self) {
        // Closing the channel ends the worker's loop once it finishes the
        // strip in hand. It is not joined: a resize or restart must not
        // wait seconds for a strip nobody will see.
        self.requests.take();
        self.thread.take();
    }
}

/// The app's clouds: where the view is, what to show, and the strip.
pub struct SkyClouds {
    seed: u32,
    cover: Cover,
    palette: &'static Palette,
    cols: i32,
    sky_rows: i32,
    /// Wind travel so far, in cells.
    wind: f64,
    /// Cloud-world column at the screen's left edge.
    view: f64,
    worker: CloudWorker,
    generation: u64,
    /// The generation the current strip must at least have to be shown.
    wanted: u64,
    /// A request is outstanding.
    pending: bool,
    /// Something changed (cover, palette, light): request a fresh strip.
    stale: bool,
    /// The light the newest request used.
    requested_light: Option<Light>,
    /// The strip on screen.
    current: Option<StripImage>,
}

impl SkyClouds {
    /// Clouds for a sky `cols` wide and `sky_rows` tall.
    pub fn new(
        seed: u32,
        cols: i32,
        sky_rows: i32,
        cover: Cover,
        palette: &'static Palette,
    ) -> Self {
        Self {
            seed,
            cover,
            palette,
            cols: cols.max(1),
            sky_rows: sky_rows.max(1),
            wind: 0.0,
            view: 0.0,
            worker: CloudWorker::spawn(),
            generation: 0,
            wanted: 0,
            pending: false,
            stale: true,
            requested_light: None,
            current: None,
        }
    }

    /// The coverage preset.
    pub fn cover(&self) -> Cover {
        self.cover
    }

    /// Switches the coverage preset (the old strip stays until the new one
    /// is ready).
    pub fn set_cover(&mut self, cover: Cover) {
        self.cover = cover;
        self.invalidate();
    }

    /// Switches the palette.
    pub fn set_palette(&mut self, palette: &'static Palette) {
        self.palette = palette;
        self.invalidate();
    }

    /// Asks for a fresh strip, and stops showing older generations once it
    /// arrives.
    fn invalidate(&mut self) {
        self.stale = true;
        self.wanted = self.generation + 1;
    }

    /// Cloud-world column at the screen's left edge.
    pub fn view(&self) -> f64 {
        self.view
    }

    /// The strip on screen, if any.
    pub fn image(&self) -> Option<&StripImage> {
        self.current.as_ref()
    }

    /// Device pixels from the strip's left edge to the screen's left edge.
    pub fn strip_offset_px(&self, cell: u32) -> f64 {
        self.current.as_ref().map_or(0.0, |image| {
            (self.view - f64::from(image.origin)) * f64::from(cell)
        })
    }

    /// True when `light` differs enough from the one last rendered.
    fn light_moved(&self, light: Option<Light>) -> bool {
        match (self.requested_light, light) {
            (None, None) => false,
            (Some(a), Some(b)) => {
                let moved = ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
                moved >= RELIGHT_CELLS || (a.strength - b.strength).abs() > 1e-9
            }
            _ => true,
        }
    }

    /// Moves the clouds one frame (wind for at most [`MAX_MOTION_STEP`]
    /// seconds, plus [`CLOUD_PARALLAX`] of the Endless `camera`), asks for a
    /// fresh strip when needed, and takes a finished one. Returns true when
    /// a new strip arrived (it needs uploading).
    pub fn advance(&mut self, dt: f64, speed: f64, camera: f64, light: Option<Light>) -> bool {
        let step = if dt.is_finite() {
            dt.clamp(0.0, MAX_MOTION_STEP)
        } else {
            0.0
        };
        self.wind += WIND_SPEED * speed.max(0.0) * step;
        if camera.is_finite() {
            self.view = self.wind + CLOUD_PARALLAX * camera;
        }
        let mut arrived = false;
        if let Some(image) = self.worker.latest() {
            self.pending = false;
            if image.generation >= self.wanted {
                self.current = Some(image);
                arrived = true;
            }
        }
        let running_out = self.current.as_ref().is_none_or(|image| {
            let into = self.view - f64::from(image.origin);
            into < 0.0 || into > f64::from(MARGIN) * 1.5
        });
        if !self.pending && (self.stale || running_out || self.light_moved(light)) {
            self.generation += 1;
            self.worker.request(StripRequest {
                generation: self.generation,
                seed: self.seed,
                cover: self.cover,
                palette: self.palette,
                sky_rows: self.sky_rows,
                origin: self.view.floor() as i32 - MARGIN,
                width: (self.cols + 2 * MARGIN) as usize,
                view: self.view,
                light,
            });
            self.pending = true;
            self.stale = false;
            self.requested_light = light;
        }
        arrived
    }

    /// Blocks until the newest requested strip arrives (tests and startup
    /// previews). Returns false if the worker stopped.
    pub fn wait(&mut self) -> bool {
        while self.pending {
            match self.worker.results.recv() {
                Ok(image) => {
                    if image.generation == self.generation {
                        self.pending = false;
                    }
                    if image.generation >= self.wanted {
                        self.current = Some(image);
                    }
                }
                Err(_) => return false,
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::palette::{palette_by_key, palettes};

    fn request(cover: Cover, origin: i32, width: usize) -> StripRequest {
        StripRequest {
            generation: 1,
            seed: 7,
            cover,
            palette: palette_by_key("vga").unwrap(),
            sky_rows: 60,
            origin,
            width,
            view: f64::from(origin),
            light: Some(Light {
                x: 30.0,
                y: 10.0,
                radius: 19.5,
                strength: 1.0,
            }),
        }
    }

    #[test]
    fn a_clear_sky_strip_is_fully_transparent() {
        let image = render_strip(&request(Cover::Clear, 0, 80));
        assert_eq!(image.rgba.len(), 80 * 60 * 4);
        assert!(image.rgba.chunks(4).all(|p| p[3] == 0));
    }

    #[test]
    fn strips_carry_translucent_cloud_and_open_sky() {
        let image = render_strip(&request(Cover::Overcast, 0, 120));
        let alphas: Vec<u8> = image.rgba.chunks(4).map(|p| p[3]).collect();
        assert!(alphas.iter().any(|&a| a > 0), "some cloud");
        assert!(alphas.contains(&0), "some open sky");
        let most = *alphas.iter().max().unwrap();
        assert!(
            f64::from(most) <= MAX_STACKED_OPACITY * 255.0 + 0.5,
            "never fully opaque: {most}"
        );
    }

    #[test]
    fn neighbouring_strips_agree_where_they_overlap() {
        // The field is world-anchored: a strip starting 10 columns later
        // shows the same clouds, shifted (same view, so same lighting).
        let mut a = request(Cover::Broken, 0, 80);
        let mut b = request(Cover::Broken, 10, 80);
        a.view = 0.0;
        b.view = 0.0;
        let (a, b) = (render_strip(&a), render_strip(&b));
        for row in 0..60 {
            for col in 10..80 {
                let pa = &a.rgba[(row * 80 + col) * 4..(row * 80 + col) * 4 + 4];
                let pb = &b.rgba[(row * 80 + col - 10) * 4..(row * 80 + col - 10) * 4 + 4];
                assert_eq!(pa, pb, "({col}, {row})");
            }
        }
    }

    #[test]
    fn every_palette_renders() {
        for palette in palettes() {
            let image = render_strip(&StripRequest {
                palette,
                ..request(Cover::Broken, 0, 40)
            });
            assert_eq!(image.width, 40);
        }
    }

    #[test]
    fn the_worker_renders_the_newest_request() {
        let worker = CloudWorker::spawn();
        for generation in 1..=3 {
            worker.request(StripRequest {
                generation,
                ..request(Cover::Puffs, 0, 30)
            });
        }
        // Collect until generation 3 arrives; older ones may be skipped.
        let mut seen = Vec::new();
        while seen.last() != Some(&3) {
            let image = worker.results.recv().unwrap();
            seen.push(image.generation);
        }
        assert!(seen.windows(2).all(|w| w[0] < w[1]), "{seen:?}");
    }

    #[test]
    fn sky_clouds_drift_request_and_swap_strips() {
        let vga = palette_by_key("vga").unwrap();
        let mut sky = SkyClouds::new(3, 40, 30, Cover::Broken, vga);
        assert!(sky.image().is_none());
        sky.advance(1.0 / 30.0, 1.0, 0.0, None);
        assert!(sky.wait());
        let first = sky.image().unwrap().clone();
        assert_eq!(first.width, (40 + 2 * MARGIN) as usize);
        assert_eq!(first.origin, -MARGIN);
        // 10 s of wind at 1x: 3.5 cells.
        for _ in 0..300 {
            sky.advance(1.0 / 30.0, 1.0, 0.0, None);
        }
        assert!((sky.view() - (3.5 + 1.0 / 30.0 * 0.35)).abs() < 1e-9);
        let offset = sky.strip_offset_px(3);
        assert!(offset > f64::from(MARGIN) * 3.0, "{offset}");
        // Endless: a tenth of the camera.
        sky.advance(0.0, 1.0, 100.0, None);
        assert!((sky.view() - (sky.wind + 10.0)).abs() < 1e-9);
        // Changing cover asks for a new strip and swaps it in.
        sky.set_cover(Cover::Clear);
        sky.advance(0.0, 1.0, 100.0, None);
        assert!(sky.wait());
        assert!(sky.image().unwrap().rgba.chunks(4).all(|p| p[3] == 0));
    }

    #[test]
    fn moonlight_moving_a_few_cells_triggers_a_relight() {
        let vga = palette_by_key("vga").unwrap();
        let mut sky = SkyClouds::new(3, 40, 30, Cover::Broken, vga);
        let moon = |x: f64| Light {
            x,
            y: 10.0,
            radius: 19.5,
            strength: 1.0,
        };
        sky.advance(0.0, 1.0, 0.0, Some(moon(20.0)));
        assert!(sky.wait());
        let generation = sky.generation;
        sky.advance(0.0, 1.0, 0.0, Some(moon(21.0)));
        assert_eq!(sky.generation, generation, "a small move waits");
        sky.advance(0.0, 1.0, 0.0, Some(moon(20.0 + RELIGHT_CELLS)));
        assert_eq!(sky.generation, generation + 1, "a bigger move relights");
        assert!(sky.wait());
    }
}
