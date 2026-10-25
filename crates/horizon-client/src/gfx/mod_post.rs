//! A mod's post-processing pass (capability `render`, docs/mods.md): the mod's WGSL fragment shader, after
//! `mod_post.wgsl`, drawn over the world's own target (its colour and depth) into a picture of its own, which a
//! shader pack's post-processing or the copy onto the screen then takes in place of the world's.
//!
//! The source is compiled when its version changes; one that does not validate is logged and skipped until the
//! mod gives a new one.
use bytemuck::{Pod, Zeroable};

/// What a mod's pass is given before its own source.
pub const PRELUDE: &str = include_str!("mod_post.wgsl");

/// The `Frame` uniform of `mod_post.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Default, Pod, Zeroable)]
pub struct FrameUniform {
    pub view: [[f32; 4]; 4],
    pub projection: [[f32; 4]; 4],
    pub inverse_view: [[f32; 4]; 4],
    pub inverse_projection: [[f32; 4]; 4],
    pub camera_position: [f32; 4],
    pub resolution: [f32; 4],
    pub time: [f32; 4],
    pub sun: [f32; 4],
    pub sky_color: [f32; 4],
    pub fog_color: [f32; 4],
    pub view_params: [f32; 4],
    pub mod_params: [[f32; 4]; 4],
}

struct Output {
    size: [u32; 2],
    view: wgpu::TextureView,
}

pub struct ModPost {
    format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    linear: wgpu::Sampler,
    nearest: wgpu::Sampler,
    uniform: wgpu::Buffer,
    /// The source's version, and its pipeline (`None`: it did not compile).
    built: Option<(u64, Option<wgpu::RenderPipeline>)>,
    output: Option<Output>,
    started: std::time::Instant,
    frames: u32,
}

impl ModPost {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let fragment = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty,
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Horizon mod post bindings"),
            entries: &[
                fragment(
                    0,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                fragment(
                    1,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                fragment(
                    2,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
                fragment(
                    3,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                ),
                fragment(
                    4,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Horizon mod post"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let sampler = |label, filter| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                mag_filter: filter,
                min_filter: filter,
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                ..Default::default()
            })
        };
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Horizon mod post frame"),
            size: std::mem::size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            format,
            layout,
            pipeline_layout,
            linear: sampler("Horizon mod post linear", wgpu::FilterMode::Linear),
            nearest: sampler("Horizon mod post nearest", wgpu::FilterMode::Nearest),
            uniform,
            built: None,
            output: None,
            started: std::time::Instant::now(),
            frames: 0,
        }
    }

    /// Compile `source` (without the prelude) if `version` is new. True when the pass can run.
    pub fn prepare(&mut self, device: &wgpu::Device, version: u64, source: &str) -> bool {
        if self.built.as_ref().is_none_or(|(v, _)| *v != version) {
            let full = format!("{PRELUDE}\n{source}");
            let pipeline = match validate(&full) {
                Ok(()) => Some(self.pipeline(device, &full)),
                Err(e) => {
                    log::warn!("Horizon: a mod's post pass did not compile: {e}");
                    None
                }
            };
            self.built = Some((version, pipeline));
        }
        self.built.as_ref().is_some_and(|(_, p)| p.is_some())
    }

    fn pipeline(&self, device: &wgpu::Device, source: &str) -> wgpu::RenderPipeline {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Horizon mod post"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Horizon mod post"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("horizon_mod_post_vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    /// Draw the pass over the world's `color` and `depth` (`size` pixels), and return its picture. Call after
    /// `prepare` said it can run.
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        size: [u32; 2],
        mut frame: FrameUniform,
    ) -> &wgpu::TextureView {
        if self.output.as_ref().is_none_or(|o| o.size != size) {
            let view = device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("Horizon mod post picture"),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default());
            self.output = Some(Output { size, view });
        }
        let output = self.output.as_ref().expect("made above");
        let (w, h) = (size[0] as f32, size[1] as f32);
        frame.resolution = [w, h, 1.0 / w, 1.0 / h];
        frame.time[0] = self.started.elapsed().as_secs_f32();
        frame.time[1] = self.frames as f32;
        self.frames = self.frames.wrapping_add(1);
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&frame));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Horizon mod post"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(color),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(depth),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.linear),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.nearest),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: self.uniform.as_entire_binding(),
                },
            ],
        });
        if let Some((_, Some(pipeline))) = &self.built {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Horizon mod post"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        &output.view
    }
}

/// Parse and validate a whole pass (prelude included) before wgpu sees it, so a broken one is an error, not a
/// device fault.
fn validate(source: &str) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(source).map_err(|e| e.emit_to_string(source))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .map_err(|e| e.emit_to_string(source))?;
    if !module
        .entry_points
        .iter()
        .any(|e| e.name == "fs_main" && e.stage == naga::ShaderStage::Fragment)
    {
        return Err("no `@fragment fn fs_main`".into());
    }
    Ok(())
}
