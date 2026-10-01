//! A star field fixed in the sky: directions and colours of one-pixel points far beyond the scene.

use glam::DVec3;
use void_math::{cos, pow, sin};

#[derive(Clone, Copy, Debug)]
pub struct StarFieldOptions {
    pub count: usize,
    pub seed: u32,
    /// Share of the stars crowded toward the galactic plane, which draws the Milky Way's band.
    pub band_share: f64,
    /// Spread of the band stars about the plane, radians.
    pub band_width: f64,
    /// Unit normal of the galactic plane, in the star field's own axes.
    pub galactic_pole: DVec3,
    /// Faintest magnitude drawn.
    pub faintest: f64,
}

pub const DEFAULT_STARS: StarFieldOptions = StarFieldOptions {
    count: 14_000,
    seed: 20260926,
    band_share: 0.55,
    band_width: 0.14,
    galactic_pole: DVec3::new(0.28, -0.46, 0.84),
    faintest: 6.5,
};

/// Distance the stars are drawn at, metres: past any atmosphere and still well inside the far plane.
pub const STAR_DISTANCE: f64 = 1e12;

/// Positions (at `STAR_DISTANCE`) and linear colours (magnitude-0 star = 1), as f32 like the lab's
/// buffers. Magnitudes follow the number of stars growing about 2.2× per magnitude; brightness falls
/// 10^(−0.25 m), flatter than the true 10^(−0.4 m), so a one-pixel faint star stays visible.
pub fn generate_stars(o: &StarFieldOptions) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    assert!(
        o.count > 0 && (0.0..=1.0).contains(&o.band_share) && o.band_width > 0.0,
        "generate_stars: {o:?}"
    );
    let pole_length = o.galactic_pole.length();
    assert!(
        (pole_length - 1.0).abs() <= 1e-2,
        "generate_stars: galactic_pole length {pole_length}"
    );
    let mut random = mulberry32(o.seed);
    let pole = three_normalize(o.galactic_pole);
    let a = three_normalize(pole.cross(if pole.x.abs() < 0.9 {
        DVec3::X
    } else {
        DVec3::Y
    }));
    let b = pole.cross(a);
    let mut positions = Vec::with_capacity(o.count);
    let mut colors = Vec::with_capacity(o.count);
    for _ in 0..o.count {
        let direction = if random() < o.band_share {
            // Near the plane: a normally distributed galactic latitude.
            let longitude = random() * 2.0 * std::f64::consts::PI;
            let latitude = gaussian(&mut random) * o.band_width;
            a * (cos(latitude) * cos(longitude))
                + b * (cos(latitude) * sin(longitude))
                + pole * sin(latitude)
        } else {
            let z = random() * 2.0 - 1.0;
            let phi = random() * 2.0 * std::f64::consts::PI;
            let s = (1.0 - z * z).sqrt();
            DVec3::new(s * cos(phi), s * sin(phi), z)
        };
        let p = three_normalize(direction) * STAR_DISTANCE;
        positions.push([p.x as f32, p.y as f32, p.z as f32]);
        // Inverse of N(<m) ∝ 10^(0.34 m), capped at the brightest real stars.
        let magnitude = (o.faintest + libm::log10(random().max(1e-12)) / 0.34).max(-1.5);
        let brightness = pow(10.0, -0.25 * magnitude);
        let [red, green, blue] = temperature_color(3200.0 + pow(random(), 1.6) * 9000.0);
        colors.push([
            (red * brightness) as f32,
            (green * brightness) as f32,
            (blue * brightness) as f32,
        ]);
    }
    (positions, colors)
}

/// three.js's Vector3.normalize: divide by the square root of the sum of squares.
fn three_normalize(v: DVec3) -> DVec3 {
    let length = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    v * (1.0 / if length == 0.0 { 1.0 } else { length })
}

/// Rough linear RGB of a star's colour, normalised so green is 1.
fn temperature_color(kelvin: f64) -> [f64; 3] {
    let t = ((kelvin - 3200.0) / 9000.0).clamp(0.0, 1.0);
    [1.25 - 0.5 * t, 1.0, 0.55 + 0.75 * t]
}

fn gaussian(random: &mut impl FnMut() -> f64) -> f64 {
    let u = random().max(1e-12);
    (-2.0 * libm::log(u)).sqrt() * cos(2.0 * std::f64::consts::PI * random())
}

fn mulberry32(seed: u32) -> impl FnMut() -> f64 {
    let mut a = seed;
    move || {
        a = a.wrapping_add(0x6d2b79f5);
        let mut t = a;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        f64::from(t ^ (t >> 14)) / 4294967296.0
    }
}
