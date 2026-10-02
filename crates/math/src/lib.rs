//! Math shared by the physics crates, matching V8 where the labs depend on the last digits:
//! `hypot` as V8's `Math.hypot`, and fdlibm's `tan`, `acos`, `asin`, `atan`, `atan2`, `exp`, which V8 also
//! uses. V8's `sin` and `cos` are neither fdlibm nor the system's (each differs on 1–3% of inputs),
//! so code using them agrees with the labs to an ulp, not bit for bit.

use glam::DVec3;

/// Euclidean norm as V8's `Math.hypot`: every value divided by the largest, squares summed with
/// Kahan compensation, then scaled back. It never overflows or underflows on the way, is more
/// accurate than sqrt(dot), and gives the LOD lab's exact results, which selection depends on
/// where two tiles' priorities differ in the last digits. Checked against Math.hypot on 1e6
/// random inputs from 1e-300 to 1e300 with no difference.
pub fn hypot<const N: usize>(values: [f64; N]) -> f64 {
    let values = values.map(f64::abs);
    if values.iter().any(|v| v.is_infinite()) {
        return f64::INFINITY;
    }
    if values.iter().any(|v| v.is_nan()) {
        return f64::NAN;
    }
    let max = values
        .iter()
        .fold(0.0, |m: f64, &v| if v > m { v } else { m });
    if max == 0.0 {
        return 0.0;
    }
    let (mut sum, mut compensation) = (0.0, 0.0);
    for v in values {
        let n = v / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * max
}

/// `hypot` of a vector's components.
pub fn length(v: DVec3) -> f64 {
    hypot([v.x, v.y, v.z])
}

// fdlibm, as V8's Math functions; the system libm behind std rounds some inputs differently.
pub use libm::{acos, asin, atan, atan2, exp, tan};

// fdlibm as well, but V8 differs from these: see the crate note.
pub use libm::{cos, pow, sin};
