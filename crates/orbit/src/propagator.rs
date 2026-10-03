//! One vessel through the ephemeris' gravity, as `lab/orbit/src/orbit/VesselPropagator.ts`.

use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;
use std::sync::Arc;

use glam::DVec3;

use crate::dopri5::Dopri5;
use crate::ephemeris::EphemerisSource;
use crate::gravity;
use crate::trajectory::Trajectory;

/// Per-step absolute error bounds. Mass needs none: its derivative is constant per leg.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tolerances {
    pub position_meters: f64,
    pub velocity_meters_per_second: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct VesselState {
    pub time: f64,
    pub position: DVec3,
    pub velocity: DVec3,
    pub mass_kg: f64,
}

/// Thrust direction law.
/// - inertial: a fixed barycentric direction.
/// - frenet: unit components along the trajectory's frame relative to a body: tangent = velocity
///   relative to the body (prograde), normal = orbit normal r × v, radial = tangent × normal.
/// - surface: near a rotating body, the normalised combination up × (local vertical) + prograde ×
///   (direction of the velocity over the ground, v − v_body − ω × r).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum AttitudeLaw {
    Inertial {
        direction: DVec3,
    },
    Frenet {
        reference_body: usize,
        tangent: f64,
        normal: f64,
        radial: f64,
    },
    Surface {
        reference_body: usize,
        up: f64,
        prograde: f64,
    },
}

/// Constant for the duration of one advance call.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThrustControl {
    pub thrust_newtons: f64,
    /// Isp × g0, m/s.
    pub exhaust_velocity: f64,
    /// Dry mass: reaching it inside a leg is a caller bug; legs end at fuel exhaustion.
    pub minimum_mass_kg: f64,
    pub attitude: AttitudeLaw,
}

/// Constant inertial force and independent propellant flow, for a multi-engine craft.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ForceControl {
    pub force: DVec3,
    pub mass_flow_kg_per_second: f64,
    pub minimum_mass_kg: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Control {
    Thrust(ThrustControl),
    Force(ForceControl),
}

impl Control {
    fn minimum_mass_kg(&self) -> f64 {
        match self {
            Control::Thrust(c) => c.minimum_mass_kg,
            Control::Force(c) => c.minimum_mass_kg,
        }
    }

    /// Validate a control before installing it in a simulation owner.
    pub fn assert_valid(&self, body_count: usize) {
        match self {
            Control::Thrust(c) => c.assert_valid(body_count),
            Control::Force(c) => {
                assert!(c.force.is_finite(), "non-finite force");
                assert!(
                    c.mass_flow_kg_per_second >= 0.0 && c.mass_flow_kg_per_second.is_finite(),
                    "invalid mass flow"
                );
                assert!(
                    c.minimum_mass_kg > 0.0 && c.minimum_mass_kg.is_finite(),
                    "invalid minimum mass"
                );
            }
        }
    }
}

impl ThrustControl {
    pub fn assert_valid(&self, body_count: usize) {
        assert!(
            self.thrust_newtons > 0.0 && self.thrust_newtons.is_finite(),
            "thrust {}",
            self.thrust_newtons
        );
        assert!(
            self.exhaust_velocity > 0.0 && self.exhaust_velocity.is_finite(),
            "exhaust velocity {}",
            self.exhaust_velocity
        );
        assert!(
            self.minimum_mass_kg > 0.0,
            "minimum mass {}",
            self.minimum_mass_kg
        );
        match self.attitude {
            AttitudeLaw::Inertial { direction } => {
                assert!(
                    (direction.length() - 1.0).abs() <= 1e-9,
                    "inertial attitude is not a unit vector"
                );
            }
            AttitudeLaw::Frenet {
                reference_body,
                tangent,
                normal,
                radial,
            } => {
                assert!(
                    reference_body < body_count,
                    "attitude body {reference_body}"
                );
                assert!(
                    (DVec3::new(tangent, normal, radial).length() - 1.0).abs() <= 1e-9,
                    "frenet attitude components are not a unit vector"
                );
            }
            AttitudeLaw::Surface {
                reference_body,
                up,
                prograde,
            } => {
                assert!(
                    reference_body < body_count,
                    "attitude body {reference_body}"
                );
                assert!(
                    up.is_finite() && prograde.is_finite() && up.hypot(prograde) > 0.0,
                    "surface attitude components {up}, {prograde}"
                );
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdvanceOutcome {
    Reached,
    Budget,
    Impact { body: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Impact {
    pub body: usize,
    pub time: f64,
}

const DIM: usize = 7;
const STEP_GROWTH_LIMIT: f64 = 5.0;
const STEP_SHRINK_LIMIT: f64 = 0.2;
const SAFETY: f64 = 0.9;
const INITIAL_STEP_SECONDS: f64 = 1.0;
const IMPACT_SCAN_SAMPLES: usize = 8;
const IMPACT_TIME_RESOLUTION_SECONDS: f64 = 1e-4;

/// Continuable propagation of one vessel: the state (position, velocity, mass), the integrator's
/// first-same-as-last derivative and the control it belongs to, and step-size memory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PropagationRun {
    pub time: f64,
    pub y: [f64; DIM],
    pub dy: [f64; DIM],
    /// The control `dy` was evaluated with; None until the first evaluation.
    derivative_control: Option<Option<Control>>,
    /// Last controller-proposed step, unclamped by leg ends.
    pub step_hint: f64,
    pub impact: Option<Impact>,
}

impl PropagationRun {
    pub fn new(state: VesselState) -> Self {
        let y = [
            state.position.x,
            state.position.y,
            state.position.z,
            state.velocity.x,
            state.velocity.y,
            state.velocity.z,
            state.mass_kg,
        ];
        assert!(
            y.iter().all(|v| v.is_finite()),
            "propagation run: non-finite initial state"
        );
        assert!(
            state.mass_kg > 0.0,
            "propagation run: mass {}",
            state.mass_kg
        );
        Self {
            time: state.time,
            y,
            dy: [0.0; DIM],
            derivative_control: None,
            step_hint: INITIAL_STEP_SECONDS,
            impact: None,
        }
    }

    pub fn state(&self) -> VesselState {
        let y = &self.y;
        VesselState {
            time: self.time,
            position: DVec3::new(y[0], y[1], y[2]),
            velocity: DVec3::new(y[3], y[4], y[5]),
            mass_kg: y[6],
        }
    }

    /// An independent copy with the same state and step memory, as the lab's `clone`: the
    /// derivative is re-evaluated and an impact is not carried over.
    pub fn restarted(&self) -> Self {
        let mut copy = Self::new(self.state());
        copy.step_hint = self.step_hint;
        copy
    }
}

/// An acceleration that depends on where the vessel is and how fast it is going, which a constant
/// `Control` cannot express: air, in practice. The integrator asks for it inside every stage, so an
/// implementation must be a pure function of the arguments — it may not accumulate anything.
///
/// Orbit knows nothing about atmospheres or vessel shapes; whoever supplies the source owns both.
pub trait AirSource: Send + Sync {
    /// Acceleration in the ephemeris frame, m/s², at `t` for a vessel of `mass_kg` passing
    /// `position` with `velocity`. Zero outside any atmosphere.
    fn acceleration(&self, t: f64, position: DVec3, velocity: DVec3, mass_kg: f64) -> DVec3;
}

/// Gravity and thrust as the integrator sees them; borrowed apart from the stepper.
struct Field {
    gm: Vec<f64>,
    radii: Vec<f64>,
    /// Per body: spin axis and `gravity::oblateness`.
    oblateness: Vec<(DVec3, f64)>,
    spin_rates: Vec<f64>,
    positions: Vec<DVec3>,
    velocities: Vec<DVec3>,
    control: Option<Control>,
    air: Option<Arc<dyn AirSource>>,
}

impl Field {
    /// Requires `positions` at t; includes the ephemeris origin's translational inertial term.
    fn gravity(&self, ephemeris: &dyn EphemerisSource, t: f64, x: f64, yy: f64, z: f64) -> DVec3 {
        let vessel = DVec3::new(x, yy, z);
        let mut a = DVec3::ZERO;
        for (i, p) in self.positions.iter().enumerate() {
            let (k, c) = self.oblateness[i];
            gravity::add_pull(&mut a, self.gm[i], c, k, vessel - *p);
        }
        a - ephemeris.frame_acceleration_at(t)
    }

    fn evaluate(
        &mut self,
        ephemeris: &dyn EphemerisSource,
        t: f64,
        y: &[f64; DIM],
        dy: &mut [f64; DIM],
    ) {
        let relative = matches!(
            self.control,
            Some(Control::Thrust(ThrustControl {
                attitude: AttitudeLaw::Frenet { .. } | AttitudeLaw::Surface { .. },
                ..
            }))
        );
        if relative {
            ephemeris.states_at(t, &mut self.positions, Some(&mut self.velocities));
        } else {
            ephemeris.positions_at(t, &mut self.positions);
        }
        let (x, yy, z) = (y[0], y[1], y[2]);
        let mut a = self.gravity(ephemeris, t, x, yy, z);
        dy[0] = y[3];
        dy[1] = y[4];
        dy[2] = y[5];
        match self.control {
            Some(Control::Force(c)) => {
                a.x += c.force.x / y[6];
                a.y += c.force.y / y[6];
                a.z += c.force.z / y[6];
                dy[6] = -c.mass_flow_kg_per_second;
            }
            Some(Control::Thrust(c)) => {
                let velocity = DVec3::new(y[3], y[4], y[5]);
                let direction = self.direction(&c.attitude, DVec3::new(x, yy, z), velocity);
                let accel = c.thrust_newtons / y[6];
                a.x += direction.x * accel;
                a.y += direction.y * accel;
                a.z += direction.z * accel;
                dy[6] = -c.thrust_newtons / c.exhaust_velocity;
            }
            None => dy[6] = 0.0,
        }
        if let Some(air) = &self.air {
            a += air.acceleration(t, DVec3::new(x, yy, z), DVec3::new(y[3], y[4], y[5]), y[6]);
        }
        dy[3] = a.x;
        dy[4] = a.y;
        dy[5] = a.z;
    }

    /// Requires `positions` and `velocities` at the same time, unless the law is inertial.
    fn direction(&self, law: &AttitudeLaw, p: DVec3, v: DVec3) -> DVec3 {
        match *law {
            AttitudeLaw::Inertial { direction } => direction,
            AttitudeLaw::Frenet {
                reference_body,
                tangent,
                normal,
                radial,
            } => {
                let r = p - self.positions[reference_body];
                let u = v - self.velocities[reference_body];
                let n = r.cross(u);
                let (u_len, n_len) = (u.length(), n.length());
                assert!(
                    u_len > 0.0 && n_len > 0.0,
                    "frenet attitude undefined: velocity relative to the reference body is zero or radial"
                );
                let t = u / u_len;
                let n = n / n_len;
                tangent * t + normal * n + radial * t.cross(n)
            }
            AttitudeLaw::Surface {
                reference_body,
                up,
                prograde,
            } => {
                let r = p - self.positions[reference_body];
                let (k, _) = self.oblateness[reference_body];
                let w = self.spin_rates[reference_body];
                let ground = v - self.velocities[reference_body] - w * k.cross(r);
                let g_len = ground.length();
                assert!(
                    prograde == 0.0 || g_len > 0.0,
                    "surface attitude undefined: no velocity over the ground"
                );
                let along = if prograde == 0.0 {
                    DVec3::ZERO
                } else {
                    prograde * ground / g_len
                };
                let d = up * r / r.length() + along;
                let d_len = d.length();
                assert!(
                    d_len > 0.0,
                    "surface attitude undefined: up and ground velocity cancel"
                );
                d / d_len
            }
        }
    }

    /// The body whose surface contains the point at t.
    fn body_containing(
        &mut self,
        ephemeris: &dyn EphemerisSource,
        t: f64,
        p: DVec3,
    ) -> Option<usize> {
        ephemeris.positions_at(t, &mut self.positions);
        self.positions
            .iter()
            .zip(&self.radii)
            .position(|(b, r)| (p - *b).length_squared() < r * r)
    }
}

pub struct VesselPropagator {
    pub tolerances: Tolerances,
    /// Accepted and rejected step counts since construction, for diagnostics.
    pub accepted_steps: u64,
    pub rejected_steps: u64,
    stepper: Dopri5<DIM>,
    field: Field,
    body_count: usize,
}

impl VesselPropagator {
    pub fn new(ephemeris: &dyn EphemerisSource, tolerances: Tolerances) -> Self {
        assert!(
            tolerances.position_meters > 0.0 && tolerances.velocity_meters_per_second > 0.0,
            "vessel propagator: tolerances {tolerances:?}"
        );
        let bodies = ephemeris.bodies();
        Self {
            tolerances,
            accepted_steps: 0,
            rejected_steps: 0,
            stepper: Dopri5::default(),
            field: Field {
                gm: bodies.iter().map(|b| b.gm).collect(),
                radii: bodies.iter().map(|b| b.radius_meters).collect(),
                oblateness: bodies
                    .iter()
                    .map(|b| (b.rotation.axis(), gravity::oblateness(b)))
                    .collect(),
                spin_rates: bodies
                    .iter()
                    .map(|b| TAU / b.rotation.period_seconds)
                    .collect(),
                positions: vec![DVec3::ZERO; bodies.len()],
                velocities: vec![DVec3::ZERO; bodies.len()],
                control: None,
                air: None,
            },
            body_count: bodies.len(),
        }
    }

    /// Add an acceleration the control cannot express, or None to fly through vacuum. It is in
    /// force for every later `advance` on this propagator.
    pub fn set_air_source(&mut self, air: Option<Arc<dyn AirSource>>) {
        self.field.air = air;
    }

    pub fn has_air_source(&self) -> bool {
        self.field.air.is_some()
    }

    /// Unit thrust direction the law gives for a state at t.
    pub fn thrust_direction(
        &mut self,
        ephemeris: &dyn EphemerisSource,
        law: &AttitudeLaw,
        t: f64,
        position: DVec3,
        velocity: DVec3,
    ) -> DVec3 {
        if let AttitudeLaw::Inertial { direction } = law {
            return *direction;
        }
        ephemeris.states_at(
            t,
            &mut self.field.positions,
            Some(&mut self.field.velocities),
        );
        self.field.direction(law, position, velocity)
    }

    /// Acceleration in the ephemeris frame at t: every body's point mass plus its J2, minus the
    /// coordinate origin's acceleration, as `advance` integrates. The ephemeris must cover t.
    pub fn gravity_at(
        &mut self,
        ephemeris: &dyn EphemerisSource,
        t: f64,
        position: DVec3,
    ) -> DVec3 {
        ephemeris.positions_at(t, &mut self.field.positions);
        self.field
            .gravity(ephemeris, t, position.x, position.y, position.z)
    }

    /// Propagate from `run.time` to `t_end` under a constant control (None = coast). Stops early at
    /// a surface impact or after `max_steps` accepted steps. Every accepted step end goes to `sink`.
    pub fn advance(
        &mut self,
        ephemeris: &mut dyn EphemerisSource,
        run: &mut PropagationRun,
        t_end: f64,
        max_steps: u64,
        mut sink: Option<&mut Trajectory>,
        control: Option<Control>,
    ) -> AdvanceOutcome {
        assert!(
            run.impact.is_none(),
            "vessel propagator: the run already ended in an impact"
        );
        assert!(
            t_end.is_finite() && t_end >= run.time,
            "vessel propagator: t_end {t_end} before {}",
            run.time
        );
        if let Some(c) = &control {
            c.assert_valid(self.body_count);
        }
        ephemeris.extend_to(t_end);
        let ephemeris = &*ephemeris;
        self.field.control = control;
        if run.derivative_control.is_none() {
            let inside = self.field.body_containing(
                ephemeris,
                run.time,
                DVec3::new(run.y[0], run.y[1], run.y[2]),
            );
            if let Some(body) = inside {
                panic!(
                    "vessel propagator: initial state is inside {}",
                    ephemeris.bodies()[body].id
                );
            }
        }
        // The stored derivative is the previous step's last stage, which is only still right if the
        // field has not moved under it. An air source is state-dependent and whoever owns it may
        // have retuned it between calls (the vessel's attitude, say), so re-evaluate then.
        if run.derivative_control != Some(control) || self.field.air.is_some() {
            self.field
                .evaluate(ephemeris, run.time, &run.y, &mut run.dy);
            run.derivative_control = Some(control);
        }
        let (mut y_next, mut dy_next) = ([0.0; DIM], [0.0; DIM]);
        let mut steps = 0;
        while run.time < t_end {
            if steps >= max_steps {
                return AdvanceOutcome::Budget;
            }
            let remaining = t_end - run.time;
            let last_step = run.step_hint >= remaining;
            let h = if last_step { remaining } else { run.step_hint };
            let field = &mut self.field;
            self.stepper.step(
                &mut |t, y, dy| field.evaluate(ephemeris, t, y, dy),
                run.time,
                &run.y,
                &run.dy,
                h,
                &mut y_next,
                &mut dy_next,
            );
            let err = self.error_norm();
            assert!(
                err.is_finite(),
                "vessel propagator: non-finite error estimate at t = {}",
                run.time
            );
            let factor = if err == 0.0 {
                STEP_GROWTH_LIMIT
            } else {
                STEP_GROWTH_LIMIT.min(STEP_SHRINK_LIMIT.max(SAFETY * err.powf(-0.2)))
            };
            if err > 1.0 {
                self.rejected_steps += 1;
                run.step_hint = h * factor.min(1.0);
                assert!(
                    run.step_hint > run.time.abs() * 1e-15,
                    "vessel propagator: step size underflow at t = {}",
                    run.time
                );
                continue;
            }
            if let Some(c) = &control {
                assert!(
                    y_next[6] >= c.minimum_mass_kg() * (1.0 - 1e-12),
                    "vessel propagator: mass {} fell below dry mass {}; the leg should have ended at fuel exhaustion",
                    y_next[6],
                    c.minimum_mass_kg()
                );
            }
            self.accepted_steps += 1;
            steps += 1;
            let t1 = if last_step { t_end } else { run.time + h };
            let candidate =
                self.scan_for_impact(ephemeris, run.time, &run.y, &run.dy, t1, &y_next, &dy_next);
            if let Some(body) = candidate
                && self.resolve_impact(ephemeris, run, body, h)
            {
                if let Some(sink) = sink.as_deref_mut() {
                    sink.append(run.time, &run.y);
                }
                return AdvanceOutcome::Impact { body };
            }
            run.time = t1;
            run.y = y_next;
            run.dy = dy_next;
            // A step clamped to the leg end says nothing about the natural step size.
            if !last_step {
                run.step_hint = h * factor;
            }
            if let Some(sink) = sink.as_deref_mut() {
                sink.append(run.time, &run.y);
            }
        }
        AdvanceOutcome::Reached
    }

    fn error_norm(&self) -> f64 {
        let e = &self.stepper.error;
        let (tp, tv) = (
            self.tolerances.position_meters,
            self.tolerances.velocity_meters_per_second,
        );
        [
            e[0].abs() / tp,
            e[1].abs() / tp,
            e[2].abs() / tp,
            e[3].abs() / tv,
            e[4].abs() / tv,
            e[5].abs() / tv,
        ]
        .into_iter()
        .fold(f64::NEG_INFINITY, f64::max)
    }

    /// Cheap screen of an accepted step: cubic Hermite vessel positions against interpolated body
    /// positions. The returned body is a candidate only.
    #[allow(clippy::too_many_arguments)]
    fn scan_for_impact(
        &mut self,
        ephemeris: &dyn EphemerisSource,
        t0: f64,
        y0: &[f64; DIM],
        dy0: &[f64; DIM],
        t1: f64,
        y1: &[f64; DIM],
        dy1: &[f64; DIM],
    ) -> Option<usize> {
        let h = t1 - t0;
        for j in 1..=IMPACT_SCAN_SAMPLES {
            let s = j as f64 / IMPACT_SCAN_SAMPLES as f64;
            let (s2, s3) = (s * s, s * s * s);
            let (h00, h10, h01, h11) = (
                2.0 * s3 - 3.0 * s2 + 1.0,
                s3 - 2.0 * s2 + s,
                -2.0 * s3 + 3.0 * s2,
                s3 - s2,
            );
            let c = |i: usize| h00 * y0[i] + h10 * h * dy0[i] + h01 * y1[i] + h11 * h * dy1[i];
            if let Some(body) =
                self.field
                    .body_containing(ephemeris, t0 + s * h, DVec3::new(c(0), c(1), c(2)))
            {
                return Some(body);
            }
        }
        None
    }

    /// Confirm a screened candidate with integrated states and bisect the first surface crossing.
    /// False when integration shows no crossing: the screen is only a filter, the integrator is
    /// the authority.
    fn resolve_impact(
        &mut self,
        ephemeris: &dyn EphemerisSource,
        run: &mut PropagationRun,
        body: usize,
        accepted_step: f64,
    ) -> bool {
        let radius = ephemeris.bodies()[body].radius_meters;
        let (mut probe_y, mut probe_dy) = ([0.0; DIM], [0.0; DIM]);
        let distance_at = |this: &mut Self,
                           tau: f64,
                           probe_y: &mut [f64; DIM],
                           probe_dy: &mut [f64; DIM]|
         -> f64 {
            let field = &mut this.field;
            this.stepper.step(
                &mut |t, y, dy| field.evaluate(ephemeris, t, y, dy),
                run.time,
                &run.y,
                &run.dy,
                tau,
                probe_y,
                probe_dy,
            );
            let p = ephemeris.body_position(body, run.time + tau);
            (DVec3::new(probe_y[0], probe_y[1], probe_y[2]) - p).length()
        };
        let mut lo = 0.0;
        let mut hi = accepted_step;
        if distance_at(self, hi, &mut probe_y, &mut probe_dy) >= radius {
            let samples = IMPACT_SCAN_SAMPLES * 4;
            let mut found = false;
            for j in 1..=samples {
                let tau = accepted_step * j as f64 / samples as f64;
                if distance_at(self, tau, &mut probe_y, &mut probe_dy) < radius {
                    hi = tau;
                    found = true;
                    break;
                }
                lo = tau;
            }
            if !found {
                return false;
            }
        }
        while hi - lo > IMPACT_TIME_RESOLUTION_SECONDS {
            let mid = 0.5 * (lo + hi);
            if distance_at(self, mid, &mut probe_y, &mut probe_dy) < radius {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        distance_at(self, hi, &mut probe_y, &mut probe_dy);
        run.time += hi;
        run.y = probe_y;
        run.dy = probe_dy;
        run.impact = Some(Impact {
            body,
            time: run.time,
        });
        true
    }
}
