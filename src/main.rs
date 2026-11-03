//! Starry Journey application: window, input, and GPU presentation.
//!
//! The scene (city model plus CPU rasterizer) lives in the library. This
//! binary generates a Normal-mode city for the window's grid, uploads the
//! rasterized logical buffer as a texture, and draws each logical cell as a
//! `cell x cell` block of device pixels with nearest-neighbor lookup.

use std::{
    error::Error,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use starry_journey::model::building::Building;
use starry_journey::model::city::generate_normal_city;
use starry_journey::model::grid::Grid;
use starry_journey::model::palette::{palette_by_key, palettes, DEFAULT_PALETTE_KEY};
use starry_journey::raster::{generate_stars, CityRaster, Star};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Fullscreen, Window, WindowId},
};

/// Initial window size in physical pixels.
const INITIAL_SIZE: (u32, u32) = (1280, 720);
/// Smallest window size in physical pixels.
const MIN_SIZE: (u32, u32) = (320, 180);

#[derive(Default)]
struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    fullscreen: bool,
    monitor_index: usize,
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

        match pollster::block_on(Renderer::new(Arc::clone(&window), random_seed())) {
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
            WindowEvent::ScaleFactorChanged { .. } => {
                if let (Some(window), Some(renderer)) = (&self.window, &mut self.renderer) {
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
                        _ => {}
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let (Some(window), Some(renderer)) = (&self.window, &mut self.renderer) {
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

/// The generated scene for one grid: buildings and stars.
struct Scene {
    seed: u32,
    grid: Grid,
    buildings: Vec<Building>,
    stars: Vec<Star>,
}

impl Scene {
    /// Generates a fully built Normal-mode city. Windows are lit to their
    /// targets until the reveal feature animates construction.
    fn new(seed: u32, width: u32, height: u32) -> Self {
        let grid = Grid::new(width, height, false);
        let mut buildings = generate_normal_city(seed, &grid);
        for building in &mut buildings {
            building.windows.light_to_target();
        }
        let stars = generate_stars(seed, &grid);
        Self {
            seed,
            grid,
            buildings,
            stars,
        }
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
}

impl Renderer {
    async fn new(window: Arc<Window>, seed: u32) -> Result<Self, Box<dyn Error>> {
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

        let scene = Scene::new(seed, surface_config.width, surface_config.height);
        let city_texture = create_city_texture(&device, &scene.grid);
        let bind_group = create_bind_group(&device, &bind_group_layout, &city_texture, &params);
        let palette_index = palettes()
            .iter()
            .position(|palette| palette.key == DEFAULT_PALETTE_KEY)
            .unwrap_or(0);
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
        };
        renderer.upload();
        Ok(renderer)
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
        let new_grid = Grid::new(size.width, size.height, false);
        if new_grid != self.scene.grid {
            self.scene = Scene::new(self.scene.seed, size.width, size.height);
            self.upload();
        }
    }

    /// Generates a new city with a new seed.
    fn restart(&mut self, seed: u32) {
        self.scene = Scene::new(seed, self.surface_config.width, self.surface_config.height);
        self.upload();
    }

    /// Switches to the next palette without regenerating geometry.
    fn next_palette(&mut self) {
        self.palette_index = (self.palette_index + 1) % palettes().len();
        self.upload();
    }

    /// Rasterizes the scene and uploads it, recreating the texture only when
    /// the grid dimensions change.
    fn upload(&mut self) {
        let palette = palettes()
            .get(self.palette_index)
            .or_else(|| palette_by_key(DEFAULT_PALETTE_KEY))
            .expect("the default palette exists");
        let raster = CityRaster::render(
            &self.scene.grid,
            &self.scene.buildings,
            &self.scene.stars,
            palette,
        );
        let size = self.city_texture.size();
        if size.width as usize != raster.width || size.height as usize != raster.height {
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
            &raster.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(raster.width as u32 * 4),
                rows_per_image: Some(raster.height as u32),
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
    let event_loop = EventLoop::new()?;
    let mut app = App::default();
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn scene_covers_the_window_grid_and_is_fully_built() {
        let scene = Scene::new(11, 1280, 720);
        assert_eq!(
            (scene.grid.cols, scene.grid.rows, scene.grid.cell),
            (427, 240, 3)
        );
        assert!(!scene.buildings.is_empty());
        assert!(scene
            .buildings
            .iter()
            .all(|b| b.windows.lit == b.windows.target));
        assert!(!scene.stars.is_empty());
    }

    #[test]
    fn random_seeds_differ_over_time() {
        let a = random_seed();
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert_ne!(a, random_seed());
    }
}
