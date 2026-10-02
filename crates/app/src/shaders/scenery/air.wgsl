// lab/scenery's transport and resolve passes (AtmosphereNodes.transport, sunDisc, CloudNodes,
// SceneryPipeline), as one full-screen pass over the rendered scene: every pixel's colour plus the
// sun's disc is dimmed by the air and clouds between it and the camera, and the light they scatter
// toward the camera on the way is added. Air and clouds are integrated together, in depth order.
//
// Render space is the planet's body-fixed axes with the camera at the origin. The camera's own
// height comes from the CPU in f64 (`camera_altitude`): subtracting two 6,371 km radii in f32 would
// lose it to half a metre.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import void::atmosphere::{AtmosphereShape, PI, SUN_ANGULAR_RADIUS, sun_transmittance, multiple_scattering, sky_irradiance}

struct Air {
    /// Clip to view (the projection's inverse) and view to world (the camera's rotation).
    view_from_clip: mat4x4<f32>,
    world_from_view: mat4x4<f32>,
    /// The planet centre in render space (camera-relative), metres.
    planet_center: vec3<f32>,
    /// Camera height above the bottom radius, metres.
    camera_altitude: f32,
    /// Unit vector from the planet centre through the camera.
    camera_up: vec3<f32>,
    sun_illuminance: f32,
    /// Unit vector toward the sun, body-fixed.
    sun_direction: vec3<f32>,
    /// 0 skips the air: the scene is shown as rendered, with the bare sun disc on the sky.
    enabled: f32,
    rayleigh_scattering: vec3<f32>,
    rayleigh_scale_height: f32,
    ozone_absorption: vec3<f32>,
    ozone_center_height: f32,
    ozone_width: f32,
    mie_scattering: f32,
    mie_extinction: f32,
    mie_scale_height: f32,
    mie_anisotropy: f32,
    /// 0 leaves out multiple scattering, for comparison.
    multiple_enabled: f32,
    sun_disc_enabled: f32,
    near: f32,
    bottom_radius: f32,
    top_radius: f32,
    horizon: f32,
    // Clouds.
    clouds_enabled: f32,
    /// Camera body-fixed position modulo each noise period, metres.
    shape_origin: vec3<f32>,
    coverage: f32,
    detail_origin: vec3<f32>,
    weather_only: f32,
    macro_origin: vec3<f32>,
    sea_level: f32,
    /// Viewport height over 2 tan(fov / 2).
    focal_pixels: f32,
}

@group(0) @binding(0) var scene_texture: texture_2d<f32>;
@group(0) @binding(1) var depth_texture: texture_depth_2d;
@group(0) @binding(2) var<uniform> air: Air;
@group(0) @binding(3) var transmittance_table: texture_2d<f32>;
@group(0) @binding(4) var multiple_table: texture_2d<f32>;
@group(0) @binding(5) var irradiance_table: texture_2d<f32>;
@group(0) @binding(6) var table_sampler: sampler;
@group(0) @binding(7) var weather_texture: texture_2d<f32>;
@group(0) @binding(8) var shape_texture: texture_3d<f32>;
@group(0) @binding(9) var detail_texture: texture_3d<f32>;
@group(0) @binding(10) var noise_sampler: sampler;

/// Samples along each view ray through clear air.
const VIEW_STEPS: i32 = 32;
const CLOUD_BOTTOM: f32 = 1500.0;
const CLOUD_TOP: f32 = 8000.0;
const CLOUD_EXTINCTION: f32 = 0.0011;
const DEFAULT_CLOUD_COVERAGE: f32 = 0.62;
const SHAPE_PERIOD: f32 = 65536.0;
const DETAIL_PERIOD: f32 = 2048.0;
const SHAPE_SIZE: f32 = 64.0;
const DETAIL_SIZE: f32 = 32.0;
const WEATHER_WIDTH: f32 = 2048.0;
const WEATHER_HEIGHT: f32 = 1024.0;

fn shape() -> AtmosphereShape {
    return AtmosphereShape(air.bottom_radius, air.top_radius, air.horizon);
}

// ---------------------------------------------------------------- clouds (CloudNodes.ts)

/// Stable camera-relative height: (r² − R²) / (r + R), with the CPU's camera altitude.
fn cloud_height(position: vec3<f32>) -> f32 {
    let big_r = air.bottom_radius;
    let r0 = big_r + air.camera_altitude;
    let delta = air.camera_altitude * (big_r * 2.0 + air.camera_altitude) + dot(air.camera_up, position) * r0 * 2.0
        + dot(position, position);
    // At orbital distances the expanded quadratic cancels huge terms. The body-fixed position instead
    // stays planet-sized there; near the ground the rational form preserves centimetres.
    if air.camera_altitude > 50000.0 {
        return length(position - air.planet_center) - big_r;
    }
    return delta / (sqrt(max(big_r * big_r + delta, 1.0)) + big_r);
}

fn cloud_density(position: vec3<f32>, footprint: f32) -> f32 {
    let height = cloud_height(position) - air.sea_level;
    if !(height > CLOUD_BOTTOM && height < CLOUD_TOP) {
        return 0.0;
    }
    let up = normalize(position - air.planet_center);
    let uv = vec2(
        atan2(up.y, up.x) / (2.0 * PI) + (0.5 + 0.5 / WEATHER_WIDTH),
        (asin(clamp(up.z, -1.0, 1.0)) / PI + 0.5) * ((WEATHER_HEIGHT - 1.0) / WEATHER_HEIGHT) + 0.5 / WEATHER_HEIGHT,
    );
    let weather = textureSampleLevel(weather_texture, noise_sampler, uv, 0.0).rg;
    let coverage = smoothstep(0.3, 0.65, weather.x + (air.coverage - DEFAULT_CLOUD_COVERAGE) * 1.5) * 0.9;
    // Anchor the broad banks to the shell base. Their footprint is horizontal; density then tapers
    // toward a locally varying domed top, rather than a flat slab.
    let column = position - up * (height - CLOUD_BOTTOM);
    let macro_noise = textureSampleLevel(shape_texture, noise_sampler, (column + air.macro_origin) / (SHAPE_PERIOD * 16.0),
        log2(max(footprint / (SHAPE_PERIOD * 16.0 / SHAPE_SIZE), 1.0)));
    let macro_shape = macro_noise.b * 0.7 + macro_noise.r * 0.3;
    let bank = smoothstep(0.15, 0.7, macro_shape);
    let shape_level = log2(max(footprint / (SHAPE_PERIOD / SHAPE_SIZE), 1.0));
    let shape_value = textureSampleLevel(shape_texture, noise_sampler, (position + air.shape_origin) / SHAPE_PERIOD, shape_level).r;
    let unresolved = smoothstep(2000.0, 16000.0, footprint);
    let top = CLOUD_BOTTOM + (weather.y * 4500.0 + 2000.0) * (bank * 0.8 + 0.2)
        * (mix(shape_value, 0.5, unresolved) * 0.55 + 0.45);
    let h = (height - CLOUD_BOTTOM) / (top - CLOUD_BOTTOM);
    let profile = smoothstep(0.0, 0.08, h) * (1.0 - smoothstep(0.35, 1.0, h));
    let cells = clamp((shape_value - h * h * 0.25 - (1.0 - coverage)) / max(coverage, 0.001), 0.0, 1.0);
    let sheet = smoothstep(0.55, 0.85, coverage) * (1.0 - weather.y * 0.6);
    let base = mix(cells, coverage * 0.32, sheet);
    let detail_weight = 1.0 - smoothstep(80.0, 500.0, footprint);
    let detail = textureSampleLevel(detail_texture, noise_sampler, (position + air.detail_origin) / DETAIL_PERIOD,
        log2(max(footprint / (DETAIL_PERIOD / DETAIL_SIZE), 1.0))).r;
    // A filtered shape value followed by a threshold loses subpixel cloud coverage. Blend toward a
    // smooth coverage moment as kilometre-scale cells become unresolved.
    let resolved_base = mix(base,
        mix(pow(coverage, 3.0) * 0.45, coverage * 0.32, sheet) * clamp(1.0 - h * h * 0.6, 0.0, 1.0), unresolved);
    let volume = clamp(resolved_base * profile - (1.0 - detail) * 0.16 * detail_weight, 0.0, 1.0)
        * (weather.y * 0.55 + 0.45) * smoothstep(0.05, 0.5, bank);
    // Weather inspection keeps the same shell and lighting, with no local noise.
    return mix(volume, coverage * profile * 0.45, air.weather_only);
}

/// Sun-ray optical depth. Five expanding samples, capped at 120 km to limit horizon-ray cost.
fn sun_optical_depth(position: vec3<f32>, footprint: f32) -> f32 {
    let big_r = air.bottom_radius;
    let height = cloud_height(position);
    let r = big_r + height;
    let mu = dot(normalize(position - air.planet_center), air.sun_direction);
    let top = big_r + air.sea_level + CLOUD_TOP;
    let delta = height - (air.sea_level + CLOUD_TOP);
    let discriminant = r * r * (mu * mu) - delta * (top * 2.0 + delta);
    let end = clamp(-r * mu + sqrt(max(discriminant, 0.0)), 0.0, 120000.0);
    var optical = 0.0;
    for (var shadow_step = 0; shadow_step < 5; shadow_step++) {
        let i = f32(shadow_step);
        let s0 = i / 5.0;
        let s1 = (i + 1.0) / 5.0;
        let sm = (i + 0.5) / 5.0;
        let dt = end * (s1 * s1 - s0 * s0);
        let q = position + air.sun_direction * (end * (sm * sm));
        optical += cloud_density(q, max(dt * 0.5, footprint)) * dt * CLOUD_EXTINCTION;
    }
    return optical;
}

fn henyey_greenstein(g: f32, cos_theta: f32) -> f32 {
    return ((1.0 - g * g) / (4.0 * PI)) / pow(1.0 + g * g - cos_theta * (2.0 * g), 1.5);
}

/// Approximate cloud multiple scattering: diminishing phase anisotropy and optical depth in three orders.
fn cloud_source(position: vec3<f32>, rd: vec3<f32>, footprint: f32) -> vec3<f32> {
    let r = air.bottom_radius + cloud_height(position);
    let up = normalize(position - air.planet_center);
    let sun_mu = dot(up, air.sun_direction);
    let horizon_mu = -sqrt(max(1.0 - (air.bottom_radius * air.bottom_radius) / (r * r), 0.0));
    let visible = smoothstep(horizon_mu - SUN_ANGULAR_RADIUS, horizon_mu + SUN_ANGULAR_RADIUS, sun_mu);
    let to_sun = mix(vec3(visible), sun_transmittance(transmittance_table, table_sampler, shape(), r, sun_mu), air.enabled);
    let cos_theta = dot(rd, air.sun_direction);
    let optical = sun_optical_depth(position, footprint);
    let phase = henyey_greenstein(0.65, cos_theta) * 0.85 + henyey_greenstein(-0.2, cos_theta) * 0.15;
    let sunlight = phase * exp(-optical)
        + henyey_greenstein(0.35, cos_theta) * 0.35 * exp(optical * -0.5)
        + henyey_greenstein(0.15, cos_theta) * 0.15 * exp(optical * -0.25)
        // Diffuse tail of higher scattering orders; restores sunlit cloud brightness when the forward
        // phase lobe points away from the camera.
        + exp(optical * -0.3) * 0.02;
    let sky = sky_irradiance(irradiance_table, table_sampler, shape(), r, sun_mu) * air.enabled / PI;
    return (to_sun * sunlight + sky * 0.5 * exp(optical * -0.35) + vec3(2e-5)) * air.sun_illuminance * 0.99;
}

// ---------------------------------------------------------------- the pass

struct Ray {
    /// Unit direction, body-fixed.
    rd: vec3<f32>,
    /// Distance along the ray to what was drawn; sky pixels read infinity.
    scene_distance: f32,
}

fn view_ray(uv: vec2<f32>, position: vec4<f32>) -> Ray {
    let ndc = vec2(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let near_point = air.view_from_clip * vec4(ndc, 1.0, 1.0);
    let view_direction = normalize(near_point.xyz / near_point.w);
    // Bevy's infinite reverse-Z: depth = near / view distance along −z; the sky is 0.
    let depth = textureLoad(depth_texture, vec2<i32>(position.xy), 0);
    let view_z = select(1e30, air.near / depth, depth > 0.0);
    var ray: Ray;
    ray.scene_distance = view_z / -view_direction.z;
    ray.rd = normalize((air.world_from_view * vec4(view_direction, 0.0)).xyz);
    return ray;
}

/// Full-resolution solar disc on sky pixels; the transport supplies its attenuation.
fn sun_disc(ray: Ray) -> vec3<f32> {
    let rd = ray.rd;
    let sun = air.sun_direction;
    let r0 = air.bottom_radius + air.camera_altitude;
    let mu0 = dot(air.camera_up, rd);
    let closest = -air.planet_center - rd * dot(-air.planet_center, rd);
    let above_ground = air.camera_altitude * (air.bottom_radius * 2.0 + air.camera_altitude);
    let ground_discriminant = select(r0 * r0 * (mu0 * mu0) - above_ground,
        air.bottom_radius * air.bottom_radius - dot(closest, closest), air.camera_altitude > 50000.0);
    let hits_ground = mu0 < 0.0 && ground_discriminant >= 0.0;
    let off_centre = length(cross(rd, sun)) / sin(SUN_ANGULAR_RADIUS);
    let sky = ray.scene_distance > 1e10 && !hits_ground && dot(rd, sun) > 0.0;
    let limb = sqrt(max(1.0 - off_centre * off_centre, 0.0));
    let disc_radiance = air.sun_illuminance / (PI * SUN_ANGULAR_RADIUS * SUN_ANGULAR_RADIUS);
    let disc = select(0.0, 1.0, sky) * select(0.0, 1.0, off_centre < 1.0) * (0.4 + limb * 0.6) * disc_radiance;
    return vec3(disc * air.sun_disc_enabled);
}

struct Medium {
    inscatter: vec3<f32>,
    transmittance: vec3<f32>,
}

struct ShellRoots {
    near: f32,
    far: f32,
    discriminant: f32,
}

/// Joint air/cloud transport along one view ray.
fn transport(ray: Ray) -> Medium {
    let rd = ray.rd;
    let sun = air.sun_direction;
    let bottom = air.bottom_radius;
    let top = air.top_radius;
    let r0 = bottom + air.camera_altitude;
    let mu0 = dot(air.camera_up, rd);
    let orbital = air.camera_altitude > 50000.0;
    let projection = select(r0 * mu0, dot(-air.planet_center, rd), orbital);
    let closest = -air.planet_center - rd * projection;
    let closest_squared = dot(closest, closest);
    // r0² − R², kept exact near the ground: altitude (2R + altitude).
    let above_ground = air.camera_altitude * (bottom * 2.0 + air.camera_altitude);
    let ground_discriminant = select(r0 * r0 * (mu0 * mu0) - above_ground, bottom * bottom - closest_squared, orbital);
    let hits_ground = mu0 < 0.0 && ground_discriminant >= 0.0;
    let ground_distance = -projection - sqrt(max(ground_discriminant, 0.0));
    let top_discriminant = select(r0 * r0 * (mu0 * mu0 - 1.0) + top * top, top * top - closest_squared, orbital);

    var medium: Medium;
    medium.transmittance = vec3(1.0);
    medium.inscatter = vec3(0.0);
    if !((air.enabled > 0.0 || air.clouds_enabled > 0.0) && top_discriminant > 0.0) {
        return medium;
    }
    let top_near = -projection - sqrt(top_discriminant);
    let top_far = -projection + sqrt(top_discriminant);
    let start = max(top_near, 0.0);
    // The lab's mix(1e30, ground, hit): as a + (b − a)·t that loses the ground distance to rounding
    // (1e30 − 1e30 = 0) on some GPUs, ending every ground ray at the camera. select picks exactly.
    let end = min(min(top_far, ray.scene_distance), select(1e30, ground_distance, hits_ground));
    if !(end > start) {
        return medium;
    }
    let cos_theta = dot(rd, sun);
    let phase_r = (3.0 / (16.0 * PI)) * (cos_theta * cos_theta + 1.0);
    let g = air.mie_anisotropy;
    let phase_m = ((3.0 / (8.0 * PI)) * ((1.0 - g * g) / (2.0 + g * g))) * (cos_theta * cos_theta + 1.0)
        / pow(1.0 + g * g - cos_theta * (2.0 * g), 1.5);
    let sun_mu0 = dot(air.camera_up, sun) * r0;
    let rd_sun = dot(rd, sun);
    // A ray can meet the cloud shell twice (near and far sides), with clear air between. Split at all
    // four shell crossings before integration, so orbit rays cannot skip a thin layer.
    let outer = shell_roots(air.sea_level + CLOUD_TOP, r0, mu0, orbital, projection, closest_squared, start, end);
    let inner = shell_roots(air.sea_level + CLOUD_BOTTOM, r0, mu0, orbital, projection, closest_squared, start, end);
    let cloud_ray = air.clouds_enabled > 0.0 && air.coverage > 0.0 && outer.discriminant > 0.0;
    var bounds = array<f32, 6>(start, select(end, outer.near, cloud_ray), select(end, inner.near, cloud_ray),
        select(end, inner.far, cloud_ray), select(end, outer.far, cloud_ray), end);
    var transmittance = vec3(1.0);
    var inscatter = vec3(0.0);
    for (var cloud_segment = 0; cloud_segment < 5; cloud_segment++) {
        let seg_from = bounds[cloud_segment];
        let seg_to = bounds[cloud_segment + 1];
        if !(seg_to > seg_from) {
            continue;
        }
        let midpoint = rd * ((seg_from + seg_to) * 0.5);
        let mid_height = cloud_height(midpoint) - air.sea_level;
        let in_cloud = cloud_ray && mid_height > CLOUD_BOTTOM && mid_height < CLOUD_TOP;
        let steps = select(VIEW_STEPS, select(24, 48, air.camera_altitude < 100000.0), in_cloud);
        let span = seg_to - seg_from;
        for (var view_step = 0; view_step < steps; view_step++) {
            if max(transmittance.x, max(transmittance.y, transmittance.z)) < 0.003 {
                break;
            }
            // Quadratic spacing retains short steps when the camera starts inside the cloud.
            let s0 = f32(view_step) / f32(steps);
            let s1 = (f32(view_step) + 1.0) / f32(steps);
            let sm = (f32(view_step) + 0.5) / f32(steps);
            let t = seg_from + span * (sm * sm);
            let dt = span * (s1 * s1 - s0 * s0);
            let position = rd * t;
            let height = max(cloud_height(position), 0.0);
            let r = bottom + height;
            let rayleigh = exp(height / -air.rayleigh_scale_height);
            let mie = exp(height / -air.mie_scale_height);
            let ozone = max(1.0 - abs(height - air.ozone_center_height) / (air.ozone_width / 2.0), 0.0);
            let rayleigh_scattering = air.rayleigh_scattering * rayleigh;
            var extinction = (rayleigh_scattering + mie * air.mie_extinction + air.ozone_absorption * ozone) * air.enabled;
            let scattering = rayleigh_scattering * phase_r + mie * air.mie_scattering * phase_m;
            let sun_mu = (sun_mu0 + rd_sun * t) / r;
            let all_scattering = rayleigh_scattering + mie * air.mie_scattering;
            var source = (scattering * sun_transmittance(transmittance_table, table_sampler, shape(), r, sun_mu)
                + all_scattering * multiple_scattering(multiple_table, table_sampler, shape(), r, sun_mu) * air.multiple_enabled)
                * air.sun_illuminance * air.enabled;
            if in_cloud {
                let footprint = max(dt, t / air.focal_pixels) * 2.0;
                let sigma = cloud_density(position, footprint) * CLOUD_EXTINCTION;
                if sigma > 0.000001 {
                    extinction += vec3(sigma);
                    source += cloud_source(position, rd, footprint) * sigma;
                }
            }
            let step = exp(-extinction * dt);
            // Joint air/cloud transport, in depth order. An air-only pass on top of clouds is insufficient.
            let integral = select(vec3(dt), (1.0 - step) / max(extinction, vec3(1e-10)), extinction > vec3(1e-10));
            inscatter += transmittance * source * integral;
            transmittance *= step;
        }
    }
    medium.transmittance = transmittance;
    medium.inscatter = inscatter;
    return medium;
}

fn shell_roots(shell_height: f32, r0: f32, mu0: f32, orbital: bool, projection: f32, closest_squared: f32,
    start: f32, end: f32) -> ShellRoots {
    let shell_radius = air.bottom_radius + shell_height;
    let h = air.camera_altitude - shell_height;
    let discriminant = select(r0 * r0 * (mu0 * mu0) - h * (shell_radius * 2.0 + h),
        shell_radius * shell_radius - closest_squared, orbital);
    let root = sqrt(max(discriminant, 0.0));
    return ShellRoots(clamp(-projection - root, start, end), clamp(-projection + root, start, end), discriminant);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let ray = view_ray(in.uv, in.position);
    let scene = textureLoad(scene_texture, vec2<i32>(in.position.xy), 0).rgb;
    let medium = transport(ray);
    return vec4((scene + sun_disc(ray)) * medium.transmittance + medium.inscatter, 1.0);
}
