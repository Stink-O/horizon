//! A mod's post-processing pass (capability `render`, docs/mods.md): the mod's WGSL fragment shader, after
//! `mod_post.wgsl`, drawn over the world's own target (its colour and depth) into a picture of its own, which a
//! shader pack's post-processing or the copy onto the screen then takes in place of the world's.
//!
//! A pass may also be drawn in two stages (docs/mods.md, "A half-size stage"): with an `fs_half` and an `fs_join`
//! besides `fs_main`, `fs_half` is drawn into one or two RGBA16F pictures of half the width and height, and `fs_join`
//! (which reads them at bindings 6 and 7, declared by the pass itself) over the world in place of `fs_main`. A build
//! without the stages, or a pass without them, draws `fs_main` alone: the declarations are only the pass's, so the
//! same source compiles and runs on either.
//!
//! The source is compiled when its version changes; one that does not validate is logged and skipped until the
//! mod gives a new one. The mod's data (`render.set-data`) is a read-only storage buffer, `mod_data`: one word, 0,
//! until it gives some, uploaded again only when its version changes.
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

/// The half-size stage's format (both its pictures).
const HALF_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

struct Output {
    size: [u32; 2],
    view: wgpu::TextureView,
    /// The half-size stage's pictures, made the first time a pass with one runs at this size.
    half: Option<[wgpu::TextureView; 2]>,
}

/// A pass's pipelines: `fs_main` alone, or the half-size stage (`fs_half`, writing `extra` + 1 pictures) and the
/// join over the world (`fs_join`).
enum Pipelines {
    Whole(wgpu::RenderPipeline),
    Split {
        half: wgpu::RenderPipeline,
        join: wgpu::RenderPipeline,
        extra: bool,
    },
}

/// What `validate` found in a pass: whether it has a half-size stage, and if so whether `fs_half` writes a second
/// picture (location 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stages {
    Whole,
    Split { extra: bool },
}

pub struct ModPost {
    format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    /// The join's: the same, and the half-size stage's two pictures at 6 and 7.
    join_layout: wgpu::BindGroupLayout,
    join_pipeline_layout: wgpu::PipelineLayout,
    linear: wgpu::Sampler,
    nearest: wgpu::Sampler,
    uniform: wgpu::Buffer,
    /// `mod_data`, and the version of the mod's data in it (0: none, the one word 0).
    data: wgpu::Buffer,
    data_version: u64,
    /// The source's version, and its pipelines (`None`: it did not compile).
    built: Option<(u64, Option<Pipelines>)>,
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
        let texture = |binding, sample_type| {
            fragment(
                binding,
                wgpu::BindingType::Texture {
                    sample_type,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            )
        };
        let entries = [
            texture(0, wgpu::TextureSampleType::Float { filterable: true }),
            texture(1, wgpu::TextureSampleType::Depth),
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
            fragment(
                5,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            ),
            texture(6, wgpu::TextureSampleType::Float { filterable: true }),
            texture(7, wgpu::TextureSampleType::Float { filterable: true }),
        ];
        let layouts = |label, entries| {
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries,
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(&layout)],
                ..Default::default()
            });
            (layout, pipeline_layout)
        };
        let (layout, pipeline_layout) = layouts("Horizon mod post", &entries[..6]);
        let (join_layout, join_pipeline_layout) = layouts("Horizon mod post join", &entries[..]);
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
            join_layout,
            join_pipeline_layout,
            linear: sampler("Horizon mod post linear", wgpu::FilterMode::Linear),
            nearest: sampler("Horizon mod post nearest", wgpu::FilterMode::Nearest),
            uniform,
            data: data_buffer(device, &[0]),
            data_version: 0,
            built: None,
            output: None,
            started: std::time::Instant::now(),
            frames: 0,
        }
    }

    /// The mod's data for `mod_data` (none: the one word 0), uploaded when its version is new: written into the
    /// buffer when it is the same size, else in a new buffer of its size (the bind group is made each frame).
    pub fn set_data(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        data: Option<(&[u32], u64)>,
    ) {
        let (words, version) = match data {
            Some((words, version)) if !words.is_empty() => (words, version),
            _ => (&[0u32][..], 0),
        };
        if version == self.data_version {
            return;
        }
        let bytes: &[u8] = bytemuck::cast_slice(words);
        if self.data.size() == bytes.len() as u64 {
            queue.write_buffer(&self.data, 0, bytes);
        } else {
            self.data = data_buffer(device, words);
        }
        self.data_version = version;
    }

    /// Compile `source` (without the prelude) if `version` is new. True when the pass can run.
    pub fn prepare(&mut self, device: &wgpu::Device, version: u64, source: &str) -> bool {
        if self.built.as_ref().is_none_or(|(v, _)| *v != version) {
            let full = format!("{PRELUDE}\n{source}");
            let pipeline =
                match validate(&full).and_then(|stages| self.pipelines(device, &full, stages)) {
                    Ok(p) => Some(p),
                    Err(e) => {
                        log::warn!("Horizon: a mod's post pass did not compile: {e}");
                        None
                    }
                };
            self.built = Some((version, pipeline));
        }
        self.built.as_ref().is_some_and(|(_, p)| p.is_some())
    }

    /// The pass's pipelines, made under a validation error scope: a pass whose bindings do not match the layout is an
    /// error to log, not a device fault.
    fn pipelines(
        &self,
        device: &wgpu::Device,
        source: &str,
        stages: Stages,
    ) -> Result<Pipelines, String> {
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Horizon mod post"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let target = |format| {
            Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })
        };
        let pipeline = |layout, entry, targets: &[Option<wgpu::ColorTargetState>]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Horizon mod post"),
                layout: Some(layout),
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
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipelines = match stages {
            Stages::Whole => Pipelines::Whole(pipeline(
                &self.pipeline_layout,
                "fs_main",
                &[target(self.format)],
            )),
            Stages::Split { extra } => Pipelines::Split {
                half: pipeline(
                    &self.pipeline_layout,
                    "fs_half",
                    &[target(HALF_FORMAT), target(HALF_FORMAT).filter(|_| extra)],
                ),
                join: pipeline(
                    &self.join_pipeline_layout,
                    "fs_join",
                    &[target(self.format)],
                ),
                extra,
            },
        };
        match pollster::block_on(scope.pop()) {
            Some(e) => Err(e.to_string()),
            None => Ok(pipelines),
        }
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
            let view = picture(device, "Horizon mod post picture", size, self.format);
            self.output = Some(Output {
                size,
                view,
                half: None,
            });
        }
        let split = matches!(&self.built, Some((_, Some(Pipelines::Split { .. }))));
        let output = self.output.as_mut().expect("made above");
        if split && output.half.is_none() {
            // (half the width and height, rounded up: each of its pixels stands for two by two of the world's)
            let half = [size[0].div_ceil(2), size[1].div_ceil(2)];
            output.half = Some([
                picture(device, "Horizon mod post half", half, HALF_FORMAT),
                picture(device, "Horizon mod post half extra", half, HALF_FORMAT),
            ]);
        }
        let output = self.output.as_ref().expect("made above");
        let (w, h) = (size[0] as f32, size[1] as f32);
        frame.resolution = [w, h, 1.0 / w, 1.0 / h];
        frame.time[0] = self.started.elapsed().as_secs_f32();
        frame.time[1] = self.frames as f32;
        self.frames = self.frames.wrapping_add(1);
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&frame));
        let mut entries = vec![
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
            wgpu::BindGroupEntry {
                binding: 5,
                resource: self.data.as_entire_binding(),
            },
        ];
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Horizon mod post"),
            layout: &self.layout,
            entries: &entries,
        });
        let draw = |encoder: &mut wgpu::CommandEncoder,
                    targets: &[Option<&wgpu::TextureView>],
                    pipeline: &wgpu::RenderPipeline,
                    group: &wgpu::BindGroup| {
            let attachments: Vec<_> = targets
                .iter()
                .map(|t| {
                    t.map(|view| wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })
                })
                .collect();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Horizon mod post"),
                color_attachments: &attachments,
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        };
        match &self.built {
            Some((_, Some(Pipelines::Whole(pipeline)))) => {
                draw(encoder, &[Some(&output.view)], pipeline, &group);
            }
            Some((_, Some(Pipelines::Split { half, join, extra }))) => {
                let [first, second] = output.half.as_ref().expect("made above");
                draw(
                    encoder,
                    &[Some(first), extra.then_some(second)],
                    half,
                    &group,
                );
                entries.extend([
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::TextureView(first),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: wgpu::BindingResource::TextureView(second),
                    },
                ]);
                let join_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Horizon mod post join"),
                    layout: &self.join_layout,
                    entries: &entries,
                });
                draw(encoder, &[Some(&output.view)], join, &join_group);
            }
            _ => {}
        }
        &output.view
    }
}

/// A picture a pass draws into and a later one reads.
fn picture(
    device: &wgpu::Device,
    label: &str,
    size: [u32; 2],
    format: wgpu::TextureFormat,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// A storage buffer holding `words` (at least one).
fn data_buffer(device: &wgpu::Device, words: &[u32]) -> wgpu::Buffer {
    use wgpu::util::DeviceExt;
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Horizon mod post data"),
        contents: bytemuck::cast_slice(words),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    })
}

/// Parse and validate a whole pass (prelude included) before wgpu sees it, so a broken one is an error, not a
/// device fault; and find whether it has a half-size stage.
fn validate(source: &str) -> Result<Stages, String> {
    let module = naga::front::wgsl::parse_str(source).map_err(|e| e.emit_to_string(source))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .map_err(|e| e.emit_to_string(source))?;
    let entry = |name: &str| {
        module
            .entry_points
            .iter()
            .position(|e| e.name == name && e.stage == naga::ShaderStage::Fragment)
    };
    if entry("fs_main").is_none() {
        return Err("no `@fragment fn fs_main`".into());
    }
    let half = match (entry("fs_half"), entry("fs_join")) {
        (None, None) => return Ok(Stages::Whole),
        (Some(half), Some(_)) => half,
        _ => {
            return Err(
                "a half-size stage needs both `@fragment fn fs_half` and `fn fs_join`".into(),
            );
        }
    };
    // the half-size stage writes its pictures at locations 0 and (if it likes) 1, and cannot read them
    let locations = match &module.entry_points[half].function.result {
        Some(naga::FunctionResult {
            binding: Some(naga::Binding::Location { location, .. }),
            ..
        }) => vec![*location],
        Some(naga::FunctionResult { ty, binding: None }) => match &module.types[*ty].inner {
            naga::TypeInner::Struct { members, .. } => members
                .iter()
                .filter_map(|m| match m.binding {
                    Some(naga::Binding::Location { location, .. }) => Some(location),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    if !locations.contains(&0) || locations.iter().any(|&l| l > 1) {
        return Err("`fs_half` writes location 0 and at most location 1".into());
    }
    let uses = info.get_entry_point(half);
    for (handle, var) in module.global_variables.iter() {
        if !uses[handle].is_empty()
            && var
                .binding
                .as_ref()
                .is_some_and(|b| b.group == 0 && b.binding >= 6)
        {
            return Err("`fs_half` reads the pictures it draws (bindings 6 and 7)".into());
        }
    }
    Ok(Stages::Split {
        extra: locations.contains(&1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pass_reads_its_data() {
        // the prelude declares mod_data; a pass may read it (guarded by its length), or leave it alone
        let reads = "@fragment fn fs_main(in: StageIn) -> @location(0) vec4<f32> {\n    \
            let n = arrayLength(&mod_data);\n    \
            let v = select(0.0, bitcast<f32>(mod_data[0]), n > 0u);\n    \
            return vec4<f32>(v, frame.mod_params[0].x, 0.0, 1.0);\n}\n";
        assert_eq!(validate(&format!("{PRELUDE}\n{reads}")), Ok(Stages::Whole));
        let ignores = "@fragment fn fs_main(in: StageIn) -> @location(0) vec4<f32> {\n    \
            return textureSampleLevel(scene_color, linear_sampler, in.uv, 0.0);\n}\n";
        validate(&format!("{PRELUDE}\n{ignores}")).unwrap();
        assert!(PRELUDE.contains("@group(0) @binding(5) var<storage, read> mod_data: array<u32>;"));
    }

    /// A pass with a half-size stage: `fs_main` for builds without it, `fs_half` into the half-size pictures,
    /// `fs_join` reading them (declared by the pass, so the same source compiles on a build whose prelude lacks them).
    const SPLIT: &str = "@group(0) @binding(6) var half_picture: texture_2d<f32>;\n\
        @group(0) @binding(7) var half_extra: texture_2d<f32>;\n\
        @fragment fn fs_main(in: StageIn) -> @location(0) vec4<f32> {\n    \
            return textureSampleLevel(scene_color, linear_sampler, in.uv, 0.0);\n}\n";

    #[test]
    fn a_pass_may_have_a_half_size_stage() {
        let one = "@fragment fn fs_half(in: StageIn) -> @location(0) vec4<f32> {\n    \
            return vec4<f32>(in.uv, 0.0, 1.0);\n}\n\
            @fragment fn fs_join(in: StageIn) -> @location(0) vec4<f32> {\n    \
            let s = textureSampleLevel(half_picture, linear_sampler, in.uv, 0.0);\n    \
            return textureSampleLevel(scene_color, linear_sampler, in.uv, 0.0) * s.a + s;\n}\n";
        assert_eq!(
            validate(&format!("{PRELUDE}\n{SPLIT}{one}")),
            Ok(Stages::Split { extra: false })
        );
        let two = "struct Half {\n    @location(0) c: vec4<f32>,\n    @location(1) e: vec4<f32>,\n}\n\
            @fragment fn fs_half(in: StageIn) -> Half {\n    \
            return Half(vec4<f32>(0.0), vec4<f32>(depth_at(in.uv)));\n}\n\
            @fragment fn fs_join(in: StageIn) -> @location(0) vec4<f32> {\n    \
            return textureLoad(half_extra, vec2<i32>(in.position.xy * 0.5), 0);\n}\n";
        assert_eq!(
            validate(&format!("{PRELUDE}\n{SPLIT}{two}")),
            Ok(Stages::Split { extra: true })
        );
        // fs_half alone, or one reading its own picture, is refused
        let alone = "@fragment fn fs_half(in: StageIn) -> @location(0) vec4<f32> {\n    \
            return vec4<f32>(0.0);\n}\n";
        assert!(validate(&format!("{PRELUDE}\n{SPLIT}{alone}")).is_err());
        let reads = one.replace(
            "vec4<f32>(in.uv, 0.0, 1.0)",
            "textureLoad(half_picture, vec2<i32>(0), 0)",
        );
        assert!(validate(&format!("{PRELUDE}\n{SPLIT}{reads}")).is_err());
        // the prelude leaves the declarations to the pass
        assert!(!PRELUDE.contains("@binding(6)"));
    }
}
