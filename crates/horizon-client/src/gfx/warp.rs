//! A mod's world warp (`render.set-warp`, capability `render`): a WGSL function the terrain's and the entities' vertices
//! go through, wherever they are, the far terrain's tiles too, so a mod can bend the world's real geometry: real blocks,
//! light and depth, seen from every side, nothing left to a picture of it.
//!
//! The function takes the place of `world_warp` in terrain.wgsl and entity.wgsl (between `// [world warp]` and
//! `// [end world warp]`): camera-relative position and a copy's number to where to draw it and a colour to multiply it by.
//! It reads `globals.warp` (see `uniform`). Its pipelines are the terrain's without back-face culling, since what the warp
//! turns over stays drawn, and while it is on nothing is left out of the frame by where it was (the frustum, the caves,
//! occlusion, faces turned away): sections and tiles out of view may be bent into it. Faces no light reaches (caves, the
//! rock's insides) are dropped in the pixels (terrain.wgsl): from under the land they would be all that shows.
//!
//! `copies` draws the solid sections within `radius` of `anchor` that many times more, with their number (1 up) in
//! `copy`: so a mod can multiply the land near a point. It is ignored on `Flat` frames' path (which has no use here: the
//! warp's frames take the per-section draws, which the emulated multi-draw of Metal also runs, one call a draw).
use super::*;

/// A mod's world warp this frame.
pub struct WorldWarp<'a> {
    pub source: &'a str,
    pub version: u64,
    pub params: [f32; 16],
    pub anchor: [f64; 3],
    pub copies: u32,
    pub radius: f32,
    pub partial_tick: f32,
}

/// What the pipelines are built from.
pub(super) struct Kit {
    pub layout: wgpu::PipelineLayout,
    pub entity_layout: wgpu::PipelineLayout,
    pub format: wgpu::TextureFormat,
    pub sprite_constants: [f64; 2],
}
pub(super) struct Pipelines {
    version: u64,
    pub solid: wgpu::RenderPipeline,
    pub translucent: wgpu::RenderPipeline,
    pub entity: wgpu::RenderPipeline,
}

/// `globals.warp`: the sixteen floats; the camera in the world (f32) and the partial tick; what the f32 leaves out of
/// the camera, and 1. (So `(a - warp[4].xyz) - warp[5].xyz` is where the world's point `a` is from the camera, the
/// difference taken first, to under a millimetre far from the world's origin.)
pub(super) fn uniform(w: &WorldWarp<'_>, camera: DVec3) -> [[f32; 4]; 6] {
    let mut u = [[0.0; 4]; 6];
    for (to, from) in u.iter_mut().zip(w.params.chunks(4)) {
        to.copy_from_slice(from);
    }
    let at = camera.to_array().map(|c| c as f32);
    u[4] = [at[0], at[1], at[2], w.partial_tick];
    u[5] = [
        (camera.x - f64::from(at[0])) as f32,
        (camera.y - f64::from(at[1])) as f32,
        (camera.z - f64::from(at[2])) as f32,
        1.0,
    ];
    u
}

/// The visible list's indices of the sections to copy: those within `radius` of the anchor across the land.
pub(super) fn near(w: &WorldWarp<'_>, camera: DVec3, visible: &[resident::Visible]) -> Vec<usize> {
    if w.copies == 0 {
        return Vec::new();
    }
    let r = f64::from(w.radius) + 12.0;
    let (ax, az) = (w.anchor[0] - camera.x, w.anchor[2] - camera.z);
    visible
        .iter()
        .enumerate()
        .filter(|(_, v)| {
            let (dx, dz) = (f64::from(v.origin[0]) + 8.0 - ax, f64::from(v.origin[2]) + 8.0 - az);
            dx * dx + dz * dz <= r * r
        })
        .map(|(i, _)| i)
        .collect()
}

/// `shader` with the world warp in `source` in place of the default.
fn with_warp(shader: &str, source: &str) -> Option<String> {
    const FROM: &str = "fn world_warp(pos: vec3<f32>, copy: u32)";
    const TO: &str = "// [end world warp]";
    let a = shader.find(FROM)?;
    let b = shader[a..].find(TO)? + a;
    Some(format!("{}{source}\n{}", &shader[..a], &shader[b..]))
}
fn validate(source: &str) -> std::result::Result<(), String> {
    let module = naga::front::wgsl::parse_str(source).map_err(|e| e.emit_to_string(source))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module)
        .map_err(|e| e.emit_to_string(source))?;
    Ok(())
}

impl Renderer {
    /// The warp's pipelines ready for this frame: built when its source changes, not again if it was refused.
    pub(super) fn prepare_warp(&mut self, w: &WorldWarp<'_>) -> bool {
        if self.warp.as_ref().is_some_and(|p| p.version == w.version) {
            return true;
        }
        if self.warp_refused == Some(w.version) {
            return false;
        }
        match self.build_warp(w.version, w.source) {
            Ok(p) => {
                self.warp = Some(p);
                true
            }
            Err(e) => {
                log::warn!("Horizon: a mod's world warp did not compile: {e}");
                self.warp_refused = Some(w.version);
                false
            }
        }
    }

    fn build_warp(&self, version: u64, source: &str) -> std::result::Result<Pipelines, String> {
        let started = std::time::Instant::now();
        let terrain = with_warp(include_str!("../terrain.wgsl"), source).ok_or("no warp marker in the terrain's shader")?;
        let entity = with_warp(&entity_shader(), source).ok_or("no warp marker in the entities' shader")?;
        validate(&terrain)?;
        validate(&entity)?;
        let device = &self.device;
        let kit = &self.warp_kit;
        let module = |label, source: String| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            })
        };
        let terrain = module("Warped terrain", terrain);
        let entity = module("Warped entities", entity);
        let instance_attributes = wgpu::vertex_attr_array![5=>Float32x4];
        let buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<TerrainVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &TERRAIN_ATTRIBUTES,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: 16,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &instance_attributes,
            }),
        ];
        let constants = [
            ("SPRITE_ARRAYS", kit.sprite_constants[0]),
            ("SPRITE_SPLIT", kit.sprite_constants[1]),
        ];
        let depth = wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
            stencil: Default::default(),
            bias: Default::default(),
        };
        let terrain_pipeline = |label, fragment, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&kit.layout),
                vertex: wgpu::VertexState {
                    module: &terrain,
                    entry_point: Some("vs_terrain"),
                    compilation_options: Default::default(),
                    buffers: &buffers,
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(depth.clone()),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &terrain,
                    entry_point: Some(fragment),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &constants,
                        ..Default::default()
                    },
                    targets: &[Some(wgpu::ColorTargetState {
                        format: kit.format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let solid = terrain_pipeline("Warped terrain solid", "fs_solid", None);
        let translucent = terrain_pipeline("Warped terrain translucent", "fs_translucent", Some(wgpu::BlendState::ALPHA_BLENDING));
        let entity_attributes = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Uint32,3=>Unorm8x4,4=>Uint8x4,5=>Snorm8x4];
        let entity = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Warped entities"),
            layout: Some(&kit.entity_layout),
            vertex: wgpu::VertexState {
                module: &entity,
                entry_point: Some("vs_entity"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &entity_attributes,
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(depth.clone()),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &entity,
                entry_point: Some("fs_entity"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: kit.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        log::info!("Horizon: a mod's world warp built in {} ms", started.elapsed().as_millis());
        Ok(Pipelines { version, solid, translucent, entity })
    }
}
