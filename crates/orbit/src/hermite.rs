//! Quintic Hermite interpolation of (x, v, a) samples, as `lab/orbit/src/orbit/Hermite.ts`.

#[derive(Clone, Copy, Debug)]
pub struct HermiteBasis {
    h: f64,
    hh: f64,
    h1: f64,
    h2: f64,
    h3: f64,
    h4: f64,
    h5: f64,
    d1: f64,
    d2: f64,
    d3: f64,
    d4: f64,
    d5: f64,
}

impl HermiteBasis {
    /// Step h and the fraction s in [0, 1] of the way through it.
    pub fn new(h: f64, s: f64) -> Self {
        let (s2, s3) = (s * s, s * s * s);
        let (s4, s5) = (s3 * s, s3 * s * s);
        Self {
            h,
            hh: h * h,
            h1: s - 6.0 * s3 + 8.0 * s4 - 3.0 * s5,
            h2: 0.5 * s2 - 1.5 * s3 + 1.5 * s4 - 0.5 * s5,
            h3: 0.5 * s3 - s4 + 0.5 * s5,
            h4: -4.0 * s3 + 7.0 * s4 - 3.0 * s5,
            h5: 10.0 * s3 - 15.0 * s4 + 6.0 * s5,
            d1: 1.0 - 18.0 * s2 + 32.0 * s3 - 15.0 * s4,
            d2: s - 4.5 * s2 + 6.0 * s3 - 2.5 * s4,
            d3: 1.5 * s2 - 4.0 * s3 + 2.5 * s4,
            d4: -12.0 * s2 + 28.0 * s3 - 15.0 * s4,
            d5: 30.0 * s2 - 60.0 * s3 + 30.0 * s4,
        }
    }

    /// One coordinate: samples (p, v, a) at the start (0) and end (1) of the step.
    pub fn position(&self, p0: f64, p1: f64, v0: f64, v1: f64, a0: f64, a1: f64) -> f64 {
        p0 + self.h5 * (p1 - p0)
            + self.h * (self.h1 * v0 + self.h4 * v1)
            + self.hh * (self.h2 * a0 + self.h3 * a1)
    }

    pub fn velocity(&self, p0: f64, p1: f64, v0: f64, v1: f64, a0: f64, a1: f64) -> f64 {
        self.d5 * (p1 - p0) / self.h
            + self.d1 * v0
            + self.d4 * v1
            + self.h * (self.d2 * a0 + self.d3 * a1)
    }
}
