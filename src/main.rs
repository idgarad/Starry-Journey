use std::{error::Error, sync::Arc};

use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Fullscreen, Window, WindowId},
};

const VIRTUAL_WIDTH: u32 = 320;
const VIRTUAL_HEIGHT: u32 = 180;

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    fullscreen: bool,
    monitor_index: usize,
}

impl Default for App {
    fn default() -> Self {
        Self {
            window: None,
            renderer: None,
            fullscreen: false,
            monitor_index: 0,
        }
    }
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
            .with_title("Starry Journey — Sky 320 × 180")
            .with_inner_size(PhysicalSize::new(1280, 720))
            .with_min_inner_size(PhysicalSize::new(320, 180));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("Could not create the application window: {error}");
                event_loop.exit();
                return;
            }
        };

        match pollster::block_on(Renderer::new(Arc::clone(&window))) {
            Ok(renderer) => {
                self.window = Some(window);
                self.renderer = Some(renderer);
            }
            Err(error) => {
                eprintln!("Could not initialize the sky renderer: {error}");
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
                            eprintln!("Could not render the sky: {error}");
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

struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: wgpu::SurfaceConfiguration,
    sky_texture: wgpu::Texture,
    sky_pipeline: wgpu::RenderPipeline,
    present_pipeline: wgpu::RenderPipeline,
    present_bind_group: wgpu::BindGroup,
}

impl Renderer {
    async fn new(window: Arc<Window>) -> Result<Self, Box<dyn Error>> {
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

        let sky_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("320 × 180 virtual sky"),
            size: wgpu::Extent3d {
                width: VIRTUAL_WIDTH,
                height: VIRTUAL_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let sky_view = sky_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sky_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Sky layer shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sky.wgsl").into()),
        });
        let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Virtual sky pipeline"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &sky_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &sky_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
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

        let present_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Virtual pixel presentation shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("present.wgsl").into()),
        });
        let present_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Nearest-neighbor virtual pixel sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let present_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Virtual sky texture bindings"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let present_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Virtual sky texture"),
            layout: &present_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&sky_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&present_sampler),
                },
            ],
        });
        let present_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Virtual pixel presentation pipeline layout"),
                bind_group_layouts: &[&present_bind_group_layout],
                push_constant_ranges: &[],
            });
        let present_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Nearest-neighbor presentation pipeline"),
            layout: Some(&present_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &present_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &present_shader,
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

        Ok(Self {
            surface,
            device,
            queue,
            surface_config,
            sky_texture,
            sky_pipeline,
            present_pipeline,
            present_bind_group,
        })
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.surface_config.width = size.width;
        self.surface_config.height = size.height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let frame = self.surface.get_current_texture()?;
        let frame_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let sky_view = self
            .sky_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Sky layer command encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render virtual sky pixels"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &sky_view,
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
            pass.set_pipeline(&self.sky_pipeline);
            pass.draw(0..3, 0..1);
        }

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Present virtual sky pixels"),
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
            let scale = (self.surface_config.width as f32 / VIRTUAL_WIDTH as f32)
                .min(self.surface_config.height as f32 / VIRTUAL_HEIGHT as f32);
            let width = VIRTUAL_WIDTH as f32 * scale;
            let height = VIRTUAL_HEIGHT as f32 * scale;
            let x = (self.surface_config.width as f32 - width) * 0.5;
            let y = (self.surface_config.height as f32 - height) * 0.5;
            pass.set_viewport(x, y, width, height, 0.0, 1.0);
            pass.set_pipeline(&self.present_pipeline);
            pass.set_bind_group(0, &self.present_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }
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
    fn virtual_canvas_is_320_by_180() {
        assert_eq!((VIRTUAL_WIDTH, VIRTUAL_HEIGHT), (320, 180));
        assert_eq!(VIRTUAL_WIDTH * 4, 1280);
        assert_eq!(VIRTUAL_HEIGHT * 4, 720);
    }

    #[test]
    fn sky_and_presentation_shaders_pass_wgsl_validation() {
        for source in [include_str!("sky.wgsl"), include_str!("present.wgsl")] {
            let module = naga::front::wgsl::parse_str(source).expect("shader should parse");
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .expect("shader should validate");
        }
    }
}
