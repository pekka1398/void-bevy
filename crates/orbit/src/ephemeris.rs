use std::collections::BTreeMap;

use glam::DVec3;
use void_frames::{BodyId, BodyStates, FrameSource, SplitPosition, SystemId};

use crate::hermite::HermiteBasis;
use crate::system::{BuiltSystem, CelestialBody};

/// Yoshida (1990) 8th-order symmetric composition of the leapfrog, solution A.
/// Sequence w7 .. w1 w0 w1 .. w7; w0 = 1 - 2 sum(w1..w7).
const YOSHIDA8_W: [f64; 7] = [
    -1.615_823_741_500_97,
    -2.446_991_823_705_24,
    -0.716_989_419_708_120e-2,
    2.440_027_326_167_35,
    0.157_739_928_123_617,
    1.820_206_309_707_14,
    1.042_426_208_699_91,
];

/// The 15 substep weights, w7 .. w1 w0 w1 .. w7, as the lab's `YOSHIDA8_SEQUENCE`.
pub fn yoshida8_sequence() -> [f64; 15] {
    let w0 = 1.0 - 2.0 * YOSHIDA8_W.iter().fold(0.0, |sum, w| sum + w);
    let mut sequence = [0.0; 15];
    for (i, w) in YOSHIDA8_W.iter().enumerate() {
        sequence[6 - i] = *w;
        sequence[8 + i] = *w;
    }
    sequence[7] = w0;
    sequence
}

/// Floats per body per sample: position, velocity, acceleration.
const SAMPLE_STRIDE: usize = 9;

#[derive(Clone, Copy, Debug)]
pub struct EphemerisOptions {
    /// Fixed integration and sampling step, seconds.
    pub step_seconds: f64,
    /// Samples per storage chunk.
    pub chunk_steps: usize,
}

/// A step resolving the tightest Jacobi periapsis passage with the given samples per orbit.
pub fn suggested_step_seconds(bodies: &[CelestialBody], steps_per_orbit: f64) -> f64 {
    assert!(steps_per_orbit > 0.0, "steps per orbit {steps_per_orbit}");
    let tightest = bodies
        .iter()
        .filter_map(|b| Some(b.orbit_period_seconds? * b.periapsis_fraction?.powf(1.5)))
        .fold(f64::INFINITY, f64::min);
    assert!(
        tightest.is_finite(),
        "suggested step: the system has no orbiting bodies"
    );
    tightest / steps_per_orbit
}

/// Massive-body trajectories integrated as one N-body problem and queryable at any covered time
/// by quintic Hermite interpolation of (x, v, a) samples. Queries outside the covered interval
/// panic: callers must extend first.
pub struct Ephemeris {
    bodies: Vec<CelestialBody>,
    step_seconds: f64,
    epoch_seconds: f64,
    chunk_steps: usize,
    sequence: [f64; 15],
    gm: Vec<f64>,
    // Flat x, y, z per body, so the arithmetic runs in the orbit lab's order.
    q: Vec<f64>,
    q_compensation: Vec<f64>,
    v: Vec<f64>,
    a: Vec<f64>,
    chunks: BTreeMap<usize, Box<[f64]>>,
    /// Index of the newest sample; sample k is at epoch + k * step.
    last_step: usize,
    /// Index of the oldest retained sample.
    first_step: usize,
}

fn flatten(vectors: &[DVec3]) -> Vec<f64> {
    vectors.iter().flat_map(|v| v.to_array()).collect()
}

impl Ephemeris {
    pub fn new(system: &BuiltSystem, options: EphemerisOptions) -> Self {
        assert!(
            options.step_seconds > 0.0 && options.step_seconds.is_finite(),
            "ephemeris step {}",
            options.step_seconds
        );
        assert!(
            options.chunk_steps >= 2,
            "ephemeris chunk steps {}",
            options.chunk_steps
        );
        let n = system.bodies.len();
        assert!(
            system.positions.len() == n && system.velocities.len() == n,
            "state does not match body count"
        );
        let mut ephemeris = Self {
            bodies: system.bodies.clone(),
            step_seconds: options.step_seconds,
            epoch_seconds: 0.0,
            chunk_steps: options.chunk_steps,
            sequence: yoshida8_sequence(),
            gm: system.bodies.iter().map(|b| b.gm).collect(),
            q: flatten(&system.positions),
            q_compensation: vec![0.0; n * 3],
            v: flatten(&system.velocities),
            a: vec![0.0; n * 3],
            chunks: BTreeMap::new(),
            last_step: 0,
            first_step: 0,
        };
        ephemeris.compute_accelerations();
        ephemeris.store_sample(0);
        ephemeris
    }

    pub fn bodies(&self) -> &[CelestialBody] {
        &self.bodies
    }

    pub fn step_seconds(&self) -> f64 {
        self.step_seconds
    }

    pub fn start_time(&self) -> f64 {
        self.epoch_seconds + self.first_step as f64 * self.step_seconds
    }

    pub fn end_time(&self) -> f64 {
        self.epoch_seconds + self.last_step as f64 * self.step_seconds
    }

    /// Bytes held by retained samples.
    pub fn retained_bytes(&self) -> usize {
        self.chunks.len() * self.chunk_steps * self.bodies.len() * SAMPLE_STRIDE * size_of::<f64>()
    }

    /// Integrate forward until the covered interval contains t.
    pub fn extend_to(&mut self, t: f64) {
        assert!(t.is_finite(), "ephemeris extend to {t}");
        while self.end_time() < t {
            self.step();
            self.last_step += 1;
            self.store_sample(self.last_step);
        }
    }

    /// Release whole chunks strictly older than t.
    pub fn forget_before(&mut self, t: f64) {
        let step = ((t - self.epoch_seconds) / self.step_seconds).floor();
        // Always keep the newest interval so the covered range never degenerates to a point.
        let kept = step.min(self.last_step.saturating_sub(1) as f64).max(0.0) as usize;
        let first_kept_chunk = kept / self.chunk_steps;
        self.chunks.retain(|&key, _| key >= first_kept_chunk);
        self.first_step = self.first_step.max(first_kept_chunk * self.chunk_steps);
    }

    /// Barycentric states of every body at t.
    pub fn states_at(&self, t: f64, positions: &mut [DVec3], mut velocities: Option<&mut [DVec3]>) {
        assert!(
            positions.len() == self.bodies.len(),
            "positions for {} bodies",
            positions.len()
        );
        let (basis, left, right) = self.bracket(t);
        for (i, position) in positions.iter_mut().enumerate() {
            let (p, v) = Self::interpolate(&basis, left, right, i, velocities.is_some());
            *position = p;
            if let Some(out) = velocities.as_deref_mut() {
                out[i] = v;
            }
        }
    }

    pub fn positions_at(&self, t: f64, positions: &mut [DVec3]) {
        self.states_at(t, positions, None);
    }

    pub fn body_position(&self, body: usize, t: f64) -> DVec3 {
        assert!(
            body < self.bodies.len(),
            "body {body} is not in this ephemeris"
        );
        let (basis, left, right) = self.bracket(t);
        Self::interpolate(&basis, left, right, body, false).0
    }

    /// Acceleration of this coordinate origin, subtracted by the vessel propagator. A plain
    /// barycentric ephemeris is inertial; FrameEphemeris supplies the moving-origin term
    /// through EphemerisSource.
    pub fn frame_acceleration_at(&self, _t: f64) -> DVec3 {
        DVec3::ZERO
    }

    /// Total energy of the integrator's newest state, scaled by G (masses are GM).
    pub fn current_energy(&self) -> f64 {
        let (q, v, gm) = (&self.q, &self.v, &self.gm);
        let (mut kinetic, mut potential) = (0.0, 0.0);
        for i in 0..gm.len() {
            let (vx, vy, vz) = (v[i * 3], v[i * 3 + 1], v[i * 3 + 2]);
            kinetic += 0.5 * gm[i] * (vx * vx + vy * vy + vz * vz);
            for j in i + 1..gm.len() {
                let dx = q[j * 3] - q[i * 3];
                let dy = q[j * 3 + 1] - q[i * 3 + 1];
                let dz = q[j * 3 + 2] - q[i * 3 + 2];
                potential -= gm[i] * gm[j] / (dx * dx + dy * dy + dz * dz).sqrt();
            }
        }
        kinetic + potential
    }

    /// Total angular momentum (scaled by G) of the newest state.
    pub fn current_angular_momentum(&self) -> DVec3 {
        let (q, v) = (&self.q, &self.v);
        let mut l = DVec3::ZERO;
        for (i, m) in self.gm.iter().enumerate() {
            let (qx, qy, qz) = (q[i * 3], q[i * 3 + 1], q[i * 3 + 2]);
            let (vx, vy, vz) = (v[i * 3], v[i * 3 + 1], v[i * 3 + 2]);
            l += DVec3::new(
                m * (qy * vz - qz * vy),
                m * (qz * vx - qx * vz),
                m * (qx * vy - qy * vx),
            );
        }
        l
    }

    fn step(&mut self) {
        let h = self.step_seconds;
        for w in self.sequence {
            let half_kick = 0.5 * w * h;
            let drift = w * h;
            for (v, a) in self.v.iter_mut().zip(&self.a) {
                *v += a * half_kick;
            }
            for ((q, c), v) in self.q.iter_mut().zip(&mut self.q_compensation).zip(&self.v) {
                // Kahan-compensated drift: barycentric coordinates are large compared with a
                // single drift, so plain summation loses low bits.
                let y = v * drift - *c;
                let t = *q + y;
                *c = (t - *q) - y;
                *q = t;
            }
            self.compute_accelerations();
            for (v, a) in self.v.iter_mut().zip(&self.a) {
                *v += a * half_kick;
            }
        }
        assert!(
            self.q.iter().chain(&self.v).all(|x| x.is_finite()),
            "ephemeris: non-finite state after step {}",
            self.last_step + 1
        );
    }

    fn compute_accelerations(&mut self) {
        let (q, out, gm) = (&self.q, &mut self.a, &self.gm);
        out.fill(0.0);
        for i in 0..gm.len() {
            let (xi, yi, zi) = (q[i * 3], q[i * 3 + 1], q[i * 3 + 2]);
            for j in i + 1..gm.len() {
                let dx = q[j * 3] - xi;
                let dy = q[j * 3 + 1] - yi;
                let dz = q[j * 3 + 2] - zi;
                let r2 = dx * dx + dy * dy + dz * dz;
                assert!(
                    r2 > 0.0,
                    "ephemeris: bodies {} and {} coincide",
                    self.bodies[i].id,
                    self.bodies[j].id
                );
                let inv = 1.0 / (r2 * r2.sqrt());
                let (si, sj) = (gm[j] * inv, gm[i] * inv);
                out[i * 3] += dx * si;
                out[i * 3 + 1] += dy * si;
                out[i * 3 + 2] += dz * si;
                out[j * 3] -= dx * sj;
                out[j * 3 + 1] -= dy * sj;
                out[j * 3 + 2] -= dz * sj;
            }
        }
    }

    fn store_sample(&mut self, step: usize) {
        let per_sample = self.bodies.len() * SAMPLE_STRIDE;
        let chunk_index = step / self.chunk_steps;
        let chunk = self
            .chunks
            .entry(chunk_index)
            .or_insert_with(|| vec![f64::NAN; self.chunk_steps * per_sample].into_boxed_slice());
        let base = (step - chunk_index * self.chunk_steps) * per_sample;
        for i in 0..self.bodies.len() {
            let o = base + i * SAMPLE_STRIDE;
            chunk[o..o + 3].copy_from_slice(&self.q[i * 3..i * 3 + 3]);
            chunk[o + 3..o + 6].copy_from_slice(&self.v[i * 3..i * 3 + 3]);
            chunk[o + 6..o + 9].copy_from_slice(&self.a[i * 3..i * 3 + 3]);
        }
    }

    /// The samples of one step, all bodies.
    fn sample(&self, step: usize) -> &[f64] {
        let per_sample = self.bodies.len() * SAMPLE_STRIDE;
        let chunk_index = step / self.chunk_steps;
        let chunk = self
            .chunks
            .get(&chunk_index)
            .unwrap_or_else(|| panic!("ephemeris: sample {step} is not retained"));
        let base = (step - chunk_index * self.chunk_steps) * per_sample;
        &chunk[base..base + per_sample]
    }

    /// The basis and the samples either side of t.
    fn bracket(&self, t: f64) -> (HermiteBasis, &[f64], &[f64]) {
        assert!(
            self.last_step > 0,
            "ephemeris: no interval integrated yet; extend first"
        );
        assert!(
            t >= self.start_time() && t <= self.end_time(),
            "ephemeris: t = {t} outside covered [{}, {}]",
            self.start_time(),
            self.end_time()
        );
        let h = self.step_seconds;
        // t is inside [start, end]; the clamp only absorbs the closed right end and division rounding.
        let k = (((t - self.epoch_seconds) / h).floor() as usize)
            .clamp(self.first_step, self.last_step - 1);
        let s = (t - self.epoch_seconds - k as f64 * h) / h;
        (HermiteBasis::new(h, s), self.sample(k), self.sample(k + 1))
    }

    fn interpolate(
        basis: &HermiteBasis,
        left: &[f64],
        right: &[f64],
        body: usize,
        with_velocity: bool,
    ) -> (DVec3, DVec3) {
        let o = body * SAMPLE_STRIDE;
        let (mut p, mut v) = ([0.0; 3], [f64::NAN; 3]);
        for c in 0..3 {
            let (p0, p1) = (left[o + c], right[o + c]);
            let (v0, v1) = (left[o + 3 + c], right[o + 3 + c]);
            let (a0, a1) = (left[o + 6 + c], right[o + 6 + c]);
            p[c] = basis.position(p0, p1, v0, v1, a0, a1);
            if with_velocity {
                v[c] = basis.velocity(p0, p1, v0, v1, a0, a1);
            }
        }
        (DVec3::from_array(p), DVec3::from_array(v))
    }
}

impl BodyStates for Ephemeris {
    fn body_state(&self, body: BodyId, t: f64) -> (DVec3, DVec3) {
        assert!(
            body.0 < self.bodies.len(),
            "{body:?} is not in this ephemeris"
        );
        let (basis, left, right) = self.bracket(t);
        Self::interpolate(&basis, left, right, body.0, true)
    }
}

/// One star system at the galaxy's origin: the frame tree's view of a lone system.
impl FrameSource for Ephemeris {
    fn system_state(&self, system: SystemId, _t: f64) -> (SplitPosition, DVec3) {
        assert_eq!(
            system,
            SystemId(0),
            "a single-system ephemeris has only system 0"
        );
        (SplitPosition::ORIGIN, DVec3::ZERO)
    }
    fn body_in_system(&self, body: BodyId, t: f64) -> (DVec3, DVec3) {
        BodyStates::body_state(self, body, t)
    }
}

/// Source of body states in inertial axes, optionally with an accelerating origin.
///
/// Physics sees everything relative to one star system's barycentre, `origin_system`
/// (`BodyStates`, `states_at`, …). The frame tree sees each body relative to its own system and
/// every system in the galaxy (`FrameSource`); both views come from the same states.
pub trait EphemerisSource: BodyStates + FrameSource {
    /// Star systems in the galaxy, numbered from 0.
    fn system_count(&self) -> usize;
    /// The system a body belongs to.
    fn system_of(&self, body: usize) -> SystemId;
    /// The system whose barycentre the physics view is relative to.
    fn origin_system(&self) -> SystemId;
    /// Select a local physics view. Definitions, body indices and galaxy states never change.
    /// Sources without multiple local views reject an unsupported request explicitly.
    fn set_origin_system(&mut self, system: SystemId) {
        assert_eq!(
            system,
            self.origin_system(),
            "ephemeris: unsupported local view"
        );
    }
    /// Independent view over the same world; no celestial state copy or model replacement.
    /// Constant split offset from the origin-system barycentre for this local integration view.
    fn physics_offset(&self) -> SplitPosition {
        SplitPosition::ORIGIN
    }
    fn set_physics_offset(&mut self, offset: SplitPosition) {
        assert_eq!(
            offset,
            SplitPosition::ORIGIN,
            "ephemeris: unsupported split physics origin"
        );
    }
    fn local_view(&self, system: SystemId) -> Option<Box<dyn EphemerisSource>> {
        assert_eq!(
            system,
            self.origin_system(),
            "ephemeris: unsupported independent local view"
        );
        None
    }
    fn bodies(&self) -> &[CelestialBody];
    fn step_seconds(&self) -> f64;
    fn start_time(&self) -> f64;
    fn end_time(&self) -> f64;
    fn retained_bytes(&self) -> usize;
    fn extend_to(&mut self, t: f64);
    fn forget_before(&mut self, t: f64);
    fn states_at(&self, t: f64, positions: &mut [DVec3], velocities: Option<&mut [DVec3]>);
    fn positions_at(&self, t: f64, positions: &mut [DVec3]);
    fn body_position(&self, body: usize, t: f64) -> DVec3;
    fn frame_acceleration_at(&self, t: f64) -> DVec3;
}
impl EphemerisSource for Ephemeris {
    fn system_count(&self) -> usize {
        1
    }
    fn system_of(&self, body: usize) -> SystemId {
        assert!(
            body < self.bodies.len(),
            "body {body} is not in this ephemeris"
        );
        SystemId(0)
    }
    fn origin_system(&self) -> SystemId {
        SystemId(0)
    }
    fn bodies(&self) -> &[CelestialBody] {
        Ephemeris::bodies(self)
    }
    fn step_seconds(&self) -> f64 {
        Ephemeris::step_seconds(self)
    }
    fn start_time(&self) -> f64 {
        Ephemeris::start_time(self)
    }
    fn end_time(&self) -> f64 {
        Ephemeris::end_time(self)
    }
    fn retained_bytes(&self) -> usize {
        Ephemeris::retained_bytes(self)
    }
    fn extend_to(&mut self, t: f64) {
        Ephemeris::extend_to(self, t)
    }
    fn forget_before(&mut self, t: f64) {
        Ephemeris::forget_before(self, t)
    }
    fn states_at(&self, t: f64, positions: &mut [DVec3], velocities: Option<&mut [DVec3]>) {
        Ephemeris::states_at(self, t, positions, velocities)
    }
    fn positions_at(&self, t: f64, positions: &mut [DVec3]) {
        Ephemeris::positions_at(self, t, positions)
    }
    fn body_position(&self, body: usize, t: f64) -> DVec3 {
        Ephemeris::body_position(self, body, t)
    }
    fn frame_acceleration_at(&self, t: f64) -> DVec3 {
        Ephemeris::frame_acceleration_at(self, t)
    }
}
impl BodyStates for Box<dyn EphemerisSource> {
    fn body_state(&self, body: BodyId, t: f64) -> (DVec3, DVec3) {
        (**self).body_state(body, t)
    }
}
impl FrameSource for Box<dyn EphemerisSource> {
    fn system_state(&self, system: SystemId, t: f64) -> (SplitPosition, DVec3) {
        (**self).system_state(system, t)
    }
    fn body_in_system(&self, body: BodyId, t: f64) -> (DVec3, DVec3) {
        (**self).body_in_system(body, t)
    }
}
impl EphemerisSource for Box<dyn EphemerisSource> {
    fn system_count(&self) -> usize {
        (**self).system_count()
    }
    fn system_of(&self, body: usize) -> SystemId {
        (**self).system_of(body)
    }
    fn origin_system(&self) -> SystemId {
        (**self).origin_system()
    }
    fn set_origin_system(&mut self, system: SystemId) {
        (**self).set_origin_system(system)
    }
    fn physics_offset(&self) -> SplitPosition {
        (**self).physics_offset()
    }
    fn set_physics_offset(&mut self, offset: SplitPosition) {
        (**self).set_physics_offset(offset)
    }
    fn local_view(&self, system: SystemId) -> Option<Box<dyn EphemerisSource>> {
        (**self).local_view(system)
    }
    fn bodies(&self) -> &[CelestialBody] {
        (**self).bodies()
    }
    fn step_seconds(&self) -> f64 {
        (**self).step_seconds()
    }
    fn start_time(&self) -> f64 {
        (**self).start_time()
    }
    fn end_time(&self) -> f64 {
        (**self).end_time()
    }
    fn retained_bytes(&self) -> usize {
        (**self).retained_bytes()
    }
    fn extend_to(&mut self, t: f64) {
        (**self).extend_to(t)
    }
    fn forget_before(&mut self, t: f64) {
        (**self).forget_before(t)
    }
    fn states_at(&self, t: f64, positions: &mut [DVec3], velocities: Option<&mut [DVec3]>) {
        (**self).states_at(t, positions, velocities)
    }
    fn positions_at(&self, t: f64, positions: &mut [DVec3]) {
        (**self).positions_at(t, positions)
    }
    fn body_position(&self, body: usize, t: f64) -> DVec3 {
        (**self).body_position(body, t)
    }
    fn frame_acceleration_at(&self, t: f64) -> DVec3 {
        (**self).frame_acceleration_at(t)
    }
}
