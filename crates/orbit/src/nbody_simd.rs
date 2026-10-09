//! Four independent pair evaluations; accumulation still follows lexicographic (i,j).
//! Deliberately no fused operations, approximate reciprocals, or horizontal reduction.
use std::arch::x86_64::*;

#[target_feature(enable = "avx2")]
pub(crate) unsafe fn accelerations(q: &[f64], gm: &[f64], out: &mut [f64]) {
    // SAFETY: caller checks AVX2; bounded blocks below guarantee every load/store is in range.
    unsafe {
        out.fill(0.0);
        let n = gm.len();
        for i in 0..gm.len() {
            let xi = _mm256_set1_pd(q[i]);
            let yi = _mm256_set1_pd(q[n + i]);
            let zi = _mm256_set1_pd(q[2 * n + i]);
            let mut ai = [out[3 * i], out[3 * i + 1], out[3 * i + 2]];
            let mut j = i + 1;
            while j + 4 <= gm.len() {
                let x = _mm256_loadu_pd(q.as_ptr().add(j));
                let y = _mm256_loadu_pd(q.as_ptr().add(n + j));
                let z = _mm256_loadu_pd(q.as_ptr().add(2 * n + j));
                let dx = _mm256_sub_pd(x, xi);
                let dy = _mm256_sub_pd(y, yi);
                let dz = _mm256_sub_pd(z, zi);
                let r2 = _mm256_add_pd(
                    _mm256_add_pd(_mm256_mul_pd(dx, dx), _mm256_mul_pd(dy, dy)),
                    _mm256_mul_pd(dz, dz),
                );
                assert_eq!(
                    _mm256_movemask_pd(_mm256_cmp_pd(r2, _mm256_setzero_pd(), _CMP_GT_OQ)),
                    15,
                    "ephemeris: coincident/non-finite pair in ({i}, {j}..{})",
                    j + 4
                );
                let inv = _mm256_div_pd(_mm256_set1_pd(1.0), _mm256_mul_pd(r2, _mm256_sqrt_pd(r2)));
                let si = _mm256_mul_pd(_mm256_loadu_pd(gm.as_ptr().add(j)), inv);
                let sj = _mm256_mul_pd(_mm256_set1_pd(gm[i]), inv);
                let mut a = [[0.0; 4]; 6];
                for (dst, value) in a.iter_mut().zip([
                    _mm256_mul_pd(dx, si),
                    _mm256_mul_pd(dy, si),
                    _mm256_mul_pd(dz, si),
                    _mm256_mul_pd(dx, sj),
                    _mm256_mul_pd(dy, sj),
                    _mm256_mul_pd(dz, sj),
                ]) {
                    _mm256_storeu_pd(dst.as_mut_ptr(), value);
                }
                for lane in 0..4 {
                    for c in 0..3 {
                        ai[c] += a[c][lane];
                        out[3 * (j + lane) + c] -= a[c + 3][lane];
                    }
                }
                j += 4;
            }
            for j in j..gm.len() {
                let dx = q[j] - q[i];
                let dy = q[n + j] - q[n + i];
                let dz = q[2 * n + j] - q[2 * n + i];
                let r2 = dx * dx + dy * dy + dz * dz;
                assert!(r2 > 0.0, "ephemeris: bodies {i} and {j} coincide");
                let inv = 1.0 / (r2 * r2.sqrt());
                let (si, sj) = (gm[j] * inv, gm[i] * inv);
                for (c, d) in [dx, dy, dz].into_iter().enumerate() {
                    ai[c] += d * si;
                    out[3 * j + c] -= d * sj;
                }
            }
            out[3 * i..3 * i + 3].copy_from_slice(&ai);
        }
    }
}
