//! The procedural clouds ([`crate::cloudgen`]) in the running app, in four
//! layers by distance (owner decision, 2026-11-04: "give clouds their own 4
//! layers so we can have a bit more 'energy' in the cloud mix").
//!
//! Each layer drifts with its own wind and follows its own share of the
//! Endless camera, nearer layers faster, so the layers slide past one
//! another. Each has its own strip of cloud world, wider than the screen
//! by [`MARGIN`] cells on each side, rendered on a **background thread**;
//! the GPU slides every strip by its layer's fractional offset. A layer
//! asks for a fresh strip when its view nears the end of the margin, when
//! the moonlight moves enough to matter, or when the cover or palette
//! changes. The worker renders the newest request for each layer and drops
//! older ones, so the frame rate never waits on it.
//!
//! **Moonlight** (GitHub issue #1): the moon is drawn behind the clouds and
//! each cloud cell lets `exp(-k × density)` of it through; clouds are lit by
//! a self-shadowing march toward the moon's real position, scaled by its
//! phase; edge light near the moon comes only through thin cloud. A layer
//! is relit when the moon has moved [`RELIGHT_CELLS`] cells or changed
//! phase.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::JoinHandle;

use crate::cloudgen::{
    dithered_alpha, layer_parts, light_at, sky_fields, tone_index, tone_ramp, Light, CLOUD_LAYERS,
    LAYERS,
};
use crate::clouds::Cover;
use crate::endless::decode_srgb;
use crate::model::palette::Palette;

/// Wind drift per layer (far to near) in cells per second at 1x. The
/// mid-near layer keeps the spec's 0.35.
pub const LAYER_WIND: [f64; CLOUD_LAYERS] = [0.15, 0.25, 0.35, 0.5];
/// Share of the Endless root camera's motion each layer follows.
pub const LAYER_PARALLAX: [f64; CLOUD_LAYERS] = [0.03, 0.06, 0.1, 0.16];
/// Extra cloud-world columns rendered on each side of the screen.
pub const MARGIN: i32 = 96;
/// How far (cells) the moon moves before the clouds are relit.
pub const RELIGHT_CELLS: f64 = 4.0;
/// Longest simulated motion step per frame, in seconds.
pub const MAX_MOTION_STEP: f64 = 1.0 / 30.0;
/// The most opaque stacked clouds can be within a layer, so the moon always
/// shows faintly (owner, 2026-11-03).
pub const MAX_STACKED_OPACITY: f64 = 0.94;

/// Everything needed to render one layer's strip.
#[derive(Debug, Clone, Copy)]
pub struct StripRequest {
    /// Increases with every request; older results are dropped.
    pub generation: u64,
    /// The distance layer, 0 (far) to 3 (near).
    pub band: usize,
    pub seed: u32,
    pub cover: Cover,
    pub palette: &'static Palette,
    /// Rows of sky (the ground row).
    pub sky_rows: i32,
    /// Cloud-world column of the strip's first column.
    pub origin: i32,
    /// Strip width in columns.
    pub width: usize,
    /// The layer's cloud-world column at the screen's left edge when the
    /// request was made: the moon's position is in screen space.
    pub view: f64,
    pub light: Option<Light>,
}

/// A rendered strip: straight-alpha, sRGB-encoded RGBA, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct StripImage {
    pub generation: u64,
    pub band: usize,
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

/// Renders one layer's strip: its parts far to near, each cell lit by
/// [`light_at`], snapped to its sky layer's tones, hazed by distance,
/// dithered at the edges, and composited in linear light. Sky is
/// transparent. O(cells × parts × march steps).
pub fn render_strip(request: &StripRequest) -> StripImage {
    let rows = request.sky_rows.max(1) as usize;
    let width = request.width.max(1);
    let fields = sky_fields(request.cover, request.sky_rows, request.seed);
    let sky = request
        .palette
        .bg
        .0
        .map(|v| decode_srgb(f64::from(v) / 255.0));
    let ramps = LAYERS.map(|layer| {
        tone_ramp(layer, request.cover, request.palette)
            .map(|c| c.0.map(|v| decode_srgb(f64::from(v) / 255.0)))
    });
    // Premultiplied linear color and coverage per cell.
    let mut color = vec![[0.0f64; 3]; width * rows];
    let mut cover = vec![0.0f64; width * rows];
    for (index, sub) in layer_parts(request.band) {
        let field = &fields[index];
        if field.weather.coverage <= 0.0 {
            continue;
        }
        let haze = field.haze(sub);
        let ramp = &ramps[index];
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
        band: request.band,
        origin: request.origin,
        width,
        rows,
        rgba,
    }
}

/// A background thread that renders strips. For each layer the worker
/// always skips to the newest request.
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
                while let Ok(first) = request_rx.recv() {
                    // Keep only the newest request for each layer.
                    let mut newest: [Option<StripRequest>; CLOUD_LAYERS] = [None; CLOUD_LAYERS];
                    let mut keep = |request: StripRequest| {
                        let slot = &mut newest[request.band.min(CLOUD_LAYERS - 1)];
                        if slot.is_none_or(|old| old.generation < request.generation) {
                            *slot = Some(request);
                        }
                    };
                    keep(first);
                    while let Ok(more) = request_rx.try_recv() {
                        keep(more);
                    }
                    // Nearest first: it moves fastest.
                    for request in newest.iter().rev().flatten() {
                        if result_tx.send(render_strip(request)).is_err() {
                            return;
                        }
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

    /// Every strip finished since the last call.
    pub fn finished(&self) -> Vec<StripImage> {
        self.results.try_iter().collect()
    }
}

impl Drop for CloudWorker {
    fn drop(&mut self) {
        // Closing the channel ends the worker's loop once it finishes the
        // strip in hand. It is not joined: a resize or restart must not
        // wait for a strip nobody will see.
        self.requests.take();
        self.thread.take();
    }
}

/// One layer's motion and strips.
#[derive(Debug, Clone, Default)]
struct LayerState {
    /// Wind travel so far, in cells.
    wind: f64,
    /// Cloud-world column at the screen's left edge.
    view: f64,
    /// The newest generation requested for this layer.
    generation: u64,
    /// The generation a strip must at least have to be shown.
    wanted: u64,
    /// A request is outstanding.
    pending: bool,
    /// Something changed (cover, palette): request a fresh strip.
    stale: bool,
    /// The light the newest request used.
    requested_light: Option<Light>,
    /// The strip on screen.
    current: Option<StripImage>,
}

/// The app's clouds: four layers, their motion, and their strips.
pub struct SkyClouds {
    seed: u32,
    cover: Cover,
    palette: &'static Palette,
    cols: i32,
    sky_rows: i32,
    worker: CloudWorker,
    /// Requests are numbered across all layers.
    next_generation: u64,
    layers: [LayerState; CLOUD_LAYERS],
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
            worker: CloudWorker::spawn(),
            next_generation: 0,
            layers: std::array::from_fn(|_| LayerState {
                stale: true,
                ..LayerState::default()
            }),
        }
    }

    /// The coverage preset.
    pub fn cover(&self) -> Cover {
        self.cover
    }

    /// Switches the coverage preset (the old strips stay until the new ones
    /// are ready).
    pub fn set_cover(&mut self, cover: Cover) {
        self.cover = cover;
        self.invalidate();
    }

    /// Switches the palette.
    pub fn set_palette(&mut self, palette: &'static Palette) {
        self.palette = palette;
        self.invalidate();
    }

    /// Asks every layer for a fresh strip, and stops showing older ones
    /// once each arrives.
    fn invalidate(&mut self) {
        for layer in &mut self.layers {
            layer.stale = true;
            layer.wanted = self.next_generation + 1;
        }
    }

    /// Layer `band`'s cloud-world column at the screen's left edge.
    pub fn view(&self, band: usize) -> f64 {
        self.layers[band].view
    }

    /// Layer `band`'s strip on screen, if any.
    pub fn image(&self, band: usize) -> Option<&StripImage> {
        self.layers[band].current.as_ref()
    }

    /// Device pixels from layer `band`'s strip's left edge to the screen's
    /// left edge.
    pub fn strip_offset_px(&self, band: usize, cell: u32) -> f64 {
        let layer = &self.layers[band];
        layer.current.as_ref().map_or(0.0, |image| {
            (layer.view - f64::from(image.origin)) * f64::from(cell)
        })
    }

    /// The width every strip has (so they can share one texture).
    pub fn strip_width(&self) -> usize {
        (self.cols + 2 * MARGIN) as usize
    }

    /// Rows of every strip.
    pub fn strip_rows(&self) -> usize {
        self.sky_rows as usize
    }

    fn light_moved(old: Option<Light>, new: Option<Light>) -> bool {
        match (old, new) {
            (None, None) => false,
            (Some(a), Some(b)) => {
                let moved = ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
                moved >= RELIGHT_CELLS || (a.strength - b.strength).abs() > 1e-9
            }
            _ => true,
        }
    }

    /// Moves every layer one frame (its wind for at most
    /// [`MAX_MOTION_STEP`] seconds, plus its share of the Endless `camera`),
    /// asks for fresh strips where needed, and takes finished ones. Returns
    /// which layers got a new strip (they need uploading).
    pub fn advance(
        &mut self,
        dt: f64,
        speed: f64,
        camera: f64,
        light: Option<Light>,
    ) -> [bool; CLOUD_LAYERS] {
        let step = if dt.is_finite() {
            dt.clamp(0.0, MAX_MOTION_STEP)
        } else {
            0.0
        };
        let mut arrived = [false; CLOUD_LAYERS];
        for image in self.worker.finished() {
            let band = image.band.min(CLOUD_LAYERS - 1);
            let layer = &mut self.layers[band];
            if image.generation == layer.generation {
                layer.pending = false;
            }
            let newer = layer
                .current
                .as_ref()
                .is_none_or(|c| c.generation < image.generation);
            if image.generation >= layer.wanted && newer {
                layer.current = Some(image);
                arrived[band] = true;
            }
        }
        for band in 0..CLOUD_LAYERS {
            let layer = &mut self.layers[band];
            layer.wind += LAYER_WIND[band] * speed.max(0.0) * step;
            if camera.is_finite() {
                layer.view = layer.wind + LAYER_PARALLAX[band] * camera;
            }
            let running_out = layer.current.as_ref().is_none_or(|image| {
                let into = layer.view - f64::from(image.origin);
                into < 0.0 || into > f64::from(MARGIN) * 1.5
            });
            let relight = Self::light_moved(layer.requested_light, light);
            if !layer.pending && (layer.stale || running_out || relight) {
                self.next_generation += 1;
                layer.generation = self.next_generation;
                self.worker.request(StripRequest {
                    generation: layer.generation,
                    band,
                    seed: self.seed,
                    cover: self.cover,
                    palette: self.palette,
                    sky_rows: self.sky_rows,
                    origin: layer.view.floor() as i32 - MARGIN,
                    width: (self.cols + 2 * MARGIN) as usize,
                    view: layer.view,
                    light,
                });
                layer.pending = true;
                layer.stale = false;
                layer.requested_light = light;
            }
        }
        arrived
    }

    /// Blocks until every layer's newest requested strip arrives (tests).
    /// Returns false if the worker stopped.
    pub fn wait(&mut self) -> bool {
        while self.layers.iter().any(|layer| layer.pending) {
            let Ok(image) = self.worker.results.recv() else {
                return false;
            };
            let layer = &mut self.layers[image.band.min(CLOUD_LAYERS - 1)];
            if image.generation == layer.generation {
                layer.pending = false;
            }
            if image.generation >= layer.wanted {
                layer.current = Some(image);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::palette::{palette_by_key, palettes};

    fn request(cover: Cover, band: usize, origin: i32, width: usize) -> StripRequest {
        StripRequest {
            generation: 1,
            band,
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
        for band in 0..CLOUD_LAYERS {
            let image = render_strip(&request(Cover::Clear, band, 0, 80));
            assert_eq!(image.rgba.len(), 80 * 60 * 4);
            assert!(image.rgba.chunks(4).all(|p| p[3] == 0));
        }
    }

    #[test]
    fn every_layer_carries_translucent_cloud_in_an_overcast_sky() {
        for band in 0..CLOUD_LAYERS {
            let image = render_strip(&request(Cover::Overcast, band, 0, 160));
            let alphas: Vec<u8> = image.rgba.chunks(4).map(|p| p[3]).collect();
            assert!(alphas.iter().any(|&a| a > 0), "layer {band} has cloud");
            assert!(alphas.contains(&0), "layer {band} has open sky");
            let most = f64::from(*alphas.iter().max().unwrap());
            assert!(most <= MAX_STACKED_OPACITY * 255.0 + 0.5, "{band}: {most}");
        }
    }

    #[test]
    fn neighbouring_strips_agree_where_they_overlap() {
        // The field is world-anchored: a strip starting 10 columns later
        // shows the same clouds, shifted (same view, so same lighting).
        for band in [0, 3] {
            let mut a = request(Cover::Broken, band, 0, 80);
            let mut b = request(Cover::Broken, band, 10, 80);
            a.view = 0.0;
            b.view = 0.0;
            let (a, b) = (render_strip(&a), render_strip(&b));
            for row in 0..60 {
                for col in 10..80 {
                    let pa = &a.rgba[(row * 80 + col) * 4..(row * 80 + col) * 4 + 4];
                    let pb = &b.rgba[(row * 80 + col - 10) * 4..(row * 80 + col - 10) * 4 + 4];
                    assert_eq!(pa, pb, "layer {band} ({col}, {row})");
                }
            }
        }
    }

    #[test]
    fn every_palette_renders() {
        for palette in palettes() {
            let image = render_strip(&StripRequest {
                palette,
                ..request(Cover::Broken, 2, 0, 40)
            });
            assert_eq!(image.width, 40);
        }
    }

    #[test]
    fn the_worker_renders_the_newest_request_per_layer() {
        let worker = CloudWorker::spawn();
        for generation in 1..=3 {
            worker.request(StripRequest {
                generation,
                ..request(Cover::Puffs, 1, 0, 30)
            });
        }
        let mut seen = Vec::new();
        while seen.last() != Some(&3) {
            let image = worker.results.recv().unwrap();
            assert_eq!(image.band, 1);
            seen.push(image.generation);
        }
        assert!(seen.windows(2).all(|w| w[0] < w[1]), "{seen:?}");
    }

    #[test]
    fn layers_drift_at_their_own_speeds_and_nearer_is_faster() {
        let vga = palette_by_key("vga").unwrap();
        let mut sky = SkyClouds::new(3, 40, 30, Cover::Broken, vga);
        sky.advance(0.0, 1.0, 0.0, None);
        assert!(sky.wait());
        for band in 0..CLOUD_LAYERS {
            let image = sky.image(band).unwrap();
            assert_eq!(image.width, sky.strip_width());
            assert_eq!(image.origin, -MARGIN);
        }
        // 10 s of wind at 1x.
        for _ in 0..300 {
            sky.advance(1.0 / 30.0, 1.0, 0.0, None);
        }
        for (band, wind) in LAYER_WIND.iter().enumerate() {
            assert!((sky.view(band) - 10.0 * wind).abs() < 1e-9, "{band}");
        }
        // The Endless camera: each layer follows its own share.
        sky.advance(0.0, 1.0, 100.0, None);
        for band in 0..CLOUD_LAYERS {
            let expected = 10.0 * LAYER_WIND[band] + 100.0 * LAYER_PARALLAX[band];
            assert!((sky.view(band) - expected).abs() < 1e-9, "{band}");
        }
        assert!(LAYER_WIND.windows(2).all(|w| w[0] < w[1]));
        assert!(LAYER_PARALLAX.windows(2).all(|w| w[0] < w[1]));
        // A cover change refreshes every layer.
        sky.set_cover(Cover::Clear);
        sky.advance(0.0, 1.0, 100.0, None);
        assert!(sky.wait());
        for band in 0..CLOUD_LAYERS {
            assert!(sky.image(band).unwrap().rgba.chunks(4).all(|p| p[3] == 0));
        }
    }

    #[test]
    fn moonlight_moving_a_few_cells_relights_every_layer() {
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
        let before = sky.next_generation;
        sky.advance(0.0, 1.0, 0.0, Some(moon(21.0)));
        assert_eq!(sky.next_generation, before, "a small move waits");
        sky.advance(0.0, 1.0, 0.0, Some(moon(20.0 + RELIGHT_CELLS)));
        assert_eq!(
            sky.next_generation,
            before + CLOUD_LAYERS as u64,
            "a bigger move relights all"
        );
        assert!(sky.wait());
    }
}
