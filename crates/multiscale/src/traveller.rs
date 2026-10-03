//! Frames that follow one system's barycentre, and a massless probe coasting between systems, as
//! the lab's `Frames.ts` and `Traveller.ts`.

use glam::DVec3;
use void_math::{hypot, pow};
use void_orbit::Dopri5;

use crate::CoupledWorld;
use void_frames::SplitPosition;

/// A position and velocity relative to system `frame`'s barycentre. Every frame has the same
/// axes; none rotates.
#[derive(Clone, Debug, PartialEq)]
pub struct FramedState {
    pub frame: String,
    pub position: SplitPosition,
    pub velocity: DVec3,
}

/// The world position and velocity of `state` at `t`.
pub fn absolute(world: &CoupledWorld, t: f64, state: &FramedState) -> (SplitPosition, DVec3) {
    let origin = &world.at(t)[world.system_index(&state.frame)];
    (
        origin.origin.compose(&state.position),
        origin.velocity + state.velocity,
    )
}

/// `state` re-expressed in `frame`: split to split, so nothing is lost across light-years.
pub fn reframe(world: &CoupledWorld, t: f64, state: &FramedState, frame: &str) -> FramedState {
    let (position, velocity) = absolute(world, t, state);
    let origin = &world.at(t)[world.system_index(frame)];
    FramedState {
        frame: frame.to_string(),
        position: position.difference(&origin.origin),
        velocity: velocity - origin.velocity,
    }
}

/// The system whose barycentre is nearest, with 5% hysteresis in favour of the current frame.
pub fn nearest_frame(world: &CoupledWorld, t: f64, state: &FramedState) -> String {
    let (p, _) = absolute(world, t, state);
    let origins = world.at(t);
    let (mut closest, mut distance) = (state.frame.clone(), f64::INFINITY);
    for (i, g) in origins.iter().enumerate() {
        let r = hypot(p.relative(&g.origin).to_array());
        if r < distance {
            distance = r;
            closest = world.ids[i].clone();
        }
    }
    let current = p.relative(&origins[world.system_index(&state.frame)].origin);
    if hypot(current.to_array()) > 1.05 * distance {
        closest
    } else {
        state.frame.clone()
    }
}

/// A change of frame: when, from, to, and how far the absolute state moved in it (should be
/// micrometres and nanometres per second at most).
#[derive(Clone, Debug, PartialEq)]
pub struct FrameEvent {
    pub time: f64,
    pub from: String,
    pub to: String,
    pub position_jump: f64,
    pub velocity_jump: f64,
}

/// A massless coasting probe. No fuel model, relativity or jump between systems.
pub struct Traveller {
    pub state: FramedState,
    pub time: f64,
    /// Why it stopped: a body's surface was reached.
    pub terminal: Option<String>,
    pub events: Vec<FrameEvent>,
    pub steps: u64,
    pub max_step: f64,
    step_hint: f64,
    integrator: Dopri5<6>,
}

impl Traveller {
    /// Default largest step: 5 days.
    pub const MAX_STEP: f64 = 5.0 * 86400.0;

    pub fn new(world: &CoupledWorld, time: f64, state: FramedState, max_step: f64) -> Self {
        world.system_index(&state.frame);
        world.at(time);
        assert!(
            state.velocity.is_finite(),
            "traveller velocity: non-finite vector"
        );
        assert!(
            max_step > 0.0 && max_step.is_finite(),
            "Traveller: invalid step"
        );
        let state = FramedState {
            position: state.position.translate(DVec3::ZERO),
            ..state
        };
        absolute(world, time, &state);
        Self {
            state,
            time,
            terminal: None,
            events: vec![],
            steps: 0,
            max_step,
            step_hint: 1.0,
            integrator: Dopri5::default(),
        }
    }

    pub fn set_frame(&mut self, world: &CoupledWorld, frame: &str) {
        let before = absolute(world, self.time, &self.state);
        let from = self.state.frame.clone();
        let next = reframe(world, self.time, &self.state, frame);
        let after = absolute(world, self.time, &next);
        let d = before.0.relative(&after.0);
        let v = before.1 - after.1;
        self.state = next;
        if from != frame {
            self.events.push(FrameEvent {
                time: self.time,
                from,
                to: frame.to_string(),
                position_jump: hypot(d.to_array()),
                velocity_jump: hypot(v.to_array()),
            });
        }
    }

    pub fn position(&self, world: &CoupledWorld) -> SplitPosition {
        absolute(world, self.time, &self.state).0
    }

    /// Coast toward `target` with at most `budget` integrator tries; false if not reached (the
    /// progress made is kept). Each step starts at a split anchor with zero float64 displacement;
    /// steps shrink so none crosses a quarter of the way to any body's surface.
    pub fn advance_to(&mut self, world: &mut CoupledWorld, target: f64, budget: u64) -> bool {
        assert!(
            target.is_finite() && target >= self.time,
            "Traveller: invalid target/budget"
        );
        if self.terminal.is_some() {
            return false;
        }
        let mut used = 0;
        while self.time < target && used < budget {
            let base = self.state.clone();
            let frame = world.system_index(&base.frame);
            let anchor = base.position;
            let mut h = self.step_hint.min(self.max_step).min(target - self.time);
            let (start_position, start_velocity) = absolute(world, self.time, &base);
            let sources = world.at(self.time);
            for (i, body) in world.bodies.iter().enumerate() {
                let d = world.body_position(i, &sources).relative(&start_position);
                let r = hypot(d.to_array());
                let clearance = r - body.radius_meters;
                let member = world.membership[i];
                let g = &sources[member.system];
                let v = start_velocity - (g.velocity + g.body_velocity(member.local));
                let speed = hypot(v.to_array());
                let resolution = 1.0_f64.max(speed * self.time.abs() * f64::EPSILON * 16.0);
                if clearance <= resolution {
                    self.terminal = Some(format!(
                        "{}: surface approach stopped (point probe)",
                        body.name
                    ));
                    return false;
                }
                if speed > 0.0 {
                    h = h.min(clearance / (4.0 * speed));
                }
            }
            assert!(self.time + h > self.time, "Traveller: time step underflow");
            assert!(
                world.extend_to(self.time + h, 100_000),
                "Traveller: world step budget exceeded"
            );
            let world_ref: &CoupledWorld = world;
            let y = [
                0.0,
                0.0,
                0.0,
                base.velocity.x,
                base.velocity.y,
                base.velocity.z,
            ];
            let mut derivative = |t: f64, q: &[f64; 6], f: &mut [f64; 6]| {
                let state = world_ref.at(t);
                let origin = &state[frame];
                let p = origin
                    .origin
                    .compose(&anchor.translate(DVec3::new(q[0], q[1], q[2])));
                let a = world_ref.gravity_in(&state, &p) - origin.acceleration;
                *f = [q[3], q[4], q[5], a.x, a.y, a.z];
            };
            let mut dy = [0.0; 6];
            let (mut out, mut end_derivative) = ([0.0; 6], [0.0; 6]);
            derivative(self.time, &y, &mut dy);
            self.integrator.step(
                &mut derivative,
                self.time,
                &y,
                &dy,
                h,
                &mut out,
                &mut end_derivative,
            );
            used += 1;
            let mut error: f64 = 0.0;
            for (i, e) in self.integrator.error.iter().enumerate() {
                error = error.max(e.abs() / if i < 3 { 0.1 } else { 1e-6 });
            }
            assert!(
                error.is_finite() && out.iter().all(|v| v.is_finite()),
                "Traveller: non-finite integration"
            );
            let factor = if error == 0.0 {
                5.0
            } else {
                5.0_f64.min(0.2_f64.max(0.9 * pow(error, -0.2)))
            };
            self.step_hint = h * factor;
            if error > 1.0 {
                continue;
            }
            self.state = FramedState {
                frame: base.frame,
                position: anchor.translate(DVec3::new(out[0], out[1], out[2])),
                velocity: DVec3::new(out[3], out[4], out[5]),
            };
            self.time += h;
            self.steps += 1;
            let nearest = nearest_frame(world, self.time, &self.state);
            self.set_frame(world, &nearest);
        }
        self.time >= target
    }
}
