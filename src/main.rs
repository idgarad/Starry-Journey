//! Starry Journey application: window, input, and GPU presentation.
//!
//! The scene (city model, CPU rasterizer, and construction reveal) lives in
//! the library. This binary generates a Normal-mode city for the window's
//! grid, reveals it one logical cell at a time over 240 seconds at 1x,
//! uploads the shown image as a texture whenever it changes, and draws each
//! logical cell as a `cell x cell` block of device pixels. The message box
//! (`--message`, and the palette name after `P`) is rasterized on the CPU
//! and drawn over the city at device resolution.

use std::{
    error::Error,
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use starry_journey::clock::{format_remaining, ConstructionClock};
use starry_journey::config::{parse_args, LaunchOptions, USAGE};
use starry_journey::endless::EndlessCity;
use starry_journey::message::{message_font, render_message_box, MessageImage, Notice};
use starry_journey::model::building::Building;
use starry_journey::model::city::generate_normal_city;
use starry_journey::model::grid::{CellScaling, Grid};
use starry_journey::model::palette::{palette_by_key, palettes, Palette, DEFAULT_PALETTE_KEY};
use starry_journey::moon::{
    is_station, moon_at, moon_sprite, MoonState, MOON_SPRITE_REACH, PASS_SECONDS,
    STATION_SPRITE_REACH,
};
use starry_journey::quips::quip_at;
use starry_journey::raster::{generate_stars, CityRaster, RasterOptions, Star, WindowLighting};
use starry_journey::reveal::{light_revealed, ConstructionView, Reveal};
use starry_journey::station::{radio_line, StationFate};
use starry_journey::street::{apply_paint, mark_opaque, CellPaint, Span, StreetLayer};
use starry_journey::traffic::{Traffic, Verge};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Fullscreen, Window, WindowId},
};

/// Initial window size in physical pixels: 1080p, the assumed screen size
/// for testing (owner decision, 2026-11-03).
const INITIAL_SIZE: (u32, u32) = (1920, 1080);
/// Smallest window size in physical pixels.
const MIN_SIZE: (u32, u32) = (320, 180);
/// Speed range and step for the `+` and `-` keys (Rebuild Prompt: 0.25..4).
const SPEED_RANGE: (f64, f64) = (0.25, 4.0);
const SPEED_STEP: f64 = 0.25;
/// Extra street columns on each side of the screen while Endless scrolls,
/// so the GPU can slide the street by fractions of a cell.
const STREET_MARGIN: i32 = 2;
/// Window title prefix.
const TITLE: &str = "Starry Journey";

#[derive(Default)]
struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    fullscreen: bool,
    monitor_index: usize,
    options: LaunchOptions,
}

impl App {
    fn toggle_fullscreen(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        if self.fullscreen {
            window.set_fullscreen(None);
            self.fullscreen = false;
        } else {
            let monitors: Vec<_> = window.available_monitors().collect();
            if !monitors.is_empty() {
                self.monitor_index %= monitors.len();
            }
            let monitor = monitors.get(self.monitor_index).cloned();
            window.set_fullscreen(Some(Fullscreen::Borderless(monitor)));
            self.fullscreen = true;
        }
    }

    fn cycle_monitor(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        let monitor_count = window.available_monitors().count();
        if monitor_count == 0 {
            return;
        }
        self.monitor_index = (self.monitor_index + 1) % monitor_count;
        if self.fullscreen {
            let monitor = window.available_monitors().nth(self.monitor_index);
            window.set_fullscreen(Some(Fullscreen::Borderless(monitor)));
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attributes = Window::default_attributes()
            .with_title("Starry Journey")
            .with_inner_size(PhysicalSize::new(INITIAL_SIZE.0, INITIAL_SIZE.1))
            .with_min_inner_size(PhysicalSize::new(MIN_SIZE.0, MIN_SIZE.1));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("Could not create the application window: {error}");
                event_loop.exit();
                return;
            }
        };

        match pollster::block_on(Renderer::new(
            Arc::clone(&window),
            random_seed(),
            self.options.clone(),
        )) {
            Ok(renderer) => {
                self.window = Some(window);
                self.renderer = Some(renderer);
            }
            Err(error) => {
                eprintln!("Could not initialize the renderer: {error}");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.window.as_ref().map(|window| window.id()) != Some(window_id) {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size);
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let (Some(window), Some(renderer)) = (&self.window, &mut self.renderer) {
                    renderer.set_scale_factor(scale_factor);
                    renderer.resize(window.inner_size());
                }
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && !event.repeat =>
            {
                if let PhysicalKey::Code(key) = event.physical_key {
                    match key {
                        KeyCode::Escape => event_loop.exit(),
                        KeyCode::KeyF => self.toggle_fullscreen(),
                        KeyCode::Tab => self.cycle_monitor(),
                        KeyCode::KeyP => {
                            if let Some(renderer) = &mut self.renderer {
                                renderer.next_palette();
                            }
                        }
                        KeyCode::KeyE => {
                            if let Some(renderer) = &mut self.renderer {
                                renderer.toggle_endless();
                            }
                        }
                        KeyCode::Space => {
                            if let Some(renderer) = &mut self.renderer {
                                renderer.toggle_pause();
                            }
                        }
                        KeyCode::KeyR => {
                            if let Some(renderer) = &mut self.renderer {
                                renderer.restart(random_seed());
                            }
                        }
                        KeyCode::Equal | KeyCode::NumpadAdd => {
                            if let Some(renderer) = &mut self.renderer {
                                renderer.change_speed(SPEED_STEP);
                            }
                        }
                        KeyCode::Minus | KeyCode::NumpadSubtract => {
                            if let Some(renderer) = &mut self.renderer {
                                renderer.change_speed(-SPEED_STEP);
                            }
                        }
                        _ => {}
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let (Some(window), Some(renderer)) = (&self.window, &mut self.renderer) {
                    if window.is_minimized() == Some(true) {
                        return;
                    }
                    // A resize can arrive before the renderer exists (for
                    // example a maximize during startup) and be dropped.
                    // Checking the real size every frame means the surface
                    // and grid always match the window, so Windows never
                    // stretches a stale frame.
                    let size = window.inner_size();
                    if renderer.surface_size() != (size.width, size.height) {
                        renderer.resize(size);
                    }
                    renderer.advance();
                    let title = renderer.title();
                    if window.title() != title {
                        window.set_title(&title);
                    }
                    match renderer.render() {
                        Ok(()) => {}
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            renderer.resize(window.inner_size());
                        }
                        Err(wgpu::SurfaceError::Timeout) => {
                            eprintln!("Timed out while acquiring the next frame.");
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => {
                            eprintln!("The GPU ran out of memory.");
                            event_loop.exit();
                        }
                        Err(error) => {
                            eprintln!("Could not render the scene: {error}");
                            event_loop.exit();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

/// A seed for a new city, from the wall clock. Cities only need to differ
/// between launches and restarts, not be unpredictable.
fn random_seed() -> u32 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    (now.as_secs() as u32).wrapping_mul(0x9e37_79b9) ^ now.subsec_nanos()
}

/// The generated scene for one grid: the city, its stars, the ownership
/// raster, the construction reveal, the street, and the composed frame.
struct Scene {
    seed: u32,
    grid: Grid,
    buildings: Vec<Building>,
    stars: Vec<Star>,
    ownership: CityRaster,
    reveal: Reveal,
    view: ConstructionView,
    street: StreetLayer,
    /// The world strip `street` and `overlay` cover (the screen, or while
    /// Endless scrolls a strip following the road camera).
    street_span: Span,
    /// Normal mode and construction: the city view, transparent wherever
    /// nothing is built yet, drawn by the GPU over the sky and moon. Empty
    /// while Endless scrolls (the layers replace it).
    city: Vec<u8>,
    /// Street traffic; starts once the city is finished (Starry Journey
    /// extension: Normal mode has traffic too).
    traffic: Option<Traffic>,
    /// The palette the scene is drawn in (palettes are static data).
    palette: &'static Palette,
    /// The scrolling layers in Endless mode; `None` in Normal mode. They
    /// start moving once construction is complete.
    endless: Option<EndlessCity>,
    /// Background and stars only: the bottom of the picture.
    sky: Vec<u8>,
    /// The street, trees, and traffic, drawn by the GPU over the city
    /// (transparent elsewhere).
    overlay: Vec<u8>,
}

impl Scene {
    /// Generates the city for a `width x height` surface and reveals it up
    /// to `progress` (0 for a new city; the old progress after a resize).
    /// `endless` holds the Endless camera position, or `None` for Normal.
    fn new(
        seed: u32,
        width: u32,
        height: u32,
        palette: &'static Palette,
        progress: f64,
        scaling: CellScaling,
        endless: Option<f64>,
    ) -> Self {
        // Starry Journey extension: Normal mode has the road too, so the
        // grid reserves the seven road rows below the city.
        let grid = Grid::with_scaling(width, height, true, scaling);
        let city = endless.map(|camera| EndlessCity::with_camera(seed, &grid, palette, camera));
        // An Endless city is built like a Normal one first (owner request,
        // 2026-11-03), from the buildings its layers show at camera 0.
        let mut buildings = match &city {
            Some(city) => {
                let mut buildings = city.initial_buildings();
                for building in &mut buildings {
                    building.windows.lit.fill(false);
                }
                buildings
            }
            None => generate_normal_city(seed, &grid),
        };
        let stars = generate_stars(seed, &grid);
        let ownership = CityRaster::render(&grid, &buildings, &stars, palette);
        let mut reveal = Reveal::new(&ownership, seed);
        let cells = reveal.advance_to(progress).to_vec();
        light_revealed(&mut buildings, &ownership, &cells);
        let endless_tint = city.is_some();
        let view = Self::view_for(&grid, &buildings, &stars, palette, &reveal, endless_tint);
        let street = StreetLayer::new(&grid, seed, palette);
        let sky = CityRaster::render(&grid, &[], &stars, palette).rgba;
        let mut scene = Self {
            seed,
            grid,
            buildings,
            stars,
            ownership,
            reveal,
            view,
            street,
            street_span: Span::screen(&grid),
            city: Vec::new(),
            traffic: None,
            palette,
            endless: city,
            sky,
            overlay: Vec::new(),
        };
        scene.start_traffic_if_complete();
        scene.compose();
        scene
    }

    /// True once an Endless city is finished, so its layers scroll.
    fn is_scrolling(&self) -> bool {
        self.endless.is_some() && self.reveal.is_complete()
    }

    /// The Endless camera to keep across a rebuild, or `None` in Normal.
    fn endless_camera(&self) -> Option<f64> {
        self.endless.as_ref().map(EndlessCity::camera)
    }

    /// Moves the Endless layers by one frame once construction is done.
    /// The GPU places them, so the frame needs no composing. Returns true if
    /// any layer moved to a new whole column.
    fn scroll(&mut self, dt: f64, speed: f64) -> bool {
        if !self.reveal.is_complete() {
            return false;
        }
        let moved = match &mut self.endless {
            Some(city) => city.advance(dt, speed, self.palette),
            None => false,
        };
        self.follow_road_camera();
        moved
    }

    /// The road camera: the world column at the screen's left edge for the
    /// road, trees, and traffic. They share the nearest layer's speed (the
    /// root camera, Rebuild Prompt) while Endless scrolls, and stay at 0
    /// otherwise.
    fn road_view(&self) -> f64 {
        match &self.endless {
            Some(city) if self.is_scrolling() => city.view_x(3),
            _ => 0.0,
        }
    }

    /// The world strip the street buffer covers for the current road view:
    /// the screen in Normal mode and construction; while Endless scrolls, a
    /// strip from two columns left of the view to two past the right edge,
    /// so the GPU can slide it by fractions of a cell.
    fn street_span_now(&self) -> Span {
        if !self.is_scrolling() {
            return Span::screen(&self.grid);
        }
        Span {
            origin_x: self.road_view().floor() as i32 - STREET_MARGIN,
            width: self.grid.cols.max(1) + 2 * STREET_MARGIN,
            rows: self.grid.rows.max(1),
        }
    }

    /// Points traffic at the road camera and rebuilds the road and trees
    /// when the view crosses a whole column. O(1) unless rebuilding, then
    /// O(street cells).
    fn follow_road_camera(&mut self) {
        let view = self.road_view();
        if let Some(traffic) = &mut self.traffic {
            traffic.set_view(view);
        }
        let span = self.street_span_now();
        if span != self.street_span {
            self.street_span = span;
            self.street = StreetLayer::for_span(&self.grid, self.seed, self.palette, span);
        }
    }

    /// Device pixels from the street buffer's left edge to the screen's
    /// left edge (fractional; 0 in Normal mode).
    fn street_offset_px(&self) -> f64 {
        (self.road_view() - f64::from(self.street_span.origin_x)) * f64::from(self.grid.cell)
    }

    /// Rebuilds both cell-resolution images that change: the still city and
    /// the street overlay. The GPU draws, bottom to top: the sky, the moon,
    /// the city (this still city, or the Endless layers while scrolling),
    /// then the overlay, so buildings hide the moon and the street covers it
    /// at the horizon. O(grid cells + street).
    fn compose(&mut self) {
        self.compose_city();
        self.compose_overlay();
    }

    /// The still city: the construction view, opaque on built cells and
    /// transparent elsewhere; empty while Endless scrolls. O(grid cells).
    fn compose_city(&mut self) {
        if self.is_scrolling() {
            self.city.clear();
            return;
        }
        self.city.clone_from(&self.view.rgba);
        for (cell, pixel) in self.city.chunks_exact_mut(4).enumerate() {
            let built = self.ownership.owner[cell] >= 0 && self.reveal.is_revealed(cell);
            pixel[3] = if built { 255 } else { 0 };
        }
    }

    /// The overlay: the street's back layer (road and background trees),
    /// traffic, and its front layer, opaque on exactly those cells, over
    /// the world strip `street_span`. O(span cells + street).
    fn compose_overlay(&mut self) {
        self.overlay.clear();
        self.overlay.resize(self.street_span.cells() * 4, 0);
        let traffic_paint = paint_street(
            &self.street,
            self.traffic.as_ref(),
            self.grid,
            self.street_span,
            self.palette,
            &mut self.overlay,
        );
        self.street.mark_opaque(&mut self.overlay);
        mark_opaque(&traffic_paint, &mut self.overlay);
    }

    /// The picture without the moon and Endless layers, flattened on the
    /// CPU as the GPU stacks it: sky, then the still city, then the
    /// overlay. For tests and previews. O(grid cells).
    /// The overlay is mapped through `street_span` at whole-cell positions
    /// (the GPU adds the fraction).
    #[cfg(test)]
    fn flattened(&self) -> Vec<u8> {
        let mut image = self.sky.clone();
        for (pixel, cover) in image.chunks_exact_mut(4).zip(self.city.chunks_exact(4)) {
            if cover[3] == 255 {
                pixel.copy_from_slice(cover);
            }
        }
        let left = self.road_view().floor() as i32;
        for (cell, pixel) in image.chunks_exact_mut(4).enumerate() {
            let (x, row) = Span::screen(&self.grid).position(cell);
            if let Some(source) = self.street_span.index(x + left, row) {
                let cover = &self.overlay[source * 4..source * 4 + 4];
                if cover[3] == 255 {
                    pixel.copy_from_slice(cover);
                }
            }
        }
        image
    }

    /// Starts traffic once the city is complete (cars then drive in from
    /// off-screen), and points the street at the road camera.
    fn start_traffic_if_complete(&mut self) {
        if self.traffic.is_none() && self.reveal.is_complete() {
            self.traffic = Some(Traffic::new(self.seed, self.grid.cols));
        }
        self.follow_road_camera();
    }

    /// Moves traffic by one frame. Returns true if there is traffic (the
    /// frame then needs composing).
    fn step_traffic(&mut self, dt: f64, speed: f64) -> bool {
        match &mut self.traffic {
            Some(traffic) => {
                traffic.step(dt, speed);
                true
            }
            None => false,
        }
    }

    /// The shown image: the finished city where revealed, sky elsewhere.
    fn view_for(
        grid: &Grid,
        buildings: &[Building],
        stars: &[Star],
        palette: &Palette,
        reveal: &Reveal,
        endless_tint: bool,
    ) -> ConstructionView {
        let options = RasterOptions {
            lighting: WindowLighting::Target,
            endless_tint,
            ..RasterOptions::default()
        };
        let built = CityRaster::render_options(grid, buildings, stars, palette, options);
        let sky = CityRaster::render(grid, &[], stars, palette);
        ConstructionView::new(built.rgba, sky.rgba, reveal)
    }

    /// Re-colors the scene for another palette without regenerating it.
    fn set_palette(&mut self, palette: &'static Palette) {
        self.palette = palette;
        self.view = Self::view_for(
            &self.grid,
            &self.buildings,
            &self.stars,
            palette,
            &self.reveal,
            self.endless.is_some(),
        );
        self.street = StreetLayer::for_span(&self.grid, self.seed, palette, self.street_span);
        self.sky = CityRaster::render(&self.grid, &[], &self.stars, palette).rgba;
        if let Some(city) = &mut self.endless {
            city.set_palette(palette);
        }
        self.compose();
    }

    /// Moves construction to `progress` (it never goes backwards).
    fn advance_to(&mut self, progress: f64) {
        if self.reveal.is_complete() {
            return;
        }
        let cells = self.reveal.advance_to(progress).to_vec();
        light_revealed(&mut self.buildings, &self.ownership, &cells);
        self.view.reveal_cells(&cells);
        self.start_traffic_if_complete();
    }
}

struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
    /// The sky and stars (grid-sized).
    city_texture: wgpu::Texture,
    /// The message box image (premultiplied sRGB; 1 x 1 when hidden).
    message_texture: wgpu::Texture,
    /// Street, trees, and traffic, drawn over the city (grid-sized).
    overlay_texture: wgpu::Texture,
    /// The still city of Normal mode and construction (grid-sized).
    still_texture: wgpu::Texture,
    /// The moon sprite.
    moon_texture: wgpu::Texture,
    /// The moon sprite's top-left in fractional cells.
    moon_position: (f64, f64),
    /// The four Endless layer buffers, far to near (cell resolution).
    layer_textures: [wgpu::Texture; 4],
    /// The layer image revisions currently uploaded (0: none).
    layer_revisions: [u64; 4],
    bind_group: wgpu::BindGroup,
    scene: Scene,
    /// What the message box shows (`--message`, or a palette name).
    notice: Notice,
    /// The inputs of the box currently uploaded, so it is redrawn only when
    /// the text, palette, surface, or scale changes.
    message_key: Option<MessageKey>,
    /// Message box position and size in device pixels (width 0: hidden).
    message_rect: [f32; 4],
    palette_index: usize,
    speed: f64,
    last_frame: Option<Instant>,
    /// How construction progress advances (untimed or `--timer`).
    clock: ConstructionClock,
    /// When construction (re)started, for the duration clock.
    construction_start: Instant,
    /// When the app started; the moon follows real time from here.
    app_start: Instant,
    /// The moon as last drawn; `None` forces a redraw.
    moon_state: Option<MoonState>,
    /// Moon phase step on the first lap (`--moon-phase`).
    moon_start_step: u64,
    /// What happens to the station (`--station-fate`).
    station_fate: StationFate,
    /// Endless scrolling and traffic are paused (`Space`).
    paused: bool,
    options: LaunchOptions,
    scale_factor: f64,
}

impl Renderer {
    async fn new(
        window: Arc<Window>,
        seed: u32,
        options: LaunchOptions,
    ) -> Result<Self, Box<dyn Error>> {
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(Arc::clone(&window))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or("No compatible graphics adapter was found.")?;
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Starry Journey device"),
                    required_features: wgpu::Features::empty(),
                    // The adapter's real texture size limit (often 16384,
                    // not the 8192 default) leaves room for wide surfaces
                    // such as a city spanning several monitors.
                    required_limits: wgpu::Limits {
                        max_texture_dimension_2d: adapter.limits().max_texture_dimension_2d,
                        ..wgpu::Limits::default()
                    },
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await?;

        let capabilities = surface.get_capabilities(&adapter);
        // An sRGB surface re-encodes the decoded sRGB texture exactly.
        let surface_format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| capabilities.formats.first().copied())
            .ok_or("The graphics adapter offers no compatible surface formats.")?;
        let size = window.inner_size();
        let surface_config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("The graphics adapter does not support this window surface.")?;
        let surface_config = wgpu::SurfaceConfiguration {
            format: surface_format,
            ..surface_config
        };
        surface.configure(&device, &surface_config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("City presentation shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("present.wgsl").into()),
        });
        // Binding 1 is the uniform buffer; 0 and 2..=9 are textures (see
        // present.wgsl).
        let entries: Vec<wgpu::BindGroupLayoutEntry> = (0..10)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: if binding == 1 {
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    }
                } else {
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    }
                },
                count: None,
            })
            .collect();
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Scene textures and parameters"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("City presentation pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("City presentation pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Cell size, message box, and layer offsets"),
            size: PARAMS_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let palette_index = palettes()
            .iter()
            .position(|palette| palette.key == DEFAULT_PALETTE_KEY)
            .unwrap_or(0);
        let scale_factor = window.scale_factor();
        let scene = Scene::new(
            seed,
            surface_config.width,
            surface_config.height,
            palette_at(palette_index),
            0.0,
            options.cell_scaling(scale_factor),
            options.endless.then_some(0.0),
        );
        let city_texture = create_city_texture(&device, &scene.grid);
        let message_texture = create_message_texture(&device, 1, 1);
        let overlay_texture = create_city_texture(&device, &scene.grid);
        let still_texture = create_city_texture(&device, &scene.grid);
        let moon_texture = create_layer_texture(&device, 1, 1);
        let layer_textures = [0, 1, 2, 3].map(|_| create_layer_texture(&device, 1, 1));
        let bind_group = create_bind_group(
            &device,
            &bind_group_layout,
            &BindTextures {
                background: &city_texture,
                message: &message_texture,
                overlay: &overlay_texture,
                layers: layer_textures.each_ref(),
                moon: &moon_texture,
                still: &still_texture,
            },
            &params,
        );
        let mut renderer = Self {
            surface,
            device,
            queue,
            surface_config,
            pipeline,
            bind_group_layout,
            params,
            city_texture,
            message_texture,
            overlay_texture,
            still_texture,
            moon_texture,
            moon_position: (0.0, 0.0),
            layer_textures,
            layer_revisions: [0; 4],
            bind_group,
            scene,
            notice: Notice::new(options.message.clone()),
            message_key: None,
            message_rect: [0.0; 4],
            palette_index,
            speed: 1.0,
            last_frame: None,
            clock: options.clock,
            construction_start: Instant::now(),
            app_start: Instant::now(),
            moon_state: None,
            moon_start_step: options.moon_start_step,
            station_fate: options.station_fate,
            paused: false,
            options,
            scale_factor,
        };
        renderer.upload();
        renderer.update_message();
        Ok(renderer)
    }

    /// The configured surface size in physical pixels.
    fn surface_size(&self) -> (u32, u32) {
        (self.surface_config.width, self.surface_config.height)
    }

    /// The cell scaling for the current launch options and display scale.
    fn scaling(&self) -> CellScaling {
        self.options.cell_scaling(self.scale_factor)
    }

    /// Records a new display scale factor (for example after moving the
    /// window to another monitor); the next resize uses it.
    fn set_scale_factor(&mut self, scale_factor: f64) {
        self.scale_factor = scale_factor;
    }

    /// Reconfigures the surface and rebuilds the city with the same seed for
    /// the new grid (Rebuild Prompt: resize rebuilds using the same seed).
    fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.surface_config.width = size.width;
        self.surface_config.height = size.height;
        self.surface.configure(&self.device, &self.surface_config);
        // The scene grid always reserves the road rows (see Scene::new).
        let new_grid = Grid::with_scaling(size.width, size.height, true, self.scaling());
        if new_grid != self.scene.grid {
            // Same seed, same construction progress and Endless camera
            // (Rebuild Prompt: resize preserves progress).
            let progress = self.scene.reveal.progress();
            self.scene = Scene::new(
                self.scene.seed,
                size.width,
                size.height,
                palette_at(self.palette_index),
                progress,
                self.scaling(),
                self.scene.endless_camera(),
            );
            self.moon_state = None;
            self.update_moon(Instant::now());
            self.scene.compose();
            self.upload();
        }
    }

    /// Generates a new city with a new seed and restarts construction.
    fn restart(&mut self, seed: u32) {
        self.scene = Scene::new(
            seed,
            self.surface_config.width,
            self.surface_config.height,
            palette_at(self.palette_index),
            0.0,
            self.scaling(),
            self.scene.endless.is_some().then_some(0.0),
        );
        // Restart restarts a duration timer (Rebuild Prompt); resizing keeps it.
        self.construction_start = Instant::now();
        self.paused = false;
        self.moon_state = None;
        self.update_moon(Instant::now());
        self.scene.compose();
        self.upload();
    }

    /// Switches to the next palette without regenerating geometry or
    /// changing construction progress.
    fn next_palette(&mut self) {
        self.palette_index = (self.palette_index + 1) % palettes().len();
        let palette = palette_at(self.palette_index);
        self.scene.set_palette(palette);
        self.notice
            .flash(palette.label, self.seconds(Instant::now()));
        self.moon_state = None;
        self.update_moon(Instant::now());
        self.scene.compose();
        self.upload();
    }

    /// Switches between Normal and Endless mode (`E`, a Starry Journey
    /// addition). The new mode's city uses the same seed and keeps the
    /// construction progress; an Endless camera starts at 0.
    fn toggle_endless(&mut self) {
        let endless = self.scene.endless.is_none();
        let progress = self.scene.reveal.progress();
        self.scene = Scene::new(
            self.scene.seed,
            self.surface_config.width,
            self.surface_config.height,
            palette_at(self.palette_index),
            progress,
            self.scaling(),
            endless.then_some(0.0),
        );
        self.paused = false;
        self.moon_state = None;
        self.update_moon(Instant::now());
        self.scene.compose();
        self.upload();
    }

    /// Pauses or resumes Endless scrolling and traffic (`Space`). Drawing,
    /// the moon, and construction continue.
    fn toggle_pause(&mut self) {
        if self.scene.endless.is_some() {
            self.paused = !self.paused;
        }
    }

    /// Changes construction speed by `delta`, within 0.25x to 4x.
    fn change_speed(&mut self, delta: f64) {
        self.speed = (self.speed + delta).clamp(SPEED_RANGE.0, SPEED_RANGE.1);
    }

    /// Advances construction by the real time since the previous frame and
    /// uploads the image if any cell changed.
    fn advance(&mut self) {
        let now = Instant::now();
        let dt = self
            .last_frame
            .map_or(0.0, |last| now.duration_since(last).as_secs_f64());
        self.last_frame = Some(now);
        let elapsed = now.duration_since(self.construction_start).as_secs_f64();
        let progress = self
            .clock
            .progress(self.scene.reveal.progress(), dt, self.speed, elapsed);
        let was_complete = self.scene.reveal.is_complete();
        self.scene.advance_to(progress);
        // The frame that completes an Endless city switches to the layers.
        let finished = !was_complete && self.scene.reveal.is_complete();
        self.update_moon(now);
        let motion_dt = if self.paused { 0.0 } else { dt };
        let traffic_moved = self.scene.step_traffic(motion_dt, self.speed);
        // The GPU places the layers and the moon, so motion needs only new
        // offsets (and an upload when a layer buffer was rebased).
        self.scene.scroll(motion_dt, self.speed);
        let city_changed = self.scene.view.take_dirty() || finished;
        if city_changed {
            self.scene.compose_city();
            self.upload_still();
        }
        if traffic_moved || city_changed {
            self.scene.compose_overlay();
            self.upload_overlay();
        }
        self.upload_layers();
        self.write_params();
        self.update_status(elapsed);
        self.update_message();
    }

    /// Shows the silly startup lines while an Endless city is being built,
    /// and clears them when it is done.
    fn update_status(&mut self, construction_seconds: f64) {
        // The station's radio calls come first (owner idea, 2026-11-03),
        // in both modes; then the Endless startup lines.
        let radio = self.moon_state.filter(is_station).and_then(|moon| {
            radio_line(moon.lap, moon.lap_seconds, self.station_fate, PASS_SECONDS)
        });
        let building = self.scene.endless.is_some() && !self.scene.reveal.is_complete();
        let quip = building.then(|| quip_at(self.scene.seed, construction_seconds));
        self.notice.set_status(radio.or(quip));
    }

    /// Seconds since the app started, the clock for the message notice.
    fn seconds(&self, now: Instant) -> f64 {
        now.duration_since(self.app_start).as_secs_f64()
    }

    /// Redraws and uploads the message box if what it shows, the palette,
    /// the surface, or the scale changed since the last upload.
    fn update_message(&mut self) {
        let text = self
            .notice
            .text(self.seconds(Instant::now()))
            .map(str::to_string);
        let key = MessageKey {
            text,
            palette_index: self.palette_index,
            surface: self.surface_size(),
            scale_factor: self.scale_factor,
        };
        if self.message_key.as_ref() == Some(&key) {
            return;
        }
        let image = key.text.as_deref().and_then(|text| {
            render_message_box(
                message_font(),
                text,
                key.surface,
                key.scale_factor,
                palette_at(key.palette_index),
            )
        });
        self.message_key = Some(key);
        match image {
            Some(image) => self.upload_message(&image),
            None => self.message_rect = [0.0; 4],
        }
        self.write_params();
    }

    /// Uploads a message box image, recreating its texture when the size
    /// changes.
    fn upload_message(&mut self, image: &MessageImage) {
        let size = self.message_texture.size();
        if size.width != image.width || size.height != image.height {
            self.message_texture = create_message_texture(&self.device, image.width, image.height);
            self.rebind();
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.message_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &image.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(image.width * 4),
                rows_per_image: Some(image.height),
            },
            self.message_texture.size(),
        );
        self.message_rect = [
            image.x as f32,
            image.y as f32,
            image.width as f32,
            image.height as f32,
        ];
    }

    /// Recreates the bind group after a texture was replaced.
    fn rebind(&mut self) {
        self.bind_group = create_bind_group(
            &self.device,
            &self.bind_group_layout,
            &BindTextures {
                background: &self.city_texture,
                message: &self.message_texture,
                overlay: &self.overlay_texture,
                layers: self.layer_textures.each_ref(),
                moon: &self.moon_texture,
                still: &self.still_texture,
            },
            &self.params,
        );
    }

    /// Uploads any Endless layer buffer whose image changed (a rebase, a
    /// palette change, or a new city), reusing texture storage when the size
    /// matches. Nothing is uploaded while the layers only move.
    fn upload_layers(&mut self) {
        let Some(city) = &self.scene.endless else {
            return;
        };
        let mut recreated = false;
        for index in 0..4 {
            let image = city.layer_image(index);
            if self.layer_revisions[index] == image.revision {
                continue;
            }
            let (width, height) = (image.width as u32, image.height as u32);
            let size = self.layer_textures[index].size();
            if size.width != width || size.height != height {
                self.layer_textures[index] = create_layer_texture(&self.device, width, height);
                recreated = true;
            }
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.layer_textures[index],
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                image.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
                    rows_per_image: Some(height),
                },
                self.layer_textures[index].size(),
            );
            self.layer_revisions[index] = image.revision;
        }
        if recreated {
            self.rebind();
        }
    }

    /// Writes the cell size, the city switches, the message box rectangle,
    /// the Endless layer offsets, and the moon offset for the shader. Called
    /// every frame: the layers and the moon move by fractions of a pixel.
    fn write_params(&self) {
        let cell = self.scene.grid.cell as f32;
        let [x, y, width, height] = self.message_rect;
        let scrolling = self.scene.is_scrolling();
        let offset = |layer: usize| match &self.scene.endless {
            Some(city) if scrolling => city.source_offset_px(layer) as f32,
            _ => 0.0,
        };
        let layers_on = if scrolling { 1.0 } else { 0.0 };
        let still_on = if scrolling { 0.0 } else { 1.0 };
        // The screen's top-left measured from the sprite's top-left, in
        // device pixels: minus the sprite position. Computed in f64.
        let cell64 = f64::from(self.scene.grid.cell);
        let moon_x = (-self.moon_position.0 * cell64) as f32;
        let moon_y = (-self.moon_position.1 * cell64) as f32;
        let params: [f32; 28] = [
            cell,
            layers_on,
            still_on,
            self.scene.street_offset_px() as f32,
            x,
            y,
            width,
            height,
            offset(0),
            0.0,
            0.0,
            0.0,
            offset(1),
            0.0,
            0.0,
            0.0,
            offset(2),
            0.0,
            0.0,
            0.0,
            offset(3),
            0.0,
            0.0,
            0.0,
            moon_x,
            moon_y,
            1.0,
            0.0,
        ];
        let bytes: Vec<u8> = params
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        self.queue.write_buffer(&self.params, 0, &bytes);
    }

    /// Moves the moon to its exact position for `now`, and redraws and
    /// uploads its sprite when its look changes: a new lap or phase (or a
    /// forced redraw after `moon_state` was cleared), or every frame on the
    /// station lap, whose battle animates.
    fn update_moon(&mut self, now: Instant) {
        let seconds = now.duration_since(self.app_start).as_secs_f64();
        let moon = moon_at(&self.scene.grid, seconds, self.moon_start_step);
        let reach = f64::from(if is_station(&moon) {
            STATION_SPRITE_REACH
        } else {
            MOON_SPRITE_REACH
        });
        self.moon_position = (moon.exact_x - reach, moon.exact_row - reach);
        let look = |m: &MoonState| (m.lap, m.phase.to_bits());
        let same_look = self.moon_state.as_ref().map(look) == Some(look(&moon));
        if same_look && !is_station(&moon) {
            return;
        }
        self.moon_state = Some(moon);
        let sprite = moon_sprite(&moon, palette_at(self.palette_index), self.station_fate);
        let size = sprite.size as u32;
        if self.moon_texture.size().width != size {
            self.moon_texture = create_layer_texture(&self.device, size, size);
            self.rebind();
        }
        self.queue.write_texture(
            self.moon_texture.as_image_copy(),
            &sprite.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size * 4),
                rows_per_image: Some(size),
            },
            self.moon_texture.size(),
        );
    }

    /// Window title with construction progress and speed.
    fn title(&self) -> String {
        let grid = &self.scene.grid;
        let size = format!(
            "{}x{} @ {}px - surface {}x{}",
            grid.cols, grid.rows, grid.cell, self.surface_config.width, self.surface_config.height
        );
        let pace = if self.clock.uses_speed() {
            format!("{}x", self.speed)
        } else {
            let elapsed = self.construction_start.elapsed().as_secs_f64();
            let left = self.clock.remaining(elapsed).unwrap_or(0.0);
            format!("{} left", format_remaining(left))
        };
        let mode = if self.scene.endless.is_some() {
            "endless - "
        } else {
            ""
        };
        if self.scene.is_scrolling() {
            let state = if self.paused {
                "paused".to_string()
            } else {
                format!("scrolling {}x", self.speed)
            };
            format!("{TITLE} - {mode}{state} - {size}")
        } else if self.scene.reveal.is_complete() {
            format!("{TITLE} - {mode}complete - {size}")
        } else {
            let percent = (self.scene.reveal.progress() * 100.0).floor();
            format!("{TITLE} - {mode}building {percent}% - {pace} - {size}")
        }
    }

    /// Uploads everything for a new or re-colored scene: the sky, the still
    /// city, the overlay, and any changed layer, recreating the grid-sized
    /// textures only when the grid dimensions change.
    fn upload(&mut self) {
        let width = self.scene.grid.cols.max(1) as u32;
        let height = self.scene.grid.rows.max(1) as u32;
        let size = self.city_texture.size();
        if size.width != width || size.height != height {
            self.city_texture = create_city_texture(&self.device, &self.scene.grid);
            self.still_texture = create_city_texture(&self.device, &self.scene.grid);
            self.rebind();
        }
        self.write_grid_texture(&self.city_texture, &self.scene.sky);
        self.upload_still();
        self.upload_overlay();
        self.upload_layers();
        self.write_params();
    }

    /// Uploads the still city (nothing while Endless scrolls; the shader
    /// skips it then).
    fn upload_still(&mut self) {
        if !self.scene.city.is_empty() {
            self.write_grid_texture(&self.still_texture, &self.scene.city);
        }
    }

    /// Uploads the street overlay.
    /// Uploads the street overlay, resizing its texture to the street
    /// strip (wider than the screen while Endless scrolls).
    fn upload_overlay(&mut self) {
        let span = self.scene.street_span;
        let (width, height) = (span.width.max(1) as u32, span.rows.max(1) as u32);
        let size = self.overlay_texture.size();
        if size.width != width || size.height != height {
            self.overlay_texture = create_layer_texture(&self.device, width, height);
            self.rebind();
        }
        self.write_grid_texture(&self.overlay_texture, &self.scene.overlay);
    }

    /// Writes an RGBA image of exactly the texture's size into it.
    fn write_grid_texture(&self, texture: &wgpu::Texture, rgba: &[u8]) {
        let size = texture.size();
        if rgba.len() != (size.width * size.height * 4) as usize {
            return; // a stale image for another grid; the next compose fixes it
        }
        self.queue.write_texture(
            texture.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.width * 4),
                rows_per_image: Some(size.height),
            },
            size,
        );
    }

    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let frame = self.surface.get_current_texture()?;
        let frame_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("City frame encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Present city"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &frame_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }
}

/// Paints the street's back layer (road and background trees), traffic,
/// and the front trees into `rgba`. With traffic, headlights light the trees
/// ahead of each car: top trees from the top lane, bottom trees from the
/// bottom lane. `rgba` covers the world strip `span`. Returns the traffic's
/// cells.
fn paint_street(
    street: &StreetLayer,
    traffic: Option<&Traffic>,
    grid: Grid,
    span: Span,
    palette: &Palette,
    rgba: &mut [u8],
) -> CellPaint {
    let mut paint = CellPaint::new();
    match traffic {
        None => {
            street.apply_back(rgba);
            street.apply_front(rgba);
        }
        Some(traffic) => {
            let light = palette.headlight;
            let back_glow = |x, row| traffic.glow(&grid, x, row, Verge::Back);
            let front_glow = |x, row| traffic.glow(&grid, x, row, Verge::Front);
            street.apply_back_lit(rgba, span, light, back_glow);
            traffic.paint_span(&grid, span, palette, &mut paint);
            apply_paint(&paint, rgba);
            street.apply_front_lit(rgba, span, light, front_glow);
        }
    }
    paint
}

/// The palette at `index`, falling back to the default palette.
fn palette_at(index: usize) -> &'static Palette {
    palettes()
        .get(index)
        .or_else(|| palette_by_key(DEFAULT_PALETTE_KEY))
        .expect("the default palette exists")
}

/// Creates the logical-resolution city texture (sRGB, one texel per cell).
fn create_city_texture(device: &wgpu::Device, grid: &Grid) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Logical city buffer"),
        size: wgpu::Extent3d {
            width: grid.cols.max(1) as u32,
            height: grid.rows.max(1) as u32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// The inputs that decide the message box image.
#[derive(Debug, Clone, PartialEq)]
struct MessageKey {
    text: Option<String>,
    palette_index: usize,
    surface: (u32, u32),
    scale_factor: f64,
}

/// Creates the device-resolution message box texture. Its bytes are
/// premultiplied sRGB values that the shader blends itself, so the format is
/// plain `Rgba8Unorm`, not an sRGB format.
fn create_message_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Message box"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Size of the shader's `Params` uniform: cell, message, and four layer
/// vectors of four floats.
const PARAMS_BYTES: u64 = 7 * 16;

/// Creates an Endless layer texture: one sRGB texel per cell, alpha 0 on
/// sky. The shader decodes it to linear light when it loads a texel.
fn create_layer_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Endless layer buffer"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// The textures the presentation shader reads.
struct BindTextures<'a> {
    background: &'a wgpu::Texture,
    message: &'a wgpu::Texture,
    overlay: &'a wgpu::Texture,
    layers: [&'a wgpu::Texture; 4],
    moon: &'a wgpu::Texture,
    still: &'a wgpu::Texture,
}

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    textures: &BindTextures<'_>,
    params: &wgpu::Buffer,
) -> wgpu::BindGroup {
    let view =
        |texture: &wgpu::Texture| texture.create_view(&wgpu::TextureViewDescriptor::default());
    // Bindings 0, 2, 3, 4..=7 (see present.wgsl); 1 is the uniform.
    let views = [
        (0, view(textures.background)),
        (2, view(textures.message)),
        (3, view(textures.overlay)),
        (4, view(textures.layers[0])),
        (5, view(textures.layers[1])),
        (6, view(textures.layers[2])),
        (7, view(textures.layers[3])),
        (8, view(textures.moon)),
        (9, view(textures.still)),
    ];
    let mut entries: Vec<wgpu::BindGroupEntry> = views
        .iter()
        .map(|(binding, view)| wgpu::BindGroupEntry {
            binding: *binding,
            resource: wgpu::BindingResource::TextureView(view),
        })
        .collect();
    entries.push(wgpu::BindGroupEntry {
        binding: 1,
        resource: params.as_entire_binding(),
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Scene textures and parameters"),
        layout,
        entries: &entries,
    })
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = match parse_args(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("starry-journey: {message}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    if options.help {
        println!("{USAGE}");
        return Ok(());
    }
    let event_loop = EventLoop::new()?;
    let mut app = App {
        options,
        ..App::default()
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_pixel_size_scene_shows_more_city_on_bigger_screens() {
        let fixed = CellScaling::Fixed(4);
        let small = Scene::new(14, 1280, 720, classic(), 1.0, fixed, None);
        let large = Scene::new(14, 2560, 1440, classic(), 1.0, fixed, None);
        assert_eq!((small.grid.cols, small.grid.rows), (320, 180));
        assert_eq!((large.grid.cols, large.grid.rows), (640, 360));
        assert!(large.buildings.len() > small.buildings.len() * 3 / 2);
        let tallest = large
            .buildings
            .iter()
            .map(|b| b.roof_height())
            .max()
            .unwrap();
        // 240-row reference minus the 7 road rows: floor(233 * 0.82).
        assert!(tallest <= large.grid.max_h && large.grid.max_h == 191);
        assert!(large.reveal.is_complete());
    }

    #[test]
    fn normal_scene_has_a_road_and_trees_from_the_start() {
        let scene = Scene::new(15, 1280, 720, classic(), 0.0, CellScaling::Fixed(3), None);
        let grid = scene.grid;
        assert_eq!(grid.ground_row, grid.rows - 7, "road rows are reserved");
        let image = scene.flattened();
        let pixel = |x: i32, row: i32| {
            let i = (row * grid.cols + x) as usize * 4;
            [image[i], image[i + 1], image[i + 2]]
        };
        assert_eq!(pixel(1, grid.ground_row), classic().lane.0, "top curb");
        assert_eq!(pixel(1, grid.rows - 1), classic().lane.0, "bottom curb");
        let road_cells = (0..grid.cols)
            .filter(|&x| pixel(x, grid.ground_row + 2) == classic().road.0)
            .count();
        assert!(road_cells > grid.cols as usize / 2);
        assert_ne!(image, scene.view.rgba, "street is drawn over the empty sky");
        assert!(scene
            .buildings
            .iter()
            .all(|b| b.roof_height() <= grid.max_h));
    }

    #[test]
    fn duration_clock_finishes_the_scene_on_time_at_any_speed() {
        let clock = ConstructionClock::Duration { seconds: 10.0 };
        for speed in [0.25, 1.0, 4.0] {
            let mut scene = Scene::new(16, 1280, 720, classic(), 0.0, CellScaling::Fixed(3), None);
            let mut elapsed = 0.0;
            while elapsed < 5.0 {
                elapsed += 1.0 / 60.0;
                let progress = clock.progress(scene.reveal.progress(), 1.0 / 60.0, speed, elapsed);
                scene.advance_to(progress);
            }
            let half = scene.reveal.shown() as f64 / scene.reveal.total() as f64;
            assert!((half - 0.5).abs() < 0.01, "speed {speed}: {half}");
            scene.advance_to(clock.progress(scene.reveal.progress(), 0.0, speed, 10.0));
            assert!(scene.reveal.is_complete(), "speed {speed}");
        }
    }

    #[test]
    fn still_city_covers_exactly_the_built_cells_so_it_hides_the_moon() {
        // The GPU draws the moon below the city, so the moon shows wherever
        // the still city is transparent and is hidden wherever it is opaque.
        for progress in [0.0, 0.5, 1.0] {
            let scene = Scene::new(
                17,
                960,
                540,
                classic(),
                progress,
                CellScaling::Fixed(3),
                None,
            );
            let mut built = 0;
            for (cell, pixel) in scene.city.chunks(4).enumerate() {
                let expected = scene.ownership.owner[cell] >= 0 && scene.reveal.is_revealed(cell);
                assert_eq!(pixel[3] == 255, expected, "cell {cell} at {progress}");
                assert!(pixel[3] == 0 || pixel[3] == 255);
                built += usize::from(expected);
            }
            assert_eq!(built > 0, progress > 0.0, "{progress}");
        }
        // The street also covers the moon at the horizon.
        let scene = Scene::new(17, 960, 540, classic(), 1.0, CellScaling::Fixed(3), None);
        let grid = scene.grid;
        let road_row = (grid.ground_row + 2) * grid.cols;
        assert!((0..grid.cols).all(|x| scene.overlay[(road_row + x) as usize * 4 + 3] == 255));
    }

    #[test]
    fn traffic_with_lit_headlights_starts_only_after_the_city_is_finished() {
        let mut scene = Scene::new(18, 1920, 1080, classic(), 0.5, CellScaling::Fixed(3), None);
        assert!(scene.traffic.is_none(), "no cars while building");
        assert!(!scene.step_traffic(1.0 / 60.0, 1.0));
        scene.advance_to(1.0);
        assert!(scene.traffic.is_some(), "cars start when the city is done");
        for _ in 0..(20 * 60) {
            scene.step_traffic(1.0 / 60.0, 1.0);
        }
        scene.compose();
        let headlight = classic().headlight.0;
        let lit = scene
            .flattened()
            .chunks(4)
            .filter(|pixel| pixel[..3] == headlight)
            .count();
        assert!(lit > 0, "headlights are on");
        let finished = Scene::new(18, 1920, 1080, classic(), 1.0, CellScaling::Fixed(3), None);
        assert!(
            finished.traffic.is_some(),
            "a resized finished city keeps its traffic"
        );

        // Park a car 6 cells before a background tree trunk (the cone widens with
        // distance, so it reaches the road edge there) and check the trunk glows.
        let mut lit_scene = Scene::new(19, 1920, 1080, classic(), 1.0, CellScaling::Fixed(3), None);
        let grid = lit_scene.grid;
        let &(cell, plain) = lit_scene
            .street
            .back_trees
            .iter()
            .find(|&&(cell, _)| (cell as i32 / grid.cols) == grid.ground_row - 1)
            .expect("a background tree trunk on the road edge");
        let trunk_x = cell as i32 % grid.cols;
        let traffic = lit_scene.traffic.as_mut().unwrap();
        traffic.vehicles.truncate(1);
        traffic.vehicles[0].lane = starry_journey::traffic::Lane::Upper;
        traffic.vehicles[0].x = f64::from(trunk_x + 6); // cone is wide enough here
        traffic.vehicles[0].length = 4;
        lit_scene.compose();
        let image = lit_scene.flattened();
        let shown = &image[cell * 4..cell * 4 + 3];
        assert_ne!(shown, &plain.0[..], "the trunk ahead of the car is lit");
    }

    #[test]
    fn presentation_shader_passes_wgsl_validation() {
        let module = naga::front::wgsl::parse_str(include_str!("present.wgsl"))
            .expect("shader should parse");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("shader should validate");
    }

    fn classic() -> &'static Palette {
        palette_by_key("classic").unwrap()
    }

    #[test]
    fn new_scene_covers_the_window_grid_and_starts_as_empty_sky() {
        let scene = Scene::new(11, 1280, 720, classic(), 0.0, CellScaling::Spec, None);
        assert_eq!(
            (scene.grid.cols, scene.grid.rows, scene.grid.cell),
            (427, 240, 3)
        );
        assert!(!scene.buildings.is_empty());
        assert!(!scene.stars.is_empty());
        assert_eq!(scene.reveal.shown(), 0);
        let sky = CityRaster::render(&scene.grid, &[], &scene.stars, classic());
        assert_eq!(scene.view.rgba, sky.rgba, "nothing built yet");
        assert!(scene.buildings.iter().all(|b| b.windows.lit_count() == 0));
    }

    #[test]
    fn scene_builds_up_over_240_seconds_and_ends_fully_built() {
        let mut scene = Scene::new(12, 1280, 720, classic(), 0.0, CellScaling::Spec, None);
        let clock = ConstructionClock::Untimed;
        let step = |scene: &mut Scene| {
            let progress = clock.progress(scene.reveal.progress(), 1.0 / 60.0, 1.0, 0.0);
            scene.advance_to(progress);
        };
        for _ in 0..(120 * 60) {
            step(&mut scene);
        }
        let half = scene.reveal.shown() as f64 / scene.reveal.total() as f64;
        assert!((half - 0.5).abs() < 0.001, "half built after 120 s: {half}");
        for _ in 0..(121 * 60) {
            step(&mut scene);
        }
        assert!(scene.reveal.is_complete());
        let built = CityRaster::render_with(
            &scene.grid,
            &scene.buildings,
            &scene.stars,
            classic(),
            WindowLighting::Target,
        );
        assert_eq!(scene.view.rgba, built.rgba);
    }

    #[test]
    fn resize_keeps_progress_and_palette_change_keeps_the_reveal() {
        let mut scene = Scene::new(13, 1280, 720, classic(), 0.0, CellScaling::Spec, None);
        scene.advance_to(150.0 / 240.0);
        let progress = scene.reveal.progress();
        let resized = Scene::new(13, 1920, 1080, classic(), progress, CellScaling::Spec, None);
        assert_eq!(resized.reveal.progress(), progress);
        let expected = (progress * resized.reveal.total() as f64).floor() as usize;
        assert_eq!(resized.reveal.shown(), expected);

        let shown = scene.reveal.shown();
        scene.set_palette(palette_by_key("artdeco").unwrap());
        assert_eq!(scene.reveal.shown(), shown);
        let sky = CityRaster::render(
            &scene.grid,
            &[],
            &scene.stars,
            palette_by_key("artdeco").unwrap(),
        );
        let unrevealed = (0..scene.ownership.owner.len())
            .find(|&cell| !scene.reveal.is_revealed(cell))
            .unwrap();
        assert_eq!(
            scene.view.rgba[unrevealed * 4..unrevealed * 4 + 4],
            sky.rgba[unrevealed * 4..unrevealed * 4 + 4]
        );
    }

    #[test]
    fn endless_scene_is_built_first_and_then_scrolls_seamlessly() {
        let fixed = CellScaling::Fixed(3);
        let mut scene = Scene::new(21, 960, 540, classic(), 0.0, fixed, Some(0.0));
        assert!(!scene.is_scrolling(), "starts by building");
        assert!(!scene.scroll(1.0 / 30.0, 4.0), "no motion while building");
        assert_eq!(scene.endless_camera(), Some(0.0));
        // Windows start dark and light as they are revealed.
        assert!(scene
            .buildings
            .iter()
            .all(|b| !b.windows.lit.contains(&true)));

        scene.advance_to(0.999_999);
        scene.compose();
        scene.advance_to(1.0);
        scene.compose();
        let last_built = scene.view.rgba.clone();
        assert!(scene.is_scrolling());
        // The first scrolling frame, before any motion, is the built city
        // (the street and traffic aside, which are drawn the same way).
        let mut layers_only = Vec::new();
        let mut mask = Vec::new();
        scene
            .endless
            .as_ref()
            .unwrap()
            .compose(&scene.sky, &mut layers_only, &mut mask);
        assert_eq!(layers_only, last_built);

        let mut moved = false;
        for _ in 0..30 {
            moved |= scene.scroll(1.0 / 30.0, 1.0);
        }
        assert!(moved, "scrolls once complete");
        assert!(scene.endless_camera().unwrap() > 1.9);
    }

    #[test]
    fn scrolling_frame_is_sky_and_the_street_goes_to_a_transparent_overlay() {
        let fixed = CellScaling::Fixed(3);
        let scene = Scene::new(23, 960, 540, classic(), 1.0, fixed, Some(0.0));
        assert!(scene.is_scrolling());
        // The layers are drawn by the GPU, so there is no still city.
        assert!(scene.city.is_empty());
        let grid = scene.grid;
        let span = scene.street_span;
        assert_eq!(
            span.origin_x, -STREET_MARGIN,
            "a strip wider than the screen"
        );
        assert_eq!(span.width, grid.cols + 2 * STREET_MARGIN);
        assert_eq!(scene.overlay.len(), span.cells() * 4);
        let alpha = |x: i32, row: i32| scene.overlay[span.index(x, row).unwrap() * 4 + 3];
        for x in [-STREET_MARGIN, 0, grid.cols / 2, grid.cols + 1] {
            assert_eq!(alpha(x, grid.ground_row), 255, "top curb");
            assert_eq!(alpha(x, grid.rows - 1), 255, "bottom curb");
            assert_eq!(alpha(x, 0), 0, "sky stays clear");
        }
        // Opaque overlay cells carry exactly the Normal-style street colors.
        let mut expected = vec![0; span.cells() * 4];
        paint_street(
            &scene.street,
            scene.traffic.as_ref(),
            grid,
            span,
            classic(),
            &mut expected,
        );
        for (cell, pixel) in scene.overlay.chunks(4).enumerate() {
            if pixel[3] == 255 {
                assert_eq!(pixel[..3], expected[cell * 4..cell * 4 + 3]);
            }
        }
        // Normal mode draws its still city and the same overlay.
        let normal = Scene::new(23, 960, 540, classic(), 1.0, fixed, None);
        assert!(!normal.city.is_empty());
        assert_eq!(normal.overlay.len(), normal.sky.len());
    }

    #[test]
    fn the_street_and_traffic_follow_the_road_camera() {
        let fixed = CellScaling::Fixed(3);
        let mut scene = Scene::new(24, 960, 540, classic(), 1.0, fixed, Some(0.0));
        // 30 s at 1x: the road camera (nearest layer) moves 60 cells.
        for _ in 0..(30 * 30) {
            scene.step_traffic(1.0 / 30.0, 1.0);
            scene.scroll(1.0 / 30.0, 1.0);
        }
        scene.compose_overlay();
        let view = scene.road_view();
        assert!((view - 60.0).abs() < 1e-6, "{view}");
        assert_eq!(scene.road_view(), scene.endless.as_ref().unwrap().view_x(3));
        let span = scene.street_span;
        assert_eq!(span.origin_x, view.floor() as i32 - STREET_MARGIN);
        // The fractional offset lies inside the margin, so the strip always
        // covers the screen.
        let offset = scene.street_offset_px();
        let cell = f64::from(scene.grid.cell);
        assert!(offset >= f64::from(STREET_MARGIN) * cell - 1e-9);
        assert!(offset < f64::from(STREET_MARGIN + 1) * cell);
        // Dashes are placed by world column, so the road scrolls with the
        // world rather than staying put on screen.
        let dash_row = scene.grid.ground_row + 3;
        for x in span.origin_x..span.origin_x + span.width {
            let cell = span.index(x, dash_row).unwrap();
            let color = &scene.overlay[cell * 4..cell * 4 + 3];
            let has_car = scene.traffic.as_ref().unwrap().vehicles.iter().any(|v| {
                let left = v.x.round() as i32 - 5;
                (left..left + v.length + 10).contains(&x)
            });
            let has_tree = scene
                .street
                .front
                .iter()
                .chain(&scene.street.back_trees)
                .any(|&(tree_cell, _)| tree_cell == cell);
            if !has_car && !has_tree {
                let dash = starry_journey::street::road_color(3, x, classic());
                assert_eq!(color, &dash.0[..], "x {x}");
            }
        }
        // Traffic uses the same view, and vehicles stay near the screen.
        let traffic = scene.traffic.as_ref().unwrap();
        assert_eq!(traffic.view_x(), view);
        let cols = f64::from(scene.grid.cols);
        for vehicle in &traffic.vehicles {
            assert!(
                vehicle.x > view - cols && vehicle.x < view + 2.0 * cols,
                "{}",
                vehicle.x
            );
        }
        // Normal mode keeps the street on the screen with no offset.
        let normal = Scene::new(24, 960, 540, classic(), 1.0, fixed, None);
        assert_eq!(normal.street_span, Span::screen(&normal.grid));
        assert_eq!(normal.street_offset_px(), 0.0);
    }

    #[test]
    fn endless_resize_keeps_the_camera_and_normal_has_no_layers() {
        let fixed = CellScaling::Fixed(3);
        let mut scene = Scene::new(22, 960, 540, classic(), 1.0, fixed, Some(0.0));
        for _ in 0..90 {
            scene.scroll(1.0 / 30.0, 4.0);
        }
        let camera = scene.endless_camera().unwrap();
        let resized = Scene::new(22, 1280, 720, classic(), 1.0, fixed, Some(camera));
        assert_eq!(resized.endless_camera(), Some(camera));
        assert!(resized.is_scrolling());
        let normal = Scene::new(22, 960, 540, classic(), 1.0, fixed, None);
        assert!(!normal.is_scrolling() && normal.endless_camera().is_none());
    }

    #[test]
    fn random_seeds_differ_over_time() {
        let a = random_seed();
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert_ne!(a, random_seed());
    }
}
