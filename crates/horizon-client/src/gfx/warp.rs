//! A mod's world warp (`render.set-warp`, capability `render`): a WGSL function the terrain's and the entities' vertices
//! go through, wherever they are, the far terrain's tiles too, so a mod can bend the world's real geometry: real blocks,
//! light and depth, seen from every side, nothing left to a picture of it.
//!
//! The function takes the place of `world_warp` in terrain.wgsl and entity.wgsl (between `// [world warp]` and
//! `// [end world warp]`): camera-relative position and a copy's number to where to draw it and a colour to multiply it by.
//! It reads `globals.warp` (see `uniform`). Its pipelines are the terrain's without back-face culling, since what the warp
//! turns over stays drawn, and while it is on nothing is left out of the frame by where it was (the frustum, the caves,
//! occlusion, faces turned away): sections and tiles out of view may be bent into it. Faces no light reaches (caves, the
//! rock's insides) are dropped in the pixels (`DEFAULT`): from under the land they would be all that shows.
//!
//! A warp's source may also give `world_fragment` (gfx/warp_fragment.wgsl): every pixel of the terrain with where it really
//! is, where it is drawn, its texture coordinates, its light and which way it faces, so a mod can cut, paint or retexture
//! the land by where it belongs and not by where it was thrown, to the pixel. Without one, `DEFAULT` keeps the warp's
//! default: the texture, less the faces no light reaches. All of it goes in only for these pipelines.
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
/// The fragments of a warp that has no `world_fragment`: the texture as the game draws it, less the faces no light reaches
/// and those the warp cut.
const DEFAULT: &str = "fn world_fragment(f: Fragment) -> Painted {
    if f.cut > 0.0 || (f.light.x < 0.5 && f.light.y < 0.5) { return Painted(vec4(0.0, 0.0, 0.0, -1.0), 1.0); }
    return Painted(sprite(f.uv, f.layer) * f.color, 1.0);
}
";
const VARYINGS: &str = "@location(5) home: vec3<f32>,
    @location(6) at: vec3<f32>,
    @location(7) warp_light: vec2<f32>,
    @location(8) cut: f32,
    @location(9) @interpolate(flat) copy: u32,
";
const VERTEX: &str = "out.home = position + origin;
    out.at = pos;
    out.warp_light = vec2(f32(light.x), f32(light.y)) / 16.0;
    out.cut = select(0.0, 1.0, warped.tint.r < 0.0);
    out.copy = copy;
";
/// `shader` with what stands between the two markers `name` replaced by `with`.
fn fill(shader: &str, name: &str, with: &str) -> Option<String> {
    let (from, to) = (format!("// [{name}]"), format!("// [end {name}]"));
    let a = shader.find(&from)?;
    let b = shader[a..].find(&to)? + a;
    Some(format!("{}{with}{}", &shader[..a], &shader[b + to.len()..]))
}
/// The terrain's shader for the warp in `source`: its `world_warp`, the pixels' inputs, and its `world_fragment` or the default.
fn warped_terrain(source: &str) -> Option<String> {
    let shader = with_warp(include_str!("../terrain.wgsl"), source)?;
    let shader = fill(&shader, "warp varyings", VARYINGS)?;
    let shader = fill(&shader, "warp vertex", VERTEX)?;
    let fragment = if source.contains("fn world_fragment") { "" } else { DEFAULT };
    Some(format!("{shader}\n{}\n{fragment}", include_str!("warp_fragment.wgsl")))
}
/// A warp's source for the entities' shader, which has no block sprites or `Fragment`: what follows the line
/// `// [fragments]` is the terrain's alone.
fn for_entities(source: &str) -> &str {
    source.split("// [fragments]").next().unwrap_or(source)
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
        let terrain = warped_terrain(source).ok_or("no warp marker in the terrain's shader")?;
        let entity = with_warp(&entity_shader(), for_entities(source)).ok_or("no warp marker in the entities' shader")?;
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
        let solid = terrain_pipeline("Warped terrain solid", "fs_warp_solid", None);
        let translucent = terrain_pipeline("Warped terrain translucent", "fs_warp_translucent", Some(wgpu::BlendState::ALPHA_BLENDING));
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

#[cfg(test)]
mod tests {
    use super::*;

    const WARP: &str = "fn world_warp(pos: vec3<f32>, copy: u32) -> Warped { return Warped(pos, vec3<f32>(1.0)); }\n";

    #[test]
    fn a_warp_without_fragments_keeps_the_default() {
        let terrain = warped_terrain(WARP).expect("the markers");
        validate(&terrain).expect("valid");
        assert!(terrain.contains(DEFAULT));
    }

    #[test]
    fn a_warps_own_fragments_take_the_place_of_the_default_and_stay_out_of_the_entities() {
        let source = format!(
            "{WARP}// [fragments]\nfn world_fragment(f: Fragment) -> Painted {{
                if f.copy > 0u || !f.front {{ return Painted(vec4(0.0, 0.0, 0.0, -1.0), 1.0); }}
                return Painted(sprite(f.uv * 2.0, f.layer) * f.color, length(f.home - f.at) * f.light.x);
            }}\n"
        );
        let terrain = warped_terrain(&source).expect("the markers");
        validate(&terrain).expect("valid");
        assert!(!terrain.contains(DEFAULT));
        let entity = with_warp(&entity_shader(), for_entities(&source)).expect("the markers");
        validate(&entity).expect("valid");
        assert!(!entity.contains("world_fragment"));
    }

    #[test]
    fn the_shader_without_a_warp_has_none_of_the_warps_fragments() {
        let terrain = include_str!("../terrain.wgsl");
        validate(terrain).expect("valid");
        assert!(!terrain.contains("home:") && !terrain.contains("world_fragment"));
    }
}
