// A world warp's fragments (gfx/warp.rs): added to terrain.wgsl for the warp's pipelines only, so a frame without a warp
// pays for none of it. `world_fragment` is the mod's if its warp's source has one, else `DEFAULT` in gfx/warp.rs.

// A pixel of the land as `world_fragment` gets it.
struct Fragment {
    // Where this point of the world really is, from the camera: the same as `world_warp`'s `pos`, before it moved.
    home: vec3<f32>,
    // Where it is drawn, from the camera.
    at: vec3<f32>,
    // The texture coordinates and the sprite's layer: `sprite(uv, layer)` is the block's texture here. It repeats, so
    // `sprite(uv * 2.0, layer)` is the face tiled two by two.
    uv: vec2<f32>,
    layer: u32,
    // The vertices' colour: biome tint, light, ambient occlusion and the warp's tint.
    color: vec4<f32>,
    // Sky and block light, 0 to 15.
    light: vec2<f32>,
    // Over 0 when a vertex of this face has a tint with red under 0.
    cut: f32,
    copy: u32,
    // The face's front: the warp's pipelines don't cull, so the back of a face is drawn too.
    front: bool,
}
// What `world_fragment` makes of it. Alpha under 0 leaves the pixel out. `fog` is how much of the game's fog it takes, 0 to
// 1: a light in the land or a window on something else can leave it out.
struct Painted {
    color: vec4<f32>,
    fog: f32,
}
fn warp_fragment(in: TerrainOut, front: bool) -> Painted {
    return world_fragment(Fragment(in.home, in.at, in.uv, in.layer & 0x7fffu, in.color, in.warp_light, in.cut, in.copy, front));
}
fn warp_fogged(p: Painted, in: TerrainOut, alpha: f32) -> vec4<f32> {
    let fogged = apply_fog(vec4(p.color.rgb, alpha), in.spherical, in.cylindrical);
    return vec4(mix(p.color.rgb, fogged.rgb, p.fog), alpha);
}
@fragment fn fs_warp_solid(in: TerrainOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let p = warp_fragment(in, front);
    // Bit 15 of the layer marks opaque leaves, as in fs_solid: the warp's own cut still leaves them out.
    if p.color.a < 0.0 || (p.color.a < 0.5 && (in.layer & 0x8000u) == 0u) { discard; }
    return warp_fogged(p, in, 1.0);
}
@fragment fn fs_warp_translucent(in: TerrainOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let p = warp_fragment(in, front);
    if p.color.a < 0.01 { discard; }
    return warp_fogged(p, in, p.color.a);
}
