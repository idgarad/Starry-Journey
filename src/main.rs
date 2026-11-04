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
use starry_journey::model::palette::Rgb;
use starry_journey::model::palette::{palette_by_key, palettes, Palette, DEFAULT_PALETTE_KEY};
use starry_journey::moon::{is_station, moon_at, sky_cells, MoonState};
use starry_journey::quips::quip_at;
use starry_journey::raster::{generate_stars, CityRaster, RasterOptions, Star, WindowLighting};
use starry_journey::reveal::{light_revealed, ConstructionView, Reveal};
use starry_journey::station::StationFate;
use starry_journey::street::{apply_paint, mark_opaque, CellPaint, StreetLayer};
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
    /// The image to present: the city view with the street drawn over it.
    frame: Vec<u8>,
    /// Moon cells to draw on sky (behind the city and street).
    moon: Vec<(usize, Rgb)>,
    /// Street traffic; starts once the city is finished (Starry Journey
    /// extension: Normal mode has traffic too).
    traffic: Option<Traffic>,
    /// The palette the scene is drawn in (palettes are static data).
    palette: &'static Palette,
    /// The scrolling layers in Endless mode; `None` in Normal mode. They
    /// start moving once construction is complete.
    endless: Option<EndlessCity>,
    /// Background and stars only, the base the Endless layers draw over.
    sky: Vec<u8>,
    /// While Endless scrolls: the street, trees, and traffic, drawn over
    /// the layers by the GPU (transparent elsewhere). Empty otherwise.
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
            frame: Vec::new(),
            moon: Vec::new(),
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
        match &mut self.endless {
            Some(city) => city.advance(dt, speed, self.palette),
            None => false,
        }
    }

    /// Rebuilds the cell-resolution images.
    ///
    /// Normal mode and construction: `frame` is the city view, then the moon
    /// on cells that show sky, then the street's back layer (road and
    /// background trees), traffic, and its front layer.
    ///
    /// Endless scrolling: `frame` is only the sky, stars, and moon; the GPU
    /// draws the layers over it at fractional positions (buildings hide the
    /// moon there), and the street and traffic go to `overlay`, drawn on top.
    ///
    /// O(grid cells + street + moon cells).
    fn compose(&mut self) {
        let scrolling = self.is_scrolling();
        if scrolling {
            self.frame.clone_from(&self.sky);
        } else {
            self.frame.clone_from(&self.view.rgba);
        }
        for &(cell, color) in &self.moon {
            let building_shown =
                !scrolling && self.ownership.owner[cell] >= 0 && self.reveal.is_revealed(cell);
            if !building_shown {
                self.frame[cell * 4..cell * 4 + 3].copy_from_slice(&color.0);
            }
        }
        if scrolling {
            self.overlay.clear();
            self.overlay.resize(self.frame.len(), 0);
            let traffic_paint = paint_street(
                &self.street,
                self.traffic.as_ref(),
                self.grid,
                self.palette,
                &mut self.overlay,
            );
            self.street.mark_opaque(&mut self.overlay);
            mark_opaque(&traffic_paint, &mut self.overlay);
        } else {
            self.overlay.clear();
            paint_street(
                &self.street,
                self.traffic.as_ref(),
                self.grid,
                self.palette,
                &mut self.frame,
            );
        }
    }

    /// Starts traffic once the city is complete (cars then drive in from
    /// off-screen).
    fn start_traffic_if_complete(&mut self) {
        if self.traffic.is_none() && self.reveal.is_complete() {
            self.traffic = Some(Traffic::new(self.seed, self.grid.cols));
        }
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
        self.street = StreetLayer::new(&self.grid, self.seed, palette);
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
    city_texture: wgpu::Texture,
    /// The message box image (premultiplied sRGB; 1 x 1 when hidden).
    message_texture: wgpu::Texture,
    /// Endless street and traffic, drawn over the layers (grid-sized).
    overlay_texture: wgpu::Texture,
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
        // Binding 1 is the uniform buffer; 0 and 2..=7 are textures (see
        // present.wgsl).
        let entries: Vec<wgpu::BindGroupLayoutEntry> = (0..8)
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
        let layer_textures = [0, 1, 2, 3].map(|_| create_layer_texture(&device, 1, 1));
        let bind_group = create_bind_group(
            &device,
            &bind_group_layout,
            &BindTextures {
                background: &city_texture,
                message: &message_texture,
                overlay: &overlay_texture,
                layers: layer_textures.each_ref(),
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
        let moon_moved = self.update_moon(now);
        let motion_dt = if self.paused { 0.0 } else { dt };
        let traffic_moved = self.scene.step_traffic(motion_dt, self.speed);
        // The GPU places the layers, so scrolling needs only new offsets
        // (and an upload when a layer buffer was rebased).
        self.scene.scroll(motion_dt, self.speed);
        if self.scene.view.take_dirty() || moon_moved || traffic_moved || finished {
            self.scene.compose();
            self.upload();
        }
        if self.scene.is_scrolling() {
            self.upload_layers();
            self.write_params();
        }
        self.update_status(elapsed);
        self.update_message();
    }

    /// Shows the silly startup lines while an Endless city is being built,
    /// and clears them when it is done.
    fn update_status(&mut self, construction_seconds: f64) {
        let building = self.scene.endless.is_some() && !self.scene.reveal.is_complete();
        let status = building.then(|| quip_at(self.scene.seed, construction_seconds));
        self.notice.set_status(status);
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

    /// Writes the cell size, the Endless switch and layer offsets, and the
    /// message box rectangle for the shader.
    fn write_params(&self) {
        let cell = self.scene.grid.cell as f32;
        let [x, y, width, height] = self.message_rect;
        let scrolling = self.scene.is_scrolling();
        let offset = |layer: usize| match &self.scene.endless {
            Some(city) if scrolling => city.source_offset_px(layer) as f32,
            _ => 0.0,
        };
        let layers_on = if scrolling { 1.0 } else { 0.0 };
        let params: [f32; 24] = [
            cell,
            layers_on,
            0.0,
            0.0,
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
        ];
        let bytes: Vec<u8> = params
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        self.queue.write_buffer(&self.params, 0, &bytes);
    }

    /// Recomputes the moon if its position or (visibly) its phase changed.
    /// Returns true when the frame needs composing again.
    fn update_moon(&mut self, now: Instant) -> bool {
        let seconds = now.duration_since(self.app_start).as_secs_f64();
        let moon = moon_at(&self.scene.grid, seconds, self.moon_start_step);
        // Phase changes are only visible in small steps; 1/400 of a pass
        // (0.75 s) is finer than any single cell's change.
        let quantized = |m: &MoonState| (m.x, m.row, (m.phase * 400.0) as i32);
        // The station's battle animates every frame; a plain moon only
        // changes when it moves a cell.
        let animated = is_station(&moon);
        if !animated && self.moon_state.as_ref().map(quantized) == Some(quantized(&moon)) {
            return false;
        }
        self.moon_state = Some(moon);
        self.scene.moon = sky_cells(
            &self.scene.grid,
            &moon,
            palette_at(self.palette_index),
            self.station_fate,
        );
        true
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

    /// Uploads the background image (and, while Endless scrolls, the
    /// overlay and any changed layer), recreating textures only when the
    /// grid dimensions change.
    fn upload(&mut self) {
        let width = self.scene.grid.cols.max(1) as u32;
        let height = self.scene.grid.rows.max(1) as u32;
        let size = self.city_texture.size();
        if size.width != width || size.height != height {
            self.city_texture = create_city_texture(&self.device, &self.scene.grid);
            self.overlay_texture = create_city_texture(&self.device, &self.scene.grid);
            self.rebind();
        }
        let layout = wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        };
        self.queue.write_texture(
            self.city_texture.as_image_copy(),
            &self.scene.frame,
            layout,
            self.city_texture.size(),
        );
        if !self.scene.overlay.is_empty() {
            self.queue.write_texture(
                self.overlay_texture.as_image_copy(),
                &self.scene.overlay,
                layout,
                self.overlay_texture.size(),
            );
        }
        self.upload_layers();
        self.write_params();
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
/// bottom lane. Returns the traffic's cells.
fn paint_street(
    street: &StreetLayer,
    traffic: Option<&Traffic>,
    grid: Grid,
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
            street.apply_back_lit(rgba, grid.cols, light, back_glow);
            traffic.paint(&grid, palette, &mut paint);
            apply_paint(&paint, rgba);
            street.apply_front_lit(rgba, grid.cols, light, front_glow);
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
const PARAMS_BYTES: u64 = 6 * 16;

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
    use starry_journey::moon::moon_cells;

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
        let pixel = |x: i32, row: i32| {
            let i = (row * grid.cols + x) as usize * 4;
            [scene.frame[i], scene.frame[i + 1], scene.frame[i + 2]]
        };
        assert_eq!(pixel(1, grid.ground_row), classic().lane.0, "top curb");
        assert_eq!(pixel(1, grid.rows - 1), classic().lane.0, "bottom curb");
        let road_cells = (0..grid.cols)
            .filter(|&x| pixel(x, grid.ground_row + 2) == classic().road.0)
            .count();
        assert!(road_cells > grid.cols as usize / 2);
        assert_ne!(
            scene.frame, scene.view.rgba,
            "street is drawn over the empty sky"
        );
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
    fn moon_is_drawn_on_sky_but_hidden_behind_revealed_buildings() {
        let mut scene = Scene::new(17, 1920, 1080, classic(), 1.0, CellScaling::Fixed(3), None);
        let grid = scene.grid;
        // A full moon low over the city, partly behind the skyline: search
        // upward from the road for a height where it overlaps both open sky
        // and buildings (the moon lives on the sky layer).
        let x = grid.cols / 2;
        let row = (0..grid.ground_row)
            .rev()
            .find(|&row| {
                let probe = MoonState {
                    x,
                    row,
                    phase: 0.5,
                    ..Default::default()
                };
                let cells = moon_cells(&grid, &probe, classic());
                let owned = cells
                    .iter()
                    .filter(|&&(cell, _)| scene.ownership.owner[cell] >= 0)
                    .count();
                owned > 0 && owned < cells.len() / 2
            })
            .expect("the skyline crosses the moon's column");
        let moon = MoonState {
            x,
            row,
            phase: 0.5,
            ..Default::default()
        };
        scene.moon = moon_cells(&grid, &moon, classic());
        scene.compose();
        let mut on_sky = 0;
        let mut behind = 0;
        for &(cell, color) in &scene.moon.clone() {
            let shown = &scene.frame[cell * 4..cell * 4 + 3];
            let row = cell as i32 / grid.cols;
            if row >= grid.ground_row {
                continue; // the street covers the moon at the horizon
            }
            if scene.ownership.owner[cell] >= 0 {
                behind += 1;
                assert_ne!(shown, &color.0[..], "a building hides the moon");
            } else {
                on_sky += 1;
                assert_eq!(shown, &color.0[..], "the moon shows on open sky");
            }
        }
        assert!(on_sky > 0 && behind > 0, "sky {on_sky}, behind {behind}");
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
            .frame
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
        let shown = &lit_scene.frame[cell * 4..cell * 4 + 3];
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
        // The layers are drawn by the GPU, so the background is only sky.
        assert_eq!(scene.frame, scene.sky);
        let grid = scene.grid;
        let alpha = |x: i32, row: i32| scene.overlay[(row * grid.cols + x) as usize * 4 + 3];
        for x in [0, grid.cols / 2, grid.cols - 1] {
            assert_eq!(alpha(x, grid.ground_row), 255, "top curb");
            assert_eq!(alpha(x, grid.rows - 1), 255, "bottom curb");
            assert_eq!(alpha(x, 0), 0, "sky stays clear");
        }
        // Opaque overlay cells carry exactly the Normal-style street colors.
        let mut expected = scene.sky.clone();
        paint_street(
            &scene.street,
            scene.traffic.as_ref(),
            grid,
            classic(),
            &mut expected,
        );
        for (cell, pixel) in scene.overlay.chunks(4).enumerate() {
            if pixel[3] == 255 {
                assert_eq!(pixel[..3], expected[cell * 4..cell * 4 + 3]);
            }
        }
        // Normal mode keeps everything in one frame and no overlay.
        let normal = Scene::new(23, 960, 540, classic(), 1.0, fixed, None);
        assert!(normal.overlay.is_empty());
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
