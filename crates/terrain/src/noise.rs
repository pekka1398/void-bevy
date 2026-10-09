//! The two gradient noises the terrains use: integer hashes with wrapping i32 multiplies,
//! quintic fades.

/// Wrapping-multiply hash of an integer lattice point.
fn hash(x: f64, y: f64, z: f64) -> u32 {
    // Lattice coordinates are floored f64s well inside i32, as ToInt32 sees them.
    let (x, y, z) = (x as i32, y as i32, z as i32);
    let h =
        x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263) ^ z.wrapping_mul(1_442_695_041);
    let h = (h ^ ((h as u32) >> 13) as i32).wrapping_mul(1_274_126_177);
    (h ^ ((h as u32) >> 16) as i32) as u32
}

const GRADIENTS: [[f64; 3]; 12] = [
    [1.0, 1.0, 0.0],
    [-1.0, 1.0, 0.0],
    [1.0, -1.0, 0.0],
    [-1.0, -1.0, 0.0],
    [1.0, 0.0, 1.0],
    [-1.0, 0.0, 1.0],
    [1.0, 0.0, -1.0],
    [-1.0, 0.0, -1.0],
    [0.0, 1.0, 1.0],
    [0.0, -1.0, 1.0],
    [0.0, 1.0, -1.0],
    [0.0, -1.0, -1.0],
];

fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// The hills' noise: 3D Perlin noise in about [−1, 1].
pub fn perlin(x: f64, y: f64, z: f64) -> f64 {
    let (ix, iy, iz) = (x.floor(), y.floor(), z.floor());
    let (fx, fy, fz) = (x - ix, y - iy, z - iz);
    let corner = |dx: f64, dy: f64, dz: f64| {
        let g = GRADIENTS[hash(ix + dx, iy + dy, iz + dz) as usize % GRADIENTS.len()];
        g[0] * (fx - dx) + g[1] * (fy - dy) + g[2] * (fz - dz)
    };
    let (wx, wy, wz) = (fade(fx), fade(fy), fade(fz));
    lerp(
        lerp(
            lerp(corner(0.0, 0.0, 0.0), corner(1.0, 0.0, 0.0), wx),
            lerp(corner(0.0, 1.0, 0.0), corner(1.0, 1.0, 0.0), wx),
            wy,
        ),
        lerp(
            lerp(corner(0.0, 0.0, 1.0), corner(1.0, 0.0, 1.0), wx),
            lerp(corner(0.0, 1.0, 1.0), corner(1.0, 1.0, 1.0), wx),
            wy,
        ),
        wz,
    )
}

/// The layered planet's noise: gradient noise and its gradient in one pass, from
/// Quilez's analytic derivative of quintic-interpolated Perlin noise. In about [−1, 1].
pub fn noise_with_gradient(x: f64, y: f64, z: f64) -> (f64, [f64; 3]) {
    let (ix, iy, iz) = (x.floor(), y.floor(), z.floor());
    let (fx, fy, fz) = (x - ix, y - iy, z - iz);
    let ux = fx * fx * fx * (fx * (fx * 6.0 - 15.0) + 10.0);
    let uy = fy * fy * fy * (fy * (fy * 6.0 - 15.0) + 10.0);
    let uz = fz * fz * fz * (fz * (fz * 6.0 - 15.0) + 10.0);
    let dux = 30.0 * fx * fx * (fx * (fx - 2.0) + 1.0);
    let duy = 30.0 * fy * fy * (fy * (fy - 2.0) + 1.0);
    let duz = 30.0 * fz * fz * (fz * (fz - 2.0) + 1.0);
    let g = |dx: f64, dy: f64, dz: f64| GRADIENTS[hash(ix + dx, iy + dy, iz + dz) as usize % 12];
    let (a, b, c, dd) = (
        g(0.0, 0.0, 0.0),
        g(1.0, 0.0, 0.0),
        g(0.0, 1.0, 0.0),
        g(1.0, 1.0, 0.0),
    );
    let (e, f, gg, h) = (
        g(0.0, 0.0, 1.0),
        g(1.0, 0.0, 1.0),
        g(0.0, 1.0, 1.0),
        g(1.0, 1.0, 1.0),
    );
    let va = a[0] * fx + a[1] * fy + a[2] * fz;
    let vb = b[0] * (fx - 1.0) + b[1] * fy + b[2] * fz;
    let vc = c[0] * fx + c[1] * (fy - 1.0) + c[2] * fz;
    let vd = dd[0] * (fx - 1.0) + dd[1] * (fy - 1.0) + dd[2] * fz;
    let ve = e[0] * fx + e[1] * fy + e[2] * (fz - 1.0);
    let vf = f[0] * (fx - 1.0) + f[1] * fy + f[2] * (fz - 1.0);
    let vg = gg[0] * fx + gg[1] * (fy - 1.0) + gg[2] * (fz - 1.0);
    let vh = h[0] * (fx - 1.0) + h[1] * (fy - 1.0) + h[2] * (fz - 1.0);
    let (k1, k2, k3) = (vb - va, vc - va, ve - va);
    let (k4, k5, k6) = (va - vb - vc + vd, va - vc - ve + vg, va - vb - ve + vf);
    let k7 = -va + vb + vc - vd + ve - vf - vg + vh;
    let value = va
        + ux * k1
        + uy * k2
        + uz * k3
        + ux * uy * k4
        + uy * uz * k5
        + uz * ux * k6
        + ux * uy * uz * k7;
    let mut gradient = [0.0; 3];
    for (axis, out) in gradient.iter_mut().enumerate() {
        let (ga, gb, gc, gd) = (a[axis], b[axis], c[axis], dd[axis]);
        let (ge, gf, ggg, gh) = (e[axis], f[axis], gg[axis], h[axis]);
        let gradients = ga
            + ux * (gb - ga)
            + uy * (gc - ga)
            + uz * (ge - ga)
            + ux * uy * (ga - gb - gc + gd)
            + uy * uz * (ga - gc - ge + ggg)
            + uz * ux * (ga - gb - ge + gf)
            + ux * uy * uz * (-ga + gb + gc - gd + ge - gf - ggg + gh);
        let interpolation = match axis {
            0 => dux * (k1 + uy * k4 + uz * k6 + uy * uz * k7),
            1 => duy * (k2 + uz * k5 + ux * k4 + uz * ux * k7),
            _ => duz * (k3 + ux * k6 + uy * k5 + ux * uy * k7),
        };
        *out = gradients + interpolation;
    }
    (value, gradient)
}

/// The layered terrain's noise without its gradient.
pub fn noise(x: f64, y: f64, z: f64) -> f64 {
    noise_with_gradient(x, y, z).0
}

/// Fractal sum of `noise`, normalised by the amplitudes.
pub fn fbm(x: f64, y: f64, z: f64, octaves: u32) -> f64 {
    let (mut sum, mut amplitude, mut norm, mut f) = (0.0, 1.0, 0.0, 1.0);
    for _ in 0..octaves {
        sum += amplitude * noise(x * f, y * f, z * f);
        norm += amplitude;
        amplitude *= 0.5;
        f *= 2.0;
    }
    sum / norm
}

pub fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
