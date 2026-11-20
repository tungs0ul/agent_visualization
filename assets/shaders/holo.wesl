// The desk subject holograms (`holo.rs`): emissive light whose colour moves toward a
// rim colour where the surface turns away from the viewer, as the prototype's
// Layer Weight "Facing" did in Blender (`tools/blender/holo_glb.py`).
import bevy_pbr::render::forward_io::VertexOutput;
import bevy_pbr::render::mesh_view_bindings::view;

struct Holo {
    // rgb: the light face-on; a: the output alpha (0 = added as light, 1 = solid)
    base: vec4<f32>,
    // rgb: the light at the silhouette
    rim: vec4<f32>,
    // x: k, y: e in f = (1 - |n.v|^k)^e; z: overall gain
    shape: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> holo: Holo;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var holo_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var holo_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let to_eye = normalize(view.world_position - in.world_position.xyz);
    let facing = abs(dot(normalize(in.world_normal), to_eye));
    let f = pow(1.0 - pow(facing, holo.shape.x), holo.shape.y);
    let light = mix(holo.base.rgb, holo.rim.rgb, f) * textureSample(holo_texture, holo_sampler, in.uv).rgb;
    return vec4(light * holo.shape.z, holo.base.a);
}
