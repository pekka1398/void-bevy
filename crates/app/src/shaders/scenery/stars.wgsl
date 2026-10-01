// lab/scenery's Stars.ts: one-pixel points 1e12 m away, their colour scaled by how dark the sky is.

#import bevy_pbr::mesh_functions::{get_world_from_local, mesh_position_local_to_world}
#import bevy_pbr::view_transformations::position_world_to_clip

/// Brightness of a magnitude-0 star; the page sets it from how dark the sky is.
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> brightness: f32;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
}

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world = mesh_position_local_to_world(get_world_from_local(v.instance_index), vec4(v.position, 1.0)).xyz;
    out.clip_position = position_world_to_clip(world);
    out.color = v.color.rgb;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4(in.color * brightness, 1.0);
}
