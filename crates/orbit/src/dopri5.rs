//! Dormand & Prince (1980) RK5(4)7M, as `lab/orbit/src/orbit/Dopri5.ts`.

const C2: f64 = 1.0 / 5.0;
const C3: f64 = 3.0 / 10.0;
const C4: f64 = 4.0 / 5.0;
const C5: f64 = 8.0 / 9.0;
const A21: f64 = 1.0 / 5.0;
const A31: f64 = 3.0 / 40.0;
const A32: f64 = 9.0 / 40.0;
const A41: f64 = 44.0 / 45.0;
const A42: f64 = -56.0 / 15.0;
const A43: f64 = 32.0 / 9.0;
const A51: f64 = 19372.0 / 6561.0;
const A52: f64 = -25360.0 / 2187.0;
const A53: f64 = 64448.0 / 6561.0;
const A54: f64 = -212.0 / 729.0;
const A61: f64 = 9017.0 / 3168.0;
const A62: f64 = -355.0 / 33.0;
const A63: f64 = 46732.0 / 5247.0;
const A64: f64 = 49.0 / 176.0;
const A65: f64 = -5103.0 / 18656.0;
const B1: f64 = 35.0 / 384.0;
const B3: f64 = 500.0 / 1113.0;
const B4: f64 = 125.0 / 192.0;
const B5: f64 = -2187.0 / 6784.0;
const B6: f64 = 11.0 / 84.0;
// 5th minus embedded 4th order weights.
const E1: f64 = 71.0 / 57600.0;
const E3: f64 = -71.0 / 16695.0;
const E4: f64 = 71.0 / 1920.0;
const E5: f64 = -17253.0 / 339200.0;
const E6: f64 = 22.0 / 525.0;
const E7: f64 = -1.0 / 40.0;

/// One explicit step with first-same-as-last reuse. Error control is the caller's policy.
pub struct Dopri5<const N: usize> {
    pub error: [f64; N],
    k2: [f64; N],
    k3: [f64; N],
    k4: [f64; N],
    k5: [f64; N],
    k6: [f64; N],
    tmp: [f64; N],
}

impl<const N: usize> Default for Dopri5<N> {
    fn default() -> Self {
        Self {
            error: [0.0; N],
            k2: [0.0; N],
            k3: [0.0; N],
            k4: [0.0; N],
            k5: [0.0; N],
            k6: [0.0; N],
            tmp: [0.0; N],
        }
    }
}

impl<const N: usize> Dopri5<N> {
    /// `k1` must hold f(t, y). Writes the 5th-order solution to `y_out`, f(t + h, y_out) to
    /// `k7_out`, and the local error estimate to `self.error`.
    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        f: &mut impl FnMut(f64, &[f64; N], &mut [f64; N]),
        t: f64,
        y: &[f64; N],
        k1: &[f64; N],
        h: f64,
        y_out: &mut [f64; N],
        k7_out: &mut [f64; N],
    ) {
        let Self {
            error,
            k2,
            k3,
            k4,
            k5,
            k6,
            tmp,
        } = self;
        for i in 0..N {
            tmp[i] = y[i] + h * A21 * k1[i];
        }
        f(t + C2 * h, tmp, k2);
        for i in 0..N {
            tmp[i] = y[i] + h * (A31 * k1[i] + A32 * k2[i]);
        }
        f(t + C3 * h, tmp, k3);
        for i in 0..N {
            tmp[i] = y[i] + h * (A41 * k1[i] + A42 * k2[i] + A43 * k3[i]);
        }
        f(t + C4 * h, tmp, k4);
        for i in 0..N {
            tmp[i] = y[i] + h * (A51 * k1[i] + A52 * k2[i] + A53 * k3[i] + A54 * k4[i]);
        }
        f(t + C5 * h, tmp, k5);
        for i in 0..N {
            tmp[i] =
                y[i] + h * (A61 * k1[i] + A62 * k2[i] + A63 * k3[i] + A64 * k4[i] + A65 * k5[i]);
        }
        f(t + h, tmp, k6);
        for i in 0..N {
            y_out[i] = y[i] + h * (B1 * k1[i] + B3 * k3[i] + B4 * k4[i] + B5 * k5[i] + B6 * k6[i]);
        }
        f(t + h, y_out, k7_out);
        for i in 0..N {
            error[i] = h
                * (E1 * k1[i] + E3 * k3[i] + E4 * k4[i] + E5 * k5[i] + E6 * k6[i] + E7 * k7_out[i]);
        }
    }
}
