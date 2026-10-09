//! The two gradient noises the terrains use, as the labs write them: integer hashes with
//! `Math.imul` (wrapping i32 multiplies here), quintic fades.

/// `Math.imul` hash of an integer lattice point.
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

/// lab/landing's hills noise (`HillsTerrain.ts`): 3D Perlin noise in about [−1, 1].
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

/// lab/scenery's noise (`LayeredTerrain.ts`): gradient noise and its gradient in one pass, from
/// Quilez's analytic derivative of quintic-interpolated Perlin noise. In about [−1, 1].
fn evaluate<const GRADIENT: bool, const SIMD: bool>(x: f64, y: f64, z: f64) -> (f64, [f64; 3]) {
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
    if !GRADIENT {
        return (value, gradient);
    }
    #[cfg(target_arch = "x86_64")]
    if SIMD {
        // SAFETY: only the runtime-checked AVX2 wrapper instantiates SIMD=true.
        return (value, unsafe {
            gradient_avx2(
                [a, b, c, dd, e, f, gg, h],
                [ux, uy, uz],
                [dux, duy, duz],
                [k1, k2, k3, k4, k5, k6, k7],
            )
        });
    }
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

/// Portable scalar reference for performance and exact-output comparisons.
pub fn noise_with_gradient_scalar(x: f64, y: f64, z: f64) -> (f64, [f64; 3]) {
    evaluate::<true, false>(x, y, z)
}

type GradientFn = fn(f64, f64, f64) -> (f64, [f64; 3]);

/// Exact f64 analytic gradient, with once-per-process hardware dispatch.
pub fn noise_with_gradient(x: f64, y: f64, z: f64) -> (f64, [f64; 3]) {
    static BACKEND: std::sync::OnceLock<GradientFn> = std::sync::OnceLock::new();
    BACKEND.get_or_init(|| {
        #[cfg(target_arch = "x86_64")]
        if std::is_x86_feature_detected!("avx2") {
            return checked_avx2;
        }
        noise_with_gradient_scalar
    })(x, y, z)
}

#[cfg(target_arch = "x86_64")]
fn checked_avx2(x: f64, y: f64, z: f64) -> (f64, [f64; 3]) {
    // SAFETY: this function pointer is chosen only after AVX2 runtime detection.
    unsafe { evaluate_avx2(x, y, z) }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn evaluate_avx2(x: f64, y: f64, z: f64) -> (f64, [f64; 3]) {
    evaluate::<true, true>(x, y, z)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn gradient_avx2(g: [[f64; 3]; 8], u: [f64; 3], du: [f64; 3], k: [f64; 7]) -> [f64; 3] {
    use std::arch::x86_64::*;
    // Each lane is one gradient axis; the fourth lane is unused. All arithmetic has
    // the scalar source's grouping, with separate multiply/add instructions (no FMA).
    let lane = |v: [f64; 3]| _mm256_setr_pd(v[0], v[1], v[2], 0.0);
    let [a, b, c, d, e, f, gg, h] = g.map(lane);
    let [ux, uy, uz] = u.map(|v| _mm256_set1_pd(v));
    macro_rules! add {
        ($a:expr,$b:expr) => {
            _mm256_add_pd($a, $b)
        };
    }
    macro_rules! sub {
        ($a:expr,$b:expr) => {
            _mm256_sub_pd($a, $b)
        };
    }
    macro_rules! mul {
        ($a:expr,$b:expr) => {
            _mm256_mul_pd($a, $b)
        };
    }
    let mut out = a;
    out = add!(out, mul!(ux, sub!(b, a)));
    out = add!(out, mul!(uy, sub!(c, a)));
    out = add!(out, mul!(uz, sub!(e, a)));
    out = add!(out, mul!(mul!(ux, uy), add!(sub!(sub!(a, b), c), d)));
    out = add!(out, mul!(mul!(uy, uz), add!(sub!(sub!(a, c), e), gg)));
    out = add!(out, mul!(mul!(uz, ux), add!(sub!(sub!(a, b), e), f)));
    let neg_a = _mm256_xor_pd(a, _mm256_set1_pd(-0.0));
    let k7g = add!(
        sub!(sub!(add!(sub!(add!(add!(neg_a, b), c), d), e), f), gg),
        h
    );
    out = add!(out, mul!(mul!(mul!(ux, uy), uz), k7g));
    let [uxs, uys, uzs] = u;
    let [k1, k2, k3, k4, k5, k6, k7] = k;
    let interpolation = [
        du[0] * (k1 + uys * k4 + uzs * k6 + uys * uzs * k7),
        du[1] * (k2 + uzs * k5 + uxs * k4 + uzs * uxs * k7),
        du[2] * (k3 + uxs * k6 + uys * k5 + uxs * uys * k7),
    ];
    out = add!(out, lane(interpolation));
    let mut result = [0.0; 4];
    // SAFETY: the unaligned store writes exactly four f64s to this stack array.
    unsafe {
        _mm256_storeu_pd(result.as_mut_ptr(), out);
    }
    [result[0], result[1], result[2]]
}

/// The layered terrain's noise without its gradient.
pub fn noise(x: f64, y: f64, z: f64) -> f64 {
    evaluate::<false, false>(x, y, z).0
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

#[cfg(test)]
mod tests {
    use super::*;
    fn assert_same(x: f64, y: f64, z: f64) {
        let reference = noise_with_gradient_scalar(x, y, z);
        let automatic = noise_with_gradient(x, y, z);
        assert_eq!(
            reference.0.to_bits(),
            automatic.0.to_bits(),
            "value at {x},{y},{z}"
        );
        assert_eq!(
            reference.1.map(f64::to_bits),
            automatic.1.map(f64::to_bits),
            "gradient at {x},{y},{z}"
        );
        assert_eq!(
            reference.0.to_bits(),
            noise(x, y, z).to_bits(),
            "value-only at {x},{y},{z}"
        );
    }
    #[test]
    fn gradient_dispatch_and_value_only_are_bitwise_scalar_equivalent() {
        // Exact lattice points, negative coordinates, signed zero and near-cell edges.
        for x in [
            -1024.0,
            -1.0000000000001,
            -1.0,
            -0.0,
            0.0,
            1e-14,
            0.9999999999999,
            1.0,
            8192.0,
        ] {
            for y in [-11.0, -0.0, 0.31, 1.0] {
                for z in [-0.99, 0.0, 13.7] {
                    assert_same(x, y, z);
                }
            }
        }
        let mut seed = 0x9b768dc123a467ef_u64;
        let mut random = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / ((1_u64 << 53) as f64) * 32768.0 - 16384.0
        };
        for _ in 0..20000 {
            assert_same(random(), random(), random());
        }
    }
}
