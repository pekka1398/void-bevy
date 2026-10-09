use std::collections::VecDeque;

use glam::DVec3;

/// Barycentric vessel samples (t, x, v) at integrator step points, strictly increasing in time,
/// as `lab/orbit/src/orbit/Trajectory.ts`.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trajectory {
    samples: VecDeque<(f64, [f64; 6])>,
}

impl Trajectory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reserve before the propagator can append; charge actual deque capacity growth.
    pub(crate) fn reserve_prediction_sample(
        &mut self,
        context: &crate::PredictionContext,
    ) -> Result<(), crate::PredictionError> {
        if self.samples.len() == self.samples.capacity() {
            let additional = self.samples.capacity().max(4);
            context.reserve_bytes(additional.saturating_mul(size_of::<(f64, [f64; 6])>()))?;
            self.samples.reserve_exact(additional);
        }
        Ok(())
    }

    pub fn count(&self) -> usize {
        self.samples.len()
    }

    pub fn first_time(&self) -> f64 {
        self.samples.front().expect("trajectory: empty").0
    }

    pub fn last_time(&self) -> f64 {
        self.samples.back().expect("trajectory: empty").0
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }

    /// Appends the first six entries of a vessel state (position, velocity).
    pub fn append(&mut self, t: f64, state: &[f64]) {
        if let Some(&(last, _)) = self.samples.back() {
            assert!(t > last, "trajectory: time {t} does not follow {last}");
        }
        let six: [f64; 6] = state[..6]
            .try_into()
            .expect("a state has at least six entries");
        self.samples.push_back((t, six));
    }

    fn sample_at(&self, i: usize) -> &(f64, [f64; 6]) {
        self.samples
            .get(i)
            .unwrap_or_else(|| panic!("trajectory: index {i} of {}", self.samples.len()))
    }

    pub fn time(&self, i: usize) -> f64 {
        self.sample_at(i).0
    }

    pub fn position(&self, i: usize) -> DVec3 {
        let s = &self.sample_at(i).1;
        DVec3::new(s[0], s[1], s[2])
    }

    pub fn velocity(&self, i: usize) -> DVec3 {
        let s = &self.sample_at(i).1;
        DVec3::new(s[3], s[4], s[5])
    }

    /// Drop samples older than t, keeping the one sample that brackets t from below.
    pub fn trim_before(&mut self, t: f64) {
        while self.samples.len() > 1 && self.samples[1].0 <= t {
            self.samples.pop_front();
        }
    }

    /// Cubic Hermite interpolation of position and velocity inside the covered interval.
    pub fn sample(&self, t: f64) -> (DVec3, DVec3) {
        assert!(
            t >= self.first_time() && t <= self.last_time(),
            "trajectory sample: t = {t} outside [{}, {}]",
            self.first_time(),
            self.last_time()
        );
        if self.samples.len() == 1 {
            return (self.position(0), self.velocity(0));
        }
        // The last sample at or before t, kept below the final one.
        let lo =
            (self.samples.partition_point(|&(time, _)| time <= t) - 1).min(self.samples.len() - 2);
        let (t0, a) = &self.samples[lo];
        let (t1, b) = &self.samples[lo + 1];
        let h = t1 - t0;
        let s = (t - t0) / h;
        let (s2, s3) = (s * s, s * s * s);
        let (h00, h10, h01, h11) = (
            2.0 * s3 - 3.0 * s2 + 1.0,
            s3 - 2.0 * s2 + s,
            -2.0 * s3 + 3.0 * s2,
            s3 - s2,
        );
        let (g00, g10, g01, g11) = (
            (6.0 * s2 - 6.0 * s) / h,
            3.0 * s2 - 4.0 * s + 1.0,
            (-6.0 * s2 + 6.0 * s) / h,
            3.0 * s2 - 2.0 * s,
        );
        let p = |c: usize| h00 * a[c] + h10 * h * a[3 + c] + h01 * b[c] + h11 * h * b[3 + c];
        let v = |c: usize| g00 * a[c] + g10 * a[3 + c] + g01 * b[c] + g11 * b[3 + c];
        (DVec3::new(p(0), p(1), p(2)), DVec3::new(v(0), v(1), v(2)))
    }
}
