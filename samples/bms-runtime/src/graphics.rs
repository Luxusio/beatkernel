//! GPU rectangle batches; no native window, input, audio, or transport ownership.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::playfield_gpu::{MAX_NOTE_INSTANCES, MAX_PLAYFIELDS, NoteInstance};
use crate::scene::{MAX_RECTANGLES, Rectangle, Scene};
use crate::texture::{MAX_TEXTURE_BYTES, MAX_TEXTURES, RgbaImage, TextureId};

pub use crate::presentation_settings::{BackendChoice, Presentation};

impl Presentation {
    fn mode(self) -> wgpu::PresentMode {
        match self {
            Self::Fifo => wgpu::PresentMode::Fifo,
            Self::Immediate => wgpu::PresentMode::Immediate,
            Self::Mailbox => wgpu::PresentMode::Mailbox,
        }
    }
}

fn supported_presentation(
    presentation: Presentation,
    modes: &[wgpu::PresentMode],
) -> Result<wgpu::PresentMode, String> {
    let requested = presentation.mode();
    if modes.contains(&requested) {
        Ok(requested)
    } else {
        Err(format!(
            "presentation {presentation:?} unavailable; supported: {modes:?}"
        ))
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
    note_pipeline: wgpu::RenderPipeline,
    note_layers: Vec<NoteLayer>,
    instances: wgpu::Buffer,
    uploaded_geometry: Option<(Arc<()>, u64)>,
    redraw_pending: bool,
    viewport: wgpu::Buffer,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    textures: BTreeMap<TextureId, TextureResource>,
    texture_bytes: u64,
    suspended: bool,
    recreate_surface: bool,
    failure: Arc<Mutex<Option<String>>>,
}

struct TextureResource {
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    bytes: u64,
}

struct NoteLayer {
    instances: wgpu::Buffer,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    uploaded: Option<Arc<[NoteInstance]>>,
}

fn admit_texture(count: usize, used_bytes: u64, image_bytes: u64) -> Result<u64, String> {
    if count >= MAX_TEXTURES {
        return Err(format!("renderer exceeds {MAX_TEXTURES} textures"));
    }
    let total = used_bytes
        .checked_add(image_bytes)
        .ok_or("texture byte budget overflow")?;
    if total > MAX_TEXTURE_BYTES {
        return Err(format!(
            "renderer texture byte budget exceeds {MAX_TEXTURE_BYTES}"
        ));
    }
    Ok(total)
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
        let present_mode = supported_presentation(presentation, &capabilities.present_modes)?;
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
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("BeatKernel viewport and texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(16),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("BeatKernel texture pipeline layout"),
            bind_group_layouts: &[&texture_layout],
            push_constant_ranges: &[],
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("BeatKernel ordered rectangles"),
            layout: Some(&pipeline_layout),
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
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
        let note_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("BeatKernel local epoch playfield"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/playfield.wgsl").into()),
        });
        let note_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("BeatKernel playfield uniform"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(32),
                },
                count: None,
            }],
        });
        let note_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("BeatKernel playfield pipeline layout"),
            bind_group_layouts: &[&note_layout],
            push_constant_ranges: &[],
        });
        let note_attributes = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4];
        let note_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("BeatKernel retained note instances"),
            layout: Some(&note_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &note_shader,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<NoteInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &note_attributes,
                }],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &note_shader,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        let note_layers = (0..MAX_PLAYFIELDS)
            .map(|_| {
                let instances = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("BeatKernel retained note buffer"),
                    size: (MAX_NOTE_INSTANCES * std::mem::size_of::<NoteInstance>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let uniform = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("BeatKernel playfield drift"),
                    size: 32,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("BeatKernel playfield drift binding"),
                    layout: &note_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    }],
                });
                NoteLayer {
                    instances,
                    uniform,
                    bind_group,
                    uploaded: None,
                }
            })
            .collect();
        let viewport = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("BeatKernel logical viewport"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("BeatKernel nearest sprite sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let mut renderer = Self {
            surface,
            adapter,
            device,
            queue,
            config,
            pipeline,
            note_pipeline,
            note_layers,
            instances,
            uploaded_geometry: None,
            redraw_pending: true,
            viewport,
            texture_layout,
            sampler,
            textures: BTreeMap::new(),
            texture_bytes: 0,
            suspended: true,
            recreate_surface: false,
            failure,
        };
        renderer.check_failure()?;
        renderer.install_texture(TextureId::WHITE, &RgbaImage::new(1, 1, vec![255; 4])?)?;
        renderer.install_texture(TextureId::FONT, &crate::font::atlas()?)?;
        Ok(renderer)
    }

    pub fn upload_texture(&mut self, image: &RgbaImage) -> Result<TextureId, String> {
        self.check_failure()?;
        self.validate_texture(image)?;
        let id = TextureId::allocate()?;
        self.install_texture(id, image)?;
        Ok(id)
    }

    fn validate_texture(&self, image: &RgbaImage) -> Result<u64, String> {
        let limit = self.device.limits().max_texture_dimension_2d;
        if image.width() > limit || image.height() > limit {
            return Err(format!(
                "texture extent {}x{} exceeds device limit {limit}",
                image.width(),
                image.height()
            ));
        }
        admit_texture(self.textures.len(), self.texture_bytes, image.byte_len())
    }

    fn install_texture(&mut self, id: TextureId, image: &RgbaImage) -> Result<(), String> {
        self.check_failure()?;
        let total = self.validate_texture(image)?;
        let bytes_per_row = image
            .width()
            .checked_mul(4)
            .ok_or("texture row byte overflow")?;
        let size = wgpu::Extent3d {
            width: image.width(),
            height: image.height(),
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("BeatKernel RGBA sprite"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            image.pixels(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(image.height()),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("BeatKernel sprite texture"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.viewport.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.check_failure()?;
        self.textures.insert(
            id,
            TextureResource {
                _texture: texture,
                bind_group,
                bytes: image.byte_len(),
            },
        );
        self.texture_bytes = total;
        Ok(())
    }

    pub fn remove_texture(&mut self, id: TextureId) -> Result<(), String> {
        self.check_failure()?;
        if id == TextureId::WHITE || id == TextureId::FONT {
            return Err("cannot remove built-in white/font textures".into());
        }
        let texture = self
            .textures
            .remove(&id)
            .ok_or("texture is stale or belongs to another renderer")?;
        self.texture_bytes -= texture.bytes;
        Ok(())
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

    /// Change supported presentation on the window owner, outside a play session.
    /// Unsupported modes leave configuration unchanged. Suspended/lost surfaces
    /// retain the request for the existing resize/recreation path.
    pub fn set_presentation(&mut self, presentation: Presentation) -> Result<(), String> {
        self.check_failure()?;
        let capabilities = self.surface.get_capabilities(&self.adapter);
        let mode = supported_presentation(presentation, &capabilities.present_modes)?;
        if self.config.present_mode == mode {
            return Ok(());
        }
        let mut requested = self.config.clone();
        requested.present_mode = mode;
        if !self.suspended && !self.recreate_surface {
            self.surface.configure(&self.device, &requested);
            self.check_failure()?;
        }
        self.config = requested;
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.check_failure()?;
        if width == 0 || height == 0 {
            self.suspended = true;
            self.redraw_pending = false;
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
        self.redraw_pending = true;
        self.check_failure()
    }

    /// A transient acquisition failure still needs a later draw. Event-driven
    /// hosts must retry at their normal cadence until a frame is presented.
    pub fn needs_redraw(&self) -> bool {
        self.redraw_pending
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
    /// instance data once, then draw contiguous texture batches in painter order.
    pub fn render(&mut self, scene: &Scene) -> Result<(), String> {
        self.check_failure()?;
        scene.status()?;
        for batch in scene.batches() {
            if !self.textures.contains_key(&batch.texture) {
                return Err(format!(
                    "scene references unavailable texture {:?}",
                    batch.texture
                ));
            }
        }
        self.redraw_pending = !self.suspended;
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
        let (identity, epoch) = scene.geometry_stamp();
        if !self
            .uploaded_geometry
            .as_ref()
            .is_some_and(|(old, old_epoch)| Arc::ptr_eq(old, identity) && *old_epoch == epoch)
        {
            if !rectangles.is_empty() {
                self.queue
                    .write_buffer(&self.instances, 0, bytemuck::cast_slice(rectangles));
            }
            self.uploaded_geometry = Some((Arc::clone(identity), epoch));
        }
        for (field, layer) in scene.playfields().iter().zip(&mut self.note_layers) {
            if !layer
                .uploaded
                .as_ref()
                .is_some_and(|old| Arc::ptr_eq(old, &field.instances))
            {
                if !field.instances.is_empty() {
                    self.queue.write_buffer(
                        &layer.instances,
                        0,
                        bytemuck::cast_slice(&field.instances),
                    );
                }
                layer.uploaded = Some(Arc::clone(&field.instances));
            }
            self.queue.write_buffer(
                &layer.uniform,
                0,
                bytemuck::cast_slice(&[
                    dimensions[0],
                    dimensions[1],
                    field.drift,
                    0.0,
                    field.top,
                    field.bottom,
                    0.0,
                    0.0,
                ]),
            );
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
            for batch in scene.batches() {
                if let Some(slot) = batch.playfield {
                    let layer = &self.note_layers[slot];
                    pass.set_pipeline(&self.note_pipeline);
                    pass.set_vertex_buffer(0, layer.instances.slice(..));
                    pass.set_bind_group(0, &layer.bind_group, &[]);
                    pass.draw(0..6, 0..scene.playfields()[slot].instances.len() as u32);
                } else {
                    let texture = &self.textures[&batch.texture];
                    pass.set_pipeline(&self.pipeline);
                    pass.set_vertex_buffer(0, self.instances.slice(..));
                    pass.set_bind_group(0, &texture.bind_group, &[]);
                    pass.draw(0..6, batch.first..batch.first + batch.count);
                }
            }
        }
        self.queue.submit([encoder.finish()]);
        frame.present();
        self.redraw_pending = false;
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
    fn presentation_admission_uses_exact_surface_capabilities() {
        let modes = [wgpu::PresentMode::Fifo, wgpu::PresentMode::Immediate];
        assert_eq!(
            supported_presentation(Presentation::Immediate, &modes).unwrap(),
            wgpu::PresentMode::Immediate
        );
        assert!(supported_presentation(Presentation::Mailbox, &modes).is_err());
        assert!(supported_presentation(Presentation::Fifo, &[]).is_err());
    }
    #[test]
    fn texture_admission_counts_builtins_and_checks_bytes() {
        assert_eq!(
            admit_texture(2, 4 + 128 * 64 * 4, 16).unwrap(),
            4 + 128 * 64 * 4 + 16
        );
        assert!(admit_texture(MAX_TEXTURES, 0, 4).is_err());
        assert!(admit_texture(2, MAX_TEXTURE_BYTES, 4).is_err());
        assert!(admit_texture(2, u64::MAX, 4).is_err());
        assert!(admit_texture(MAX_TEXTURES - 1, MAX_TEXTURE_BYTES - 4, 4).is_ok());
    }
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
