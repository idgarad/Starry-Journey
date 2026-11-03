//! Starry Journey application: window, input, and GPU presentation.
//!
//! The scene (city model, CPU rasterizer, and construction reveal) lives in
//! the library. This binary generates a Normal-mode city for the window's
//! grid, reveals it one logical cell at a time over 240 seconds at 1x,
//! uploads the shown image as a texture whenever it changes, and draws each
//! logical cell as a `cell x cell` block of device pixels.

use std::{
    error::Error,
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use starry_journey::clock::{format_remaining, ConstructionClock};
use starry_journey::config::{parse_args, LaunchOptions, USAGE};
use starry_journey::model::building::Building;
use starry_journey::model::city::generate_normal_city;
use starry_journey::model::grid::{CellScaling, Grid};
use starry_journey::model::palette::{palette_by_key, palettes, Palette, DEFAULT_PALETTE_KEY};
use starry_journey::raster::{generate_stars, CityRaster, Star, WindowLighting};
use starry_journey::reveal::{light_revealed, ConstructionView, Reveal};
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
            self.options,
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
/// raster, the construction reveal, and the image currently shown.
struct Scene {
    seed: u32,
    grid: Grid,
    buildings: Vec<Building>,
    stars: Vec<Star>,
    ownership: CityRaster,
    reveal: Reveal,
    view: ConstructionView,
}

impl Scene {
    /// Generates the city for a `width x height` surface and reveals it up
    /// to `progress` (0 for a new city; the old progress after a resize).
    fn new(
        seed: u32,
        width: u32,
        height: u32,
        palette: &Palette,
        progress: f64,
        scaling: CellScaling,
    ) -> Self {
        let grid = Grid::with_scaling(width, height, false, scaling);
        let mut buildings = generate_normal_city(seed, &grid);
        let stars = generate_stars(seed, &grid);
        let ownership = CityRaster::render(&grid, &buildings, &stars, palette);
        let mut reveal = Reveal::new(&ownership, seed);
        let cells = reveal.advance_to(progress).to_vec();
        light_revealed(&mut buildings, &ownership, &cells);
        let view = Self::view_for(&grid, &buildings, &stars, palette, &reveal);
        Self {
            seed,
            grid,
            buildings,
            stars,
            ownership,
            reveal,
            view,
        }
    }

    /// The shown image: the finished city where revealed, sky elsewhere.
    fn view_for(
        grid: &Grid,
        buildings: &[Building],
        stars: &[Star],
        palette: &Palette,
        reveal: &Reveal,
    ) -> ConstructionView {
        let built =
            CityRaster::render_with(grid, buildings, stars, palette, WindowLighting::Target);
        let sky = CityRaster::render(grid, &[], stars, palette);
        ConstructionView::new(built.rgba, sky.rgba, reveal)
    }

    /// Re-colors the scene for another palette without regenerating it.
    fn set_palette(&mut self, palette: &Palette) {
        self.view = Self::view_for(
            &self.grid,
            &self.buildings,
            &self.stars,
            palette,
            &self.reveal,
        );
    }

    /// Moves construction to `progress` (it never goes backwards).
    fn advance_to(&mut self, progress: f64) {
        if self.reveal.is_complete() {
            return;
        }
        let cells = self.reveal.advance_to(progress).to_vec();
        light_revealed(&mut self.buildings, &self.ownership, &cells);
        self.view.reveal_cells(&cells);
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
    bind_group: wgpu::BindGroup,
    scene: Scene,
    palette_index: usize,
    speed: f64,
    last_frame: Option<Instant>,
    /// How construction progress advances (untimed or `--timer`).
    clock: ConstructionClock,
    /// When construction (re)started, for the duration clock.
    construction_start: Instant,
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
                    required_limits: wgpu::Limits::default(),
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
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("City texture and cell size"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
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
            label: Some("Cell size"),
            size: 16,
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
        );
        let city_texture = create_city_texture(&device, &scene.grid);
        let bind_group = create_bind_group(&device, &bind_group_layout, &city_texture, &params);
        let mut renderer = Self {
            surface,
            device,
            queue,
            surface_config,
            pipeline,
            bind_group_layout,
            params,
            city_texture,
            bind_group,
            scene,
            palette_index,
            speed: 1.0,
            last_frame: None,
            clock: options.clock,
            construction_start: Instant::now(),
            options,
            scale_factor,
        };
        renderer.upload();
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
        let new_grid = Grid::with_scaling(size.width, size.height, false, self.scaling());
        if new_grid != self.scene.grid {
            // Same seed, same construction progress (Rebuild Prompt: resize
            // preserves progress).
            let progress = self.scene.reveal.progress();
            self.scene = Scene::new(
                self.scene.seed,
                size.width,
                size.height,
                palette_at(self.palette_index),
                progress,
                self.scaling(),
            );
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
        );
        // Restart restarts a duration timer (Rebuild Prompt); resizing keeps it.
        self.construction_start = Instant::now();
        self.upload();
    }

    /// Switches to the next palette without regenerating geometry or
    /// changing construction progress.
    fn next_palette(&mut self) {
        self.palette_index = (self.palette_index + 1) % palettes().len();
        self.scene.set_palette(palette_at(self.palette_index));
        self.upload();
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
        self.scene.advance_to(progress);
        if self.scene.view.take_dirty() {
            self.upload();
        }
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
        if self.scene.reveal.is_complete() {
            format!("{TITLE} - complete - {size}")
        } else {
            let percent = (self.scene.reveal.progress() * 100.0).floor();
            format!("{TITLE} - building {percent}% - {pace} - {size}")
        }
    }

    /// Uploads the shown image, recreating the texture only when the grid
    /// dimensions change.
    fn upload(&mut self) {
        let width = self.scene.grid.cols.max(1) as u32;
        let height = self.scene.grid.rows.max(1) as u32;
        let size = self.city_texture.size();
        if size.width != width || size.height != height {
            self.city_texture = create_city_texture(&self.device, &self.scene.grid);
            self.bind_group = create_bind_group(
                &self.device,
                &self.bind_group_layout,
                &self.city_texture,
                &self.params,
            );
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.city_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.scene.view.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            self.city_texture.size(),
        );
        let cell = self.scene.grid.cell as f32;
        let params: [f32; 4] = [cell, 0.0, 0.0, 0.0];
        let bytes: Vec<u8> = params
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        self.queue.write_buffer(&self.params, 0, &bytes);
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

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    texture: &wgpu::Texture,
    params: &wgpu::Buffer,
) -> wgpu::BindGroup {
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("City texture and cell size"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: params.as_entire_binding(),
            },
        ],
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
        let small = Scene::new(14, 1280, 720, classic(), 1.0, fixed);
        let large = Scene::new(14, 2560, 1440, classic(), 1.0, fixed);
        assert_eq!((small.grid.cols, small.grid.rows), (320, 180));
        assert_eq!((large.grid.cols, large.grid.rows), (640, 360));
        assert!(large.buildings.len() > small.buildings.len() * 3 / 2);
        let tallest = large
            .buildings
            .iter()
            .map(|b| b.roof_height())
            .max()
            .unwrap();
        assert!(tallest <= large.grid.max_h && large.grid.max_h == 196);
        assert!(large.reveal.is_complete());
    }

    #[test]
    fn duration_clock_finishes_the_scene_on_time_at_any_speed() {
        let clock = ConstructionClock::Duration { seconds: 10.0 };
        for speed in [0.25, 1.0, 4.0] {
            let mut scene = Scene::new(16, 1280, 720, classic(), 0.0, CellScaling::Fixed(3));
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
        let scene = Scene::new(11, 1280, 720, classic(), 0.0, CellScaling::Spec);
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
        let mut scene = Scene::new(12, 1280, 720, classic(), 0.0, CellScaling::Spec);
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
        let mut scene = Scene::new(13, 1280, 720, classic(), 0.0, CellScaling::Spec);
        scene.advance_to(150.0 / 240.0);
        let progress = scene.reveal.progress();
        let resized = Scene::new(13, 1920, 1080, classic(), progress, CellScaling::Spec);
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
    fn random_seeds_differ_over_time() {
        let a = random_seed();
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert_ne!(a, random_seed());
    }
}
