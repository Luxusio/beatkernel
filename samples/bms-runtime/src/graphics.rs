//! GPU rectangle batches; no native window, input, audio, or transport ownership.

use std::str::FromStr;
use std::sync::{Arc, Mutex};

use crate::scene::{Rectangle, Scene, MAX_RECTANGLES};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendChoice {
    Auto,
    Vulkan,
    Dx12,
    Metal,
    Gl,
}

impl FromStr for BackendChoice {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto" => Ok(Self::Auto),
            "vulkan" => Ok(Self::Vulkan),
            "dx12" => Ok(Self::Dx12),
            "metal" => Ok(Self::Metal),
            "gl" => Ok(Self::Gl),
            _ => Err("graphics backend must be auto, vulkan, dx12, metal, or gl".into()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presentation {
    Fifo,
    Immediate,
    Mailbox,
}

impl FromStr for Presentation {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "fifo" => Ok(Self::Fifo),
            "immediate" => Ok(Self::Immediate),
            "mailbox" => Ok(Self::Mailbox),
            _ => Err("presentation must be fifo, immediate, or mailbox".into()),
        }
    }
}

impl Presentation {
    fn mode(self) -> wgpu::PresentMode {
        match self {
            Self::Fifo => wgpu::PresentMode::Fifo,
            Self::Immediate => wgpu::PresentMode::Immediate,
            Self::Mailbox => wgpu::PresentMode::Mailbox,
        }
    }
}

/// Explicit backend discovery without window, audio or gameplay ownership.
pub fn instance_descriptor(backend: BackendChoice) -> Result<wgpu::InstanceDescriptor, String> {
    let requested = match backend {
        BackendChoice::Auto => wgpu::Backends::all(),
        BackendChoice::Vulkan => wgpu::Backends::VULKAN,
        BackendChoice::Dx12 => wgpu::Backends::DX12,
        BackendChoice::Metal => wgpu::Backends::METAL,
        BackendChoice::Gl => wgpu::Backends::GL,
    };
    let enabled = requested & wgpu::Instance::enabled_backend_features();
    if enabled.is_empty() {
        return Err(format!(
            "graphics backend {backend:?} is unavailable in this build/platform"
        ));
    }
    let mut descriptor = wgpu::InstanceDescriptor::default();
    descriptor.backends = enabled;
    Ok(descriptor)
}

pub fn instance(backend: BackendChoice) -> Result<wgpu::Instance, String> {
    Ok(wgpu::Instance::new(&instance_descriptor(backend)?))
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    instances: wgpu::Buffer,
    viewport: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    suspended: bool,
    recreate_surface: bool,
    failure: Arc<Mutex<Option<String>>>,
}

impl Renderer {
    pub async fn new(
        surface: wgpu::Surface<'static>,
        instance: &wgpu::Instance,
        presentation: Presentation,
    ) -> Result<Self, String> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|error| format!("request graphics adapter: {error}"))?;
        let capabilities = surface.get_capabilities(&adapter);
        let present_mode = presentation.mode();
        if !capabilities.present_modes.contains(&present_mode) {
            return Err(format!(
                "presentation {presentation:?} unavailable; supported: {:?}",
                capabilities.present_modes
            ));
        }
        // Prefer a linear UNORM surface so existing byte RGB glyph/note colors
        // retain their values instead of applying an extra sRGB conversion.
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .or_else(|| capabilities.formats.first().copied())
            .ok_or("graphics surface exposes no formats")?;
        let alpha_mode = capabilities
            .alpha_modes
            .first()
            .copied()
            .ok_or("graphics surface exposes no alpha modes")?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("BeatKernel graphics"),
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await
            .map_err(|error| format!("request graphics device: {error}"))?;
        let failure = Arc::new(Mutex::new(None));
        let errors = Arc::clone(&failure);
        device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
            if let Ok(mut failure) = errors.lock() {
                if failure.is_none() {
                    *failure = Some(format!("graphics device error: {error}"));
                }
            }
        }));
        let lost = Arc::clone(&failure);
        device.set_device_lost_callback(move |reason, message| {
            if let Ok(mut failure) = lost.lock() {
                if failure.is_none() {
                    *failure = Some(format!("graphics device lost ({reason:?}): {message}"));
                }
            }
        });
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: 1,
            height: 1,
            desired_maximum_frame_latency: 1,
            present_mode,
            alpha_mode,
            view_formats: vec![],
        };
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("BeatKernel rectangles"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/rectangles.wgsl").into()),
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("BeatKernel ordered rectangles"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Rectangle>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attributes,
                }],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("BeatKernel reusable rectangle instances"),
            size: (MAX_RECTANGLES * std::mem::size_of::<Rectangle>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let viewport = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("BeatKernel logical viewport"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("BeatKernel viewport"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport.as_entire_binding(),
            }],
        });
        let renderer = Self {
            surface,
            adapter,
            device,
            queue,
            config,
            pipeline,
            instances,
            viewport,
            bind_group,
            suspended: true,
            recreate_surface: false,
            failure,
        };
        renderer.check_failure()?;
        Ok(renderer)
    }

    fn check_failure(&self) -> Result<(), String> {
        let failure = self
            .failure
            .lock()
            .map_err(|_| "graphics error state poisoned")?;
        match failure.as_ref() {
            Some(message) => Err(message.clone()),
            None => Ok(()),
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.check_failure()?;
        if width == 0 || height == 0 {
            self.suspended = true;
            return Ok(());
        }
        let limit = self.device.limits().max_texture_dimension_2d;
        if width > limit || height > limit {
            return Err(format!(
                "surface extent {width}x{height} exceeds device limit {limit}"
            ));
        }
        self.config.width = width;
        self.config.height = height;
        if !self.recreate_surface {
            self.surface.configure(&self.device, &self.config);
        }
        self.suspended = false;
        self.check_failure()
    }

    pub fn needs_surface_recreation(&self) -> bool {
        self.recreate_surface
    }

    /// Window/canvas ownership stays outside this renderer. The owner recreates
    /// a lost surface using the same instance and hands it back here.
    pub fn replace_surface(&mut self, surface: wgpu::Surface<'static>) -> Result<(), String> {
        self.check_failure()?;
        let caps = surface.get_capabilities(&self.adapter);
        if !caps.formats.contains(&self.config.format)
            || !caps.present_modes.contains(&self.config.present_mode)
            || !caps.alpha_modes.contains(&self.config.alpha_mode)
        {
            return Err("replacement surface requires graphics device/pipeline recreation".into());
        }
        self.surface = surface;
        self.recreate_surface = false;
        if !self.suspended {
            self.surface.configure(&self.device, &self.config);
        }
        self.check_failure()
    }

    /// Logical geometry stretches to the full physical surface. Upload only
    /// instance data, then draw the entire painter-ordered batch in one call.
    pub fn render(&mut self, scene: &Scene) -> Result<(), String> {
        self.check_failure()?;
        scene.status()?;
        if self.suspended || self.recreate_surface {
            return Ok(());
        }
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Timeout) => return Ok(()),
            Err(wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return self.check_failure();
            }
            Err(wgpu::SurfaceError::Lost) => {
                self.recreate_surface = true;
                return Ok(());
            }
            Err(error) => return Err(format!("graphics surface acquisition failed: {error}")),
        };
        let suboptimal = frame.suboptimal;
        let dimensions = scene.dimensions();
        self.queue.write_buffer(
            &self.viewport,
            0,
            bytemuck::cast_slice(&[dimensions[0], dimensions[1], 0.0, 0.0]),
        );
        let rectangles = scene.rectangles();
        if !rectangles.is_empty() {
            self.queue
                .write_buffer(&self.instances, 0, bytemuck::cast_slice(rectangles));
        }
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("BeatKernel frame"),
            });
        {
            let attachments = [Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 16.0 / 255.0,
                        g: 21.0 / 255.0,
                        b: 30.0 / 255.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("BeatKernel scene"),
                color_attachments: &attachments,
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.instances.slice(..));
            pass.draw(0..6, 0..rectangles.len() as u32);
        }
        self.queue.submit([encoder.finish()]);
        frame.present();
        if suboptimal {
            self.surface.configure(&self.device, &self.config);
        }
        self.check_failure()
    }

    pub fn description(&self) -> String {
        let info = self.adapter.get_info();
        format!(
            "{} / {:?} / {:?}",
            info.name, info.backend, self.config.present_mode
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_choices_reject_unknown_values() {
        assert_eq!(
            "vulkan".parse::<BackendChoice>().unwrap(),
            BackendChoice::Vulkan
        );
        assert_eq!(
            "mailbox".parse::<Presentation>().unwrap(),
            Presentation::Mailbox
        );
        assert!("directx".parse::<BackendChoice>().is_err());
        assert!("auto".parse::<Presentation>().is_err());
    }
}
