//! The procedural clouds ([`crate::cloudgen`]) in the running app: four
//! layers by distance, each changing over time through cross-faded
//! keyframes (owner decisions, 2026-11-04: "give clouds their own 4 layers
//! so we can have a bit more 'energy'", and clouds that "fade in and out of
//! existence" and "boil" in storms).
//!
//! **Layers.** Each drifts with its own wind and follows its own share of
//! the Endless camera, nearer layers faster, so they slide past one
//! another.
//!
//! **Keyframes.** Clouds live: heaps grow and dissolve and billows drift
//! and rise ([`crate::cloudgen::LayerField::at_time`]). Rendering every
//! frame is far too slow, so **background threads** (one per layer)
//! render each layer at moments a keyframe interval apart (about 8 s calm,
//! 2.5 s in a storm, times the layer's [`LAYER_PACE`]), always one keyframe
//! ahead. The GPU cross-fades from the previous keyframe to the next over
//! the whole cloud time between them: a heap that forms in between fades
//! in, edges morph, and every keyframe is lit with the moon where it is
//! now. A keyframe never snaps in: if rendering falls behind, the interval
//! stretches to fit the render time, so the clouds evolve a little slower
//! instead of jumping (owner, 2026-11-04: "Clouds just suddenly vanish";
//! measured, storm keyframes due every 2.5-4 s arrived 8-10 s apart and
//! snapped in over 1.5 s). A cover or palette change fades over
//! [`QUICK_FADE`] seconds. Each strip is wider
//! than the screen by [`MARGIN`] cells on each side; the GPU slides it by
//! its layer's fractional offset. The frame rate never waits on the worker.
//!
//! **Moonlight** (GitHub issue #1): the moon is drawn behind the clouds and
//! each cloud cell lets `exp(-k × density)` of it through; clouds are lit by
//! a self-shadowing march toward the moon's position, scaled by its phase;
//! edge light near the moon comes only through thin cloud.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::JoinHandle;

use crate::cloudgen::{
    dithered_alpha, layer_parts, light_at, sky_fields, tone_index, tone_ramp, weather, Light,
    CLOUD_LAYERS, LAYERS,
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
/// Longest simulated motion step per frame, in seconds.
pub const MAX_MOTION_STEP: f64 = 1.0 / 30.0;
/// The most opaque stacked clouds can be within a layer, so the moon always
/// shows faintly (owner, 2026-11-03).
pub const MAX_STACKED_OPACITY: f64 = 0.94;
/// Seconds a cover or palette change takes to fade in.
pub const QUICK_FADE: f64 = 1.5;
/// Strips kept on the GPU per layer: the one fading out and the one in.
pub const SLOTS: usize = 2;

/// How many times the cloud time a render takes the keyframe interval is
/// at least, so the next keyframe is ready before the current fade ends.
pub const RENDER_HEADROOM: f64 = 1.5;

/// The keyframe interval actually used: [`keyframe_interval`], stretched
/// to [`RENDER_HEADROOM`] times the cloud time a render takes
/// (`render_seconds` of real time at `speed`).
pub fn effective_interval(cover: Cover, band: usize, render_seconds: f64, speed: f64) -> f64 {
    let render = render_seconds.max(0.0) * speed.max(0.0) * RENDER_HEADROOM;
    keyframe_interval(cover, band).max(if render.is_finite() { render } else { 0.0 })
}

/// Each layer's keyframe pace (far to near), as a share of the base
/// interval. The shares differ, so the layers' cross-fades drift apart
/// instead of the whole sky pulsing on one beat (owner, 2026-11-04: "they
/// are too synchronized").
pub const LAYER_PACE: [f64; 4] = [1.5, 1.17, 1.0, 0.87];

/// Seconds of cloud time between keyframes for layer `band` under `cover`:
/// 8 s calm down to 2.5 s in a boiling storm, times the layer's
/// [`LAYER_PACE`].
pub fn keyframe_interval(cover: Cover, band: usize) -> f64 {
    let churn = weather(cover)[1].churn;
    (8.0 - 5.5 * churn) * LAYER_PACE[band.min(LAYER_PACE.len() - 1)]
}

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
    /// The cloud time this keyframe shows.
    pub time: f64,
}

/// A rendered strip: straight-alpha, sRGB-encoded RGBA, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct StripImage {
    pub generation: u64,
    pub band: usize,
    pub origin: i32,
    pub width: usize,
    pub rows: usize,
    /// The cloud time it shows.
    pub time: f64,
    /// Real seconds the render took (set by the worker).
    pub render_seconds: f64,
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

/// Renders one layer's strip at its cloud time: its parts far to near, each
/// cell lit by [`light_at`], snapped to its sky layer's tones, hazed by
/// distance, dithered at the edges, and composited in linear light. Sky is
/// transparent. O(cells × parts × march steps).
pub fn render_strip(request: &StripRequest) -> StripImage {
    let rows = request.sky_rows.max(1) as usize;
    let width = request.width.max(1);
    let fields = sky_fields(request.cover, request.sky_rows, request.seed)
        .map(|field| field.at_time(request.time));
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
        time: request.time,
        render_seconds: 0.0,
        rgba,
    }
}

/// Background threads that render strips, one per layer, so a slow layer
/// never holds up the others. Each skips to its layer's newest request.
pub struct CloudWorker {
    requests: Vec<Sender<StripRequest>>,
    results: Receiver<StripImage>,
    threads: Vec<JoinHandle<()>>,
}

impl CloudWorker {
    /// Starts the threads.
    pub fn spawn() -> Self {
        let (result_tx, result_rx) = channel::<StripImage>();
        let mut requests = Vec::new();
        let mut threads = Vec::new();
        for band in 0..CLOUD_LAYERS {
            let (request_tx, request_rx) = channel::<StripRequest>();
            let result_tx = result_tx.clone();
            let thread = std::thread::Builder::new()
                .name(format!("cloud layer {band}"))
                .spawn(move || {
                    while let Ok(mut request) = request_rx.recv() {
                        // Keep only the newest request.
                        while let Ok(more) = request_rx.try_recv() {
                            if more.generation > request.generation {
                                request = more;
                            }
                        }
                        let start = std::time::Instant::now();
                        let mut image = render_strip(&request);
                        image.render_seconds = start.elapsed().as_secs_f64();
                        if result_tx.send(image).is_err() {
                            return;
                        }
                    }
                })
                .expect("a cloud thread starts");
            requests.push(request_tx);
            threads.push(thread);
        }
        Self {
            requests,
            results: result_rx,
            threads,
        }
    }

    /// Queues a request with its layer's thread.
    pub fn request(&self, request: StripRequest) {
        if let Some(tx) = self.requests.get(request.band.min(CLOUD_LAYERS - 1)) {
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
        // Closing the channels ends each thread's loop once it finishes the
        // strip in hand. They are not joined: a resize or restart must not
        // wait for a strip nobody will see.
        self.requests.clear();
        self.threads.clear();
    }
}

/// One layer's motion, keyframes, and fade.
#[derive(Debug, Clone, Default)]
struct LayerState {
    /// Wind travel so far, in cells.
    wind: f64,
    /// Cloud-world column at the screen's left edge.
    view: f64,
    /// The two GPU slots: the keyframe fading in and the one fading out.
    slots: [Option<StripImage>; SLOTS],
    /// Which slot holds the keyframe fading in.
    front: usize,
    /// A finished keyframe waiting for the current fade to end.
    queued: Option<StripImage>,
    /// The cloud time the current fade started, and how long it lasts.
    fade_start: f64,
    fade_length: f64,
    /// The newest generation requested for this layer.
    generation: u64,
    /// The generation a strip must at least have to be used.
    wanted: u64,
    /// A request is outstanding.
    pending: bool,
    /// Something changed (cover, palette): fetch a keyframe for now.
    urgent: bool,
    /// Real seconds this layer's last render took.
    render_seconds: f64,
}

impl LayerState {
    /// How far the current fade has run, 0 to 1.
    fn fade(&self, clock: f64) -> f64 {
        if self.fade_length <= 0.0 {
            return 1.0;
        }
        ((clock - self.fade_start) / self.fade_length).clamp(0.0, 1.0)
    }

    /// The keyframe fading in.
    fn front_image(&self) -> Option<&StripImage> {
        self.slots[self.front].as_ref()
    }

    /// Starts fading `image` in over `length` seconds from `clock`. Returns
    /// the slot it went into.
    fn rotate(&mut self, image: StripImage, clock: f64, length: f64) -> usize {
        let slot = if self.slots[self.front].is_some() {
            1 - self.front
        } else {
            self.front
        };
        self.slots[slot] = Some(image);
        self.front = slot;
        self.fade_start = clock;
        self.fade_length = length.max(1e-3);
        slot
    }
}

/// The app's clouds: four layers, their motion, and their keyframes.
pub struct SkyClouds {
    seed: u32,
    cover: Cover,
    palette: &'static Palette,
    cols: i32,
    sky_rows: i32,
    worker: CloudWorker,
    /// Requests are numbered across all layers.
    next_generation: u64,
    /// Seconds of cloud life (runs with the speed setting, stops on pause).
    clock: f64,
    layers: [LayerState; CLOUD_LAYERS],
    /// Snapshot mode: keyframe intervals ignore render times, so a run is
    /// the same on any machine (the caller waits for each keyframe).
    lockstep: bool,
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
            clock: 0.0,
            lockstep: false,
            layers: std::array::from_fn(|_| LayerState {
                urgent: true,
                ..LayerState::default()
            }),
        }
    }

    /// The coverage preset.
    pub fn cover(&self) -> Cover {
        self.cover
    }

    /// Switches the coverage preset; the new sky fades in over
    /// [`QUICK_FADE`] seconds once it is ready.
    pub fn set_cover(&mut self, cover: Cover) {
        self.cover = cover;
        self.invalidate();
    }

    /// Switches the palette, fading like a cover change.
    pub fn set_palette(&mut self, palette: &'static Palette) {
        self.palette = palette;
        self.invalidate();
    }

    fn invalidate(&mut self) {
        for layer in &mut self.layers {
            layer.urgent = true;
            layer.queued = None;
            layer.wanted = self.next_generation + 1;
        }
    }

    /// Lockstep (snapshot mode): keyframe intervals stop stretching with
    /// render times, and [`SkyClouds::advance`] blocks for any keyframe
    /// the frame uses, so the same seed gives the same keyframes anywhere.
    pub fn set_lockstep(&mut self, lockstep: bool) {
        self.lockstep = lockstep;
    }

    /// Lockstep: whether some layer needs its requested keyframe in this
    /// frame (a first or urgent sky, or the next keyframe once the current
    /// fade has ended) and it is not among `arrivals` yet.
    fn needs_keyframe(&self, arrivals: &[StripImage]) -> bool {
        self.layers.iter().enumerate().any(|(band, layer)| {
            let arrived = arrivals
                .iter()
                .any(|image| image.band == band && image.generation == layer.generation);
            let needed = layer.front_image().is_none()
                || layer.urgent
                || (layer.queued.is_none() && layer.fade(self.clock) >= 1.0);
            layer.pending && !arrived && needed
        })
    }

    /// Seconds of cloud life so far.
    pub fn clock(&self) -> f64 {
        self.clock
    }

    /// The render time layer `band`'s intervals stretch to: its last
    /// render's, or 0 in lockstep.
    fn render_seconds(&self, band: usize) -> f64 {
        if self.lockstep {
            0.0
        } else {
            self.layers[band].render_seconds
        }
    }

    /// Layer `band`'s cloud-world column at the screen's left edge.
    pub fn view(&self, band: usize) -> f64 {
        self.layers[band].view
    }

    /// The strip in GPU slot `slot` of layer `band`, if any.
    pub fn slot_image(&self, band: usize, slot: usize) -> Option<&StripImage> {
        self.layers[band].slots[slot].as_ref()
    }

    /// The slot of layer `band` fading in.
    pub fn front_slot(&self, band: usize) -> usize {
        self.layers[band].front
    }

    /// How far layer `band`'s current fade has run, 0 to 1: the weight of
    /// the front slot over the other one.
    pub fn fade(&self, band: usize) -> f64 {
        self.layers[band].fade(self.clock)
    }

    /// Device pixels from the left edge of layer `band`'s strip in `slot`
    /// to the screen's left edge.
    pub fn strip_offset_px(&self, band: usize, slot: usize, cell: u32) -> f64 {
        let layer = &self.layers[band];
        layer.slots[slot].as_ref().map_or(0.0, |image| {
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

    fn request(&mut self, band: usize, time: f64, light: Option<Light>) {
        self.next_generation += 1;
        let layer = &mut self.layers[band];
        layer.generation = self.next_generation;
        layer.pending = true;
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
            time,
        });
    }

    /// Moves every layer one frame (its wind for at most
    /// [`MAX_MOTION_STEP`] seconds, plus its share of the Endless `camera`),
    /// runs the cloud clock, starts fades as keyframes become due, and asks
    /// for the next keyframes. Returns the `(layer, slot)` pairs whose
    /// strips changed (they need uploading).
    pub fn advance(
        &mut self,
        dt: f64,
        speed: f64,
        camera: f64,
        light: Option<Light>,
    ) -> Vec<(usize, usize)> {
        let step = if dt.is_finite() {
            dt.clamp(0.0, MAX_MOTION_STEP)
        } else {
            0.0
        };
        self.clock += step * speed.max(0.0);
        let clock = self.clock;
        let mut changed = Vec::new();
        let mut arrivals = self.worker.finished();
        if self.lockstep {
            // Block only for keyframes this frame uses; later ones keep
            // rendering in parallel. Keyframes take effect only at fade
            // ends (or at once for a first or urgent sky), so the result
            // does not depend on when they arrive.
            while self.needs_keyframe(&arrivals) {
                match self.worker.results.recv() {
                    Ok(image) => arrivals.push(image),
                    Err(_) => break,
                }
            }
        }
        for image in arrivals {
            let band = image.band.min(CLOUD_LAYERS - 1);
            let layer = &mut self.layers[band];
            if image.generation == layer.generation {
                layer.pending = false;
            }
            layer.render_seconds = image.render_seconds;
            if image.generation < layer.wanted {
                continue;
            }
            if layer.front_image().is_none() || layer.urgent {
                // First sky, or a cover or palette change: fade in now.
                layer.urgent = false;
                changed.push((band, layer.rotate(image, clock, QUICK_FADE)));
            } else {
                layer.queued = Some(image);
            }
        }
        for band in 0..CLOUD_LAYERS {
            let interval = effective_interval(self.cover, band, self.render_seconds(band), speed);
            let layer = &mut self.layers[band];
            layer.wind += LAYER_WIND[band] * speed.max(0.0) * step;
            if camera.is_finite() {
                layer.view = layer.wind + LAYER_PARALLAX[band] * camera;
            }
            // When the current fade is done, the queued keyframe fades in
            // over the whole cloud time between the two, so it never snaps
            // in, even when it arrived late.
            if layer.fade(clock) >= 1.0 {
                if let Some(next) = layer.queued.take() {
                    let since = layer.front_image().map_or(0.0, |image| image.time);
                    let length = (next.time - since).max(QUICK_FADE.min(interval));
                    changed.push((band, layer.rotate(next, clock, length)));
                }
            }
            // A queued strip the view has nearly run past is replaced.
            let runs_out = |image: &StripImage| {
                let into = layer.view - f64::from(image.origin);
                into < 0.0 || into > f64::from(MARGIN) * 1.5
            };
            if layer.queued.as_ref().is_some_and(runs_out) {
                layer.queued = None;
            }
            if layer.pending {
                continue;
            }
            if layer.urgent || layer.front_image().is_none() {
                self.request(band, clock, light);
            } else if layer.queued.is_none() {
                // Always one keyframe ahead: the next is due an interval
                // after the one now fading in.
                let front_time = layer.front_image().map_or(clock, |image| image.time);
                self.request(band, front_time + interval, light);
            }
        }
        changed
    }

    /// Blocks until every layer's newest requested strip arrives, then
    /// takes it as in [`SkyClouds::advance`] (tests). Returns false if the
    /// worker stopped.
    pub fn wait(&mut self) -> bool {
        self.wait_changed().is_some()
    }

    /// Like [`SkyClouds::wait`], returning the `(layer, slot)` pairs whose
    /// strips changed, or `None` if the worker stopped.
    fn wait_changed(&mut self) -> Option<Vec<(usize, usize)>> {
        let mut changed = Vec::new();
        while self.layers.iter().any(|layer| layer.pending) {
            let Ok(image) = self.worker.results.recv() else {
                return None;
            };
            let band = image.band.min(CLOUD_LAYERS - 1);
            let clock = self.clock;
            let layer = &mut self.layers[band];
            if image.generation == layer.generation {
                layer.pending = false;
            }
            layer.render_seconds = image.render_seconds;
            if image.generation < layer.wanted {
                continue;
            }
            if layer.front_image().is_none() || layer.urgent {
                layer.urgent = false;
                changed.push((band, layer.rotate(image, clock, QUICK_FADE)));
            } else {
                layer.queued = Some(image);
            }
        }
        Some(changed)
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
            time: 0.0,
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
    fn keyframes_at_different_times_differ() {
        let a = render_strip(&request(Cover::Storm, 3, 0, 120));
        let b = render_strip(&StripRequest {
            time: 20.0,
            ..request(Cover::Storm, 3, 0, 120)
        });
        assert_ne!(a.rgba, b.rgba, "storm clouds change in 20 s");
        assert_eq!(b.time, 20.0);
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
    fn storms_keyframe_faster_and_the_far_layer_slower() {
        assert!(keyframe_interval(Cover::Storm, 3) < keyframe_interval(Cover::Scattered, 3));
        assert!(keyframe_interval(Cover::Broken, 0) > keyframe_interval(Cover::Broken, 3));
        assert!((keyframe_interval(Cover::Clear, 2) - 8.0).abs() < 1e-12);
        assert!((keyframe_interval(Cover::Storm, 2) - 2.5).abs() < 1e-12);
        // Every layer keeps its own beat.
        for a in 0..CLOUD_LAYERS {
            for b in a + 1..CLOUD_LAYERS {
                let (x, y) = (
                    keyframe_interval(Cover::Broken, a),
                    keyframe_interval(Cover::Broken, b),
                );
                assert!((x - y).abs() > 0.5, "layers {a} and {b} share a beat");
            }
        }
    }

    #[test]
    fn layers_drift_at_their_own_speeds_and_nearer_is_faster() {
        let vga = palette_by_key("vga").unwrap();
        let mut sky = SkyClouds::new(3, 40, 30, Cover::Broken, vga);
        sky.advance(0.0, 1.0, 0.0, None);
        assert!(sky.wait());
        for band in 0..CLOUD_LAYERS {
            let image = sky.slot_image(band, sky.front_slot(band)).unwrap();
            assert_eq!(image.width, sky.strip_width());
            assert_eq!(image.origin, -MARGIN);
        }
        for _ in 0..300 {
            sky.advance(1.0 / 30.0, 1.0, 0.0, None);
        }
        for (band, wind) in LAYER_WIND.iter().enumerate() {
            assert!((sky.view(band) - 10.0 * wind).abs() < 1e-9, "{band}");
        }
        sky.advance(0.0, 1.0, 100.0, None);
        for band in 0..CLOUD_LAYERS {
            let expected = 10.0 * LAYER_WIND[band] + 100.0 * LAYER_PARALLAX[band];
            assert!((sky.view(band) - expected).abs() < 1e-9, "{band}");
        }
        assert!(LAYER_WIND.windows(2).all(|w| w[0] < w[1]));
        assert!(LAYER_PARALLAX.windows(2).all(|w| w[0] < w[1]));
        assert!(
            (sky.clock() - 10.0).abs() < 1e-9,
            "the cloud clock runs with the speed"
        );
    }

    #[test]
    fn lockstep_runs_give_the_same_keyframes() {
        let vga = palette_by_key("vga").unwrap();
        let run = || {
            let mut sky = SkyClouds::new(5, 40, 30, Cover::Storm, vga);
            sky.set_lockstep(true);
            let mut times = Vec::new();
            for _ in 0..(12 * 30) {
                // No waiting here: lockstep waits inside advance.
                sky.advance(1.0 / 30.0, 1.0, 0.0, None);
                // The first sky arrives on the frame after it is asked for.
                let Some(image) = sky.slot_image(3, sky.front_slot(3)) else {
                    continue;
                };
                if times.last() != Some(&image.time) {
                    times.push(image.time);
                }
            }
            times
        };
        let (a, b) = (run(), run());
        assert!(a.len() >= 3, "{a:?}");
        assert_eq!(a, b);
    }

    #[test]
    fn slow_renders_stretch_the_interval() {
        let base = keyframe_interval(Cover::Storm, 3);
        assert_eq!(effective_interval(Cover::Storm, 3, 0.1, 1.0), base);
        let slow = effective_interval(Cover::Storm, 3, 4.0, 1.0);
        assert!((slow - 4.0 * RENDER_HEADROOM).abs() < 1e-12, "{slow}");
        // At 4x the cloud clock runs faster than the renders.
        assert!(effective_interval(Cover::Storm, 3, 1.0, 4.0) > base);
        assert_eq!(effective_interval(Cover::Storm, 3, f64::NAN, 1.0), base);
    }

    #[test]
    fn late_keyframes_still_fade_over_their_whole_gap() {
        // Regression (owner, 2026-11-04: "Clouds just suddenly vanish"):
        // with the clock far ahead of the renders, every keyframe after the
        // first still fades over the cloud time since the one before.
        let vga = palette_by_key("vga").unwrap();
        let mut sky = SkyClouds::new(3, 60, 40, Cover::Storm, vga);
        sky.advance(0.0, 1.0, 0.0, None);
        assert!(sky.wait());
        let mut fades = 0;
        for _ in 0..(10 * 30) {
            // 4x speed, and no sleeping: the renders lag the clock.
            for (band, slot) in sky.advance(1.0 / 30.0, 4.0, 0.0, None) {
                let layer = &sky.layers[band];
                let (Some(front), Some(back)) = (&layer.slots[slot], &layer.slots[1 - slot]) else {
                    continue;
                };
                let gap = front.time - back.time;
                assert!(
                    layer.fade_length >= gap - 1e-9,
                    "layer {band} faded {} s over a {gap} s gap",
                    layer.fade_length
                );
                fades += 1;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(fades >= 4, "keyframes arrived: {fades}");
    }

    #[test]
    fn keyframes_cross_fade_one_into_the_next() {
        let vga = palette_by_key("vga").unwrap();
        let mut sky = SkyClouds::new(3, 40, 30, Cover::Storm, vga);
        // The first sky fades in.
        sky.advance(0.0, 1.0, 0.0, None);
        assert!(sky.wait());
        let first = sky.front_slot(0);
        assert_eq!(sky.fade(0), 0.0);
        // Run the clock; keyframes for later times arrive and rotate in.
        let mut times = vec![sky.slot_image(3, sky.front_slot(3)).unwrap().time];
        for _ in 0..(20 * 30) {
            let changed = sky.advance(1.0 / 30.0, 1.0, 0.0, None);
            for (band, slot) in changed {
                assert!(slot < SLOTS);
                if band == 3 {
                    times.push(sky.slot_image(3, slot).unwrap().time);
                }
            }
            for band in 0..CLOUD_LAYERS {
                let fade = sky.fade(band);
                assert!((0.0..=1.0).contains(&fade));
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(
            times.len() >= 3,
            "several keyframes in 20 s of storm: {times:?}"
        );
        assert!(
            times.windows(2).all(|w| w[1] > w[0]),
            "keyframes move forward: {times:?}"
        );
        // Both slots got used.
        assert!(sky.slot_image(0, first).is_some());
    }

    #[test]
    fn a_cover_change_fades_in_quickly_and_pausing_stops_the_clock() {
        let vga = palette_by_key("vga").unwrap();
        let mut sky = SkyClouds::new(3, 40, 30, Cover::Broken, vga);
        sky.advance(0.0, 1.0, 0.0, None);
        assert!(sky.wait());
        sky.set_cover(Cover::Clear);
        sky.advance(0.0, 1.0, 0.0, None);
        assert!(sky.wait());
        for band in 0..CLOUD_LAYERS {
            let image = sky.slot_image(band, sky.front_slot(band)).unwrap();
            assert!(
                image.rgba.chunks(4).all(|p| p[3] == 0),
                "the clear sky is fading in"
            );
        }
        let clock = sky.clock();
        for _ in 0..60 {
            sky.advance(0.0, 1.0, 0.0, None); // paused: no time passes
        }
        assert_eq!(sky.clock(), clock);
        for _ in 0..(QUICK_FADE * 30.0) as usize + 2 {
            sky.advance(1.0 / 30.0, 1.0, 0.0, None);
        }
        assert_eq!(sky.fade(2), 1.0, "the quick fade finished");
    }
}
