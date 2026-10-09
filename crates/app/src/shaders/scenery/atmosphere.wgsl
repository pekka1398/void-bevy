// Atmosphere table lookups: sunlight through the air, multiple scattering and
// sky irradiance, from the tables void-scenery builds on the CPU.
#define_import_path void::atmosphere

const TRANSMITTANCE_WIDTH: f32 = 256.0;
const TRANSMITTANCE_HEIGHT: f32 = 64.0;
const MULTIPLE_SCATTERING_SIZE: f32 = 32.0;
const IRRADIANCE_WIDTH: f32 = 32.0;
const IRRADIANCE_HEIGHT: f32 = 16.0;
/// The Sun seen from Aurelia's distance (1 AU): angular radius, radians.
const SUN_ANGULAR_RADIUS: f32 = 0.004654;
const PI: f32 = 3.141592653589793;

/// Bottom and top radius of the air and the distance to its horizon from the bottom, metres.
struct AtmosphereShape {
    bottom: f32,
    top: f32,
    horizon: f32,
}

/// Transmittance from radius r toward zenith cosine mu, to space. Zero once the ground sphere hides
/// the sun, softened over the sun's radius.
fn sun_transmittance(table: texture_2d<f32>, table_sampler: sampler, a: AtmosphereShape, r: f32, mu: f32) -> vec3<f32> {
    let radius = min(r, a.top);
    let r2 = radius * radius;
    let rho = sqrt(max(r2 - a.bottom * a.bottom, 0.0));
    let discriminant = r2 * (mu * mu - 1.0) + a.top * a.top;
    let d = max(-radius * mu + sqrt(max(discriminant, 0.0)), 0.0);
    let d_min = a.top - radius;
    let d_max = rho + a.horizon;
    let x = (d - d_min) / max(d_max - d_min, 1e-3);
    let y = rho / a.horizon;
    // Coordinates are 0–1 between the first and last texel centres.
    let uv = vec2(
        x * ((TRANSMITTANCE_WIDTH - 1.0) / TRANSMITTANCE_WIDTH) + 0.5 / TRANSMITTANCE_WIDTH,
        y * ((TRANSMITTANCE_HEIGHT - 1.0) / TRANSMITTANCE_HEIGHT) + 0.5 / TRANSMITTANCE_HEIGHT,
    );
    let horizon_mu = -sqrt(max(1.0 - a.bottom * a.bottom / r2, 0.0));
    let visible = smoothstep(horizon_mu - SUN_ANGULAR_RADIUS, horizon_mu + SUN_ANGULAR_RADIUS, mu);
    // Marching branches differ per pixel: implicit texture derivatives are undefined there.
    return textureSampleLevel(table, table_sampler, uv, 0.0).rgb * visible;
}

/// (height, sun zenith cosine) coordinates of the multiple-scattering and irradiance tables.
fn sky_table_uv(a: AtmosphereShape, r: f32, sun_mu: f32, width: f32, height: f32) -> vec2<f32> {
    let depth = a.top - a.bottom;
    let x = (clamp(sun_mu, -1.0, 1.0) + 1.0) * 0.5;
    let y = sqrt(clamp(r - a.bottom, 0.0, depth) / depth);
    return vec2(x * ((width - 1.0) / width) + 0.5 / width, y * ((height - 1.0) / height) + 0.5 / height);
}

/// Light scattered twice or more, per unit scattering coefficient and sun illuminance.
fn multiple_scattering(table: texture_2d<f32>, table_sampler: sampler, a: AtmosphereShape, r: f32, sun_mu: f32) -> vec3<f32> {
    let uv = sky_table_uv(a, r, sun_mu, MULTIPLE_SCATTERING_SIZE, MULTIPLE_SCATTERING_SIZE);
    return textureSampleLevel(table, table_sampler, uv, 0.0).rgb;
}

/// Sky irradiance (without the sun's beam) on level ground at radius r, per unit sun illuminance.
fn sky_irradiance(table: texture_2d<f32>, table_sampler: sampler, a: AtmosphereShape, r: f32, sun_mu: f32) -> vec3<f32> {
    let uv = sky_table_uv(a, r, sun_mu, IRRADIANCE_WIDTH, IRRADIANCE_HEIGHT);
    return textureSampleLevel(table, table_sampler, uv, 0.0).rgb;
}
