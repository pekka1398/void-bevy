// lab/scenery's GroundMaterial.ts: the lit ground and sea on lab/lod's tiles. Unlit by Bevy: sunlight
// is computed here, through the atmosphere's transmittance, so the ground reddens at sunset and goes
// dark past the terminator; the transport pass then adds the air between the ground and the camera.
//
// Render space is the planet's body-fixed axes with the camera at the origin, so a world position is
// the camera-relative body-fixed vector.

#import bevy_pbr::mesh_functions::{get_world_from_local, mesh_position_local_to_world, mesh_normal_local_to_world}
#import bevy_pbr::view_transformations::position_world_to_clip
#import void::atmosphere::{AtmosphereShape, PI, sun_transmittance, sky_irradiance}

struct Ground {
    /// The planet centre, camera-relative, metres.
    planet_center: vec3<f32>,
    /// Sunlight above the air; the unit every radiance here is measured in.
    sun_illuminance: f32,
    /// Unit vector toward the sun, body-fixed.
    sun_direction: vec3<f32>,
    /// 0 lights the ground with bare sunlight and no sky.
    atmosphere_enabled: f32,
    /// Camera body-fixed position modulo WAVE_PERIOD on each axis, metres.
    wave_origin: vec3<f32>,
    /// Seconds, wrapped at a day.
    time: f32,
    /// Sea level above the reference radius, metres.
    sea_level: f32,
    ocean_enabled: f32,
    /// Heights where rock and snow begin, metres.
    rock_height: f32,
    snow_height: f32,
    bottom_radius: f32,
    top_radius: f32,
    horizon: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> ground: Ground;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var transmittance_table: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var transmittance_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var irradiance_table: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var irradiance_sampler: sampler;

/// Deep-water waves: integer multiples of 2π / WAVE_PERIOD on each body-fixed axis, so the pattern
/// repeats every WAVE_PERIOD metres along x, y and z. Each is (nx, ny, nz, slope): slope is
/// amplitude times wavenumber.
const WAVE_PERIOD: f32 = 4096.0;
const WAVES = array<vec4<f32>, 4>(
    vec4(37.0, 91.0, 13.0, 0.06),
    vec4(-83.0, 22.0, 57.0, 0.05),
    vec4(150.0, -40.0, 110.0, 0.035),
    vec4(-20.0, 170.0, -190.0, 0.025),
);
const GRAVITY: f32 = 9.81;
/// Irradiance at night, per unit sun illuminance.
const NIGHT_LIGHT: f32 = 2e-4;
/// Wavelengths of the ground's sub-mesh detail, metres; each divides WAVE_PERIOD.
const DETAIL_WAVELENGTHS = array<f32, 5>(256.0, 64.0, 16.0, 4.0, 1.0);

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) height: f32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) height: f32,
}

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    var out: VertexOutput;
    var world = mesh_position_local_to_world(get_world_from_local(v.instance_index), vec4(v.position, 1.0)).xyz;
    // Vertices below sea level are raised onto the sea sphere.
    let up = normalize(world - ground.planet_center);
    world += up * (max(ground.sea_level - v.height, 0.0) * ground.ocean_enabled);
    out.clip_position = position_world_to_clip(world);
    out.world_position = world;
    out.world_normal = mesh_normal_local_to_world(v.normal, v.instance_index);
    out.color = v.color.rgb;
    out.height = v.height;
    return out;
}

/// GLSL's mod: x − y·floor(x / y), never negative for a positive y.
fn glsl_mod(x: vec3<f32>, y: f32) -> vec3<f32> {
    return x - y * floor(x / y);
}

fn hash_corner(cell: vec3<f32>, d: vec3<f32>, period: f32) -> f32 {
    let c = glsl_mod(cell + d, period);
    return fract(sin(dot(c, vec3(127.1, 311.7, 74.7))) * 43758.5453);
}

/// Value noise in 0–1 on an integer lattice that repeats every `period` cells.
fn periodic_value_noise(p: vec3<f32>, period: f32) -> f32 {
    let cell = floor(p);
    let f = fract(p);
    let u = f * f * (f * -2.0 + 3.0);
    let x00 = mix(hash_corner(cell, vec3(0.0, 0.0, 0.0), period), hash_corner(cell, vec3(1.0, 0.0, 0.0), period), u.x);
    let x10 = mix(hash_corner(cell, vec3(0.0, 1.0, 0.0), period), hash_corner(cell, vec3(1.0, 1.0, 0.0), period), u.x);
    let x01 = mix(hash_corner(cell, vec3(0.0, 0.0, 1.0), period), hash_corner(cell, vec3(1.0, 0.0, 1.0), period), u.x);
    let x11 = mix(hash_corner(cell, vec3(0.0, 1.0, 1.0), period), hash_corner(cell, vec3(1.0, 1.0, 1.0), period), u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let shape = AtmosphereShape(ground.bottom_radius, ground.top_radius, ground.horizon);
    let body_position = in.world_position;
    let from_center = body_position - ground.planet_center;
    let up = normalize(from_center);
    let r = length(from_center);
    let sun = ground.sun_direction;
    let sun_mu = dot(up, sun);
    let enabled = ground.atmosphere_enabled;
    let sunlight = mix(vec3(1.0), sun_transmittance(transmittance_table, transmittance_sampler, shape, r, sun_mu), enabled)
        * ground.sun_illuminance;
    // Sky irradiance on level ground from the table (multiple scattering included), plus a night floor
    // for starlight and airglow, exaggerated about a hundredfold so the night side is dim, not black.
    let sky_light = sky_irradiance(irradiance_table, irradiance_sampler, shape, r, sun_mu) * enabled * ground.sun_illuminance
        + NIGHT_LIGHT;

    // Land.
    let normal = normalize(in.world_normal);
    let flat = dot(normal, up);
    let height = in.height;
    let sea = ground.sea_level;
    let ocean = ground.ocean_enabled;
    let above_sea = height - sea * ocean;
    let sand = vec3(0.42, 0.37, 0.26);
    let rock = vec3(0.16, 0.145, 0.13);
    let snow = vec3(0.62, 0.64, 0.68);
    // What covers the land (desert to forest) comes with the tile, from the terrain's sampler.
    let lowland = mix(sand, in.color, max(smoothstep(2.0, 12.0, above_sea), 1.0 - ocean));
    // Bare rock on steep ground, and on gentler slopes the higher it is.
    let rock_height = ground.rock_height;
    let high = smoothstep(sea, rock_height, height);
    let steep = 1.0 - smoothstep(mix(0.75, 0.94, high), mix(0.9, 0.985, high), flat);
    let rocky = mix(lowland, rock, max(steep, smoothstep(rock_height, rock_height + 800.0, height)));
    // The snow line falls toward the poles, to 30% of its height above the sea there.
    let snow_line = ground.snow_height - (ground.snow_height - sea) * 0.7 * (up.z * up.z);
    let albedo = mix(rocky, snow, smoothstep(snow_line, snow_line + 300.0, height) * smoothstep(0.8, 0.9, flat));
    // Detail finer than the mesh: mottling from 256 m down to 1 m, each octave faded out where it is
    // under a pixel.
    let detail_position = body_position + ground.wave_origin;
    var detail = 0.0;
    // Function-scope copies: naga indexes those, not module constants, with a runtime index.
    var wavelengths = DETAIL_WAVELENGTHS;
    for (var index = 0; index < 5; index++) {
        let wavelength = wavelengths[index];
        let q = detail_position / wavelength + f32(index) * 17.31;
        let width = fwidth(q);
        let visible = 1.0 - smoothstep(0.3, 1.0, max(width.x, max(width.y, width.z)));
        detail += (periodic_value_noise(q, WAVE_PERIOD / wavelength) - 0.5) * visible * (0.35 * pow(0.8, f32(index)));
    }
    let mottled = albedo * (detail + 1.0);
    // A tilted surface sees part of the sky: (1 + N·up) / 2 of it.
    let land = mottled * (sunlight * max(dot(normal, sun), 0.0) + sky_light * (dot(normal, up) * 0.5 + 0.5)) / PI;

    // Sea: wave normals from the waves' slopes, faded out where a wave is under a pixel wide.
    let wave_position = body_position + ground.wave_origin;
    var slope = vec3(0.0);
    var drawn = 0.0;
    var waves = WAVES;
    for (var w = 0; w < 4; w++) {
        let wave = waves[w];
        let k = wave.xyz * (2.0 * PI / WAVE_PERIOD);
        let omega = sqrt(GRAVITY * (2.0 * PI / WAVE_PERIOD) * length(wave.xyz));
        let phase = dot(k, wave_position) - ground.time * omega;
        let resolved = 1.0 - smoothstep(0.4, 1.5, fwidth(phase));
        let along = normalize(k - up * dot(k, up));
        slope += along * (cos(phase) * wave.w * resolved);
        drawn += resolved / 4.0;
    }
    let water_normal = normalize(up - slope);
    let to_camera = normalize(-body_position);
    let facing = clamp(dot(water_normal, to_camera), 0.0, 1.0);
    let fresnel = 0.02 + pow(1.0 - facing, 5.0) * 0.98;
    let depth = max(sea - height, 0.0);
    let deep = vec3(0.004, 0.018, 0.035);
    let water = mix(deep, albedo * 0.5, pow(0.5, depth / 6.0));
    let body = water * (sunlight * max(sun_mu, 0.0) + sky_light) / PI;
    // The sky's average radiance is its irradiance over π; the sky near the horizon, which grazing
    // views reflect, is brighter.
    let sky_reflection = sky_light / PI * mix(1.0, 2.0, pow(1.0 - facing, 2.0));
    let halfway = normalize(sun + to_camera);
    // Waves too small to draw still roughen the sea: their slopes spread the glint as a broad, dim
    // lobe, the way the glitter looks from orbit.
    let shininess = mix(60.0, 600.0, drawn);
    let glint = pow(max(dot(water_normal, halfway), 0.0), shininess) * ((shininess + 8.0) / (8.0 * PI))
        * fresnel * sunlight * max(dot(water_normal, sun), 0.0);
    let sea_color = mix(body, sky_reflection, fresnel) + glint;

    // Coast: where the interpolated height crosses sea level, about a pixel wide.
    let coast_width = fwidth(height);
    let wet = (1.0 - smoothstep(-coast_width, coast_width, height - sea)) * ocean;
    return vec4(mix(land, sea_color, wet), 1.0);
}
