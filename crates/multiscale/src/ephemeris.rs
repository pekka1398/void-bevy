//! Shared-world view in the nonrotating axes of one system's moving barycentre.
use crate::CoupledWorld;
use glam::DVec3;
use std::{cell::RefCell, rc::Rc};
use void_frames::{BodyId, BodyStates};
use void_orbit::{CelestialBody, EphemerisSource};

pub type SharedWorld = Rc<RefCell<CoupledWorld>>;

#[derive(Clone)]
pub struct FrameEphemeris {
    pub world: SharedWorld,
    pub system_index: usize,
    bodies: Vec<CelestialBody>,
}
impl FrameEphemeris {
    pub fn new(world: SharedWorld, system: &str) -> Self {
        let w = world.borrow();
        let system_index = w.system_index(system);
        let bodies = w.bodies.clone();
        drop(w);
        Self {
            world,
            system_index,
            bodies,
        }
    }
}
impl BodyStates for FrameEphemeris {
    fn body_state(&self, body: BodyId, t: f64) -> (DVec3, DVec3) {
        let world = self.world.borrow();
        let m = world
            .membership
            .get(body.0)
            .expect("frame ephemeris: unknown body");
        let states = world.at(t);
        let g = &states[m.system];
        let origin = &states[self.system_index];
        let p = g.body_position(m.local);
        let position = if m.system == self.system_index {
            p
        } else {
            g.origin.relative(&origin.origin) + p
        };
        (
            position,
            g.velocity - origin.velocity + g.body_velocity(m.local),
        )
    }
}
impl EphemerisSource for FrameEphemeris {
    fn bodies(&self) -> &[CelestialBody] {
        &self.bodies
    }
    fn step_seconds(&self) -> f64 {
        self.world.borrow().step_seconds
    }
    fn start_time(&self) -> f64 {
        self.world.borrow().start_time()
    }
    fn end_time(&self) -> f64 {
        self.world.borrow().time()
    }
    fn retained_bytes(&self) -> usize {
        self.world.borrow().retained_bytes()
    }
    fn extend_to(&mut self, t: f64) {
        assert!(
            self.world.borrow_mut().extend_to(t, 100_000),
            "frame ephemeris: extension exceeded step budget"
        );
    }
    fn forget_before(&mut self, _: f64) {
        panic!("frame ephemeris: prune the shared world explicitly")
    }
    fn frame_acceleration_at(&self, t: f64) -> DVec3 {
        self.world.borrow().at(t)[self.system_index].acceleration
    }
    fn states_at(&self, t: f64, positions: &mut [DVec3], velocities: Option<&mut [DVec3]>) {
        assert_eq!(
            positions.len(),
            self.bodies.len(),
            "frame ephemeris: wrong output size"
        );
        if let Some(v) = velocities.as_ref() {
            assert_eq!(
                v.len(),
                positions.len(),
                "frame ephemeris: wrong output size"
            );
        }
        let world = self.world.borrow();
        let states = world.at(t);
        let origin = &states[self.system_index];
        let mut velocities = velocities;
        for (i, m) in world.membership.iter().enumerate() {
            let g = &states[m.system];
            let p = g.body_position(m.local);
            positions[i] = if m.system == self.system_index {
                p
            } else {
                g.origin.relative(&origin.origin) + p
            };
            if let Some(v) = velocities.as_deref_mut() {
                v[i] = g.velocity - origin.velocity + g.body_velocity(m.local);
            }
        }
    }
    fn positions_at(&self, t: f64, positions: &mut [DVec3]) {
        self.states_at(t, positions, None);
    }
    fn body_position(&self, body: usize, t: f64) -> DVec3 {
        self.body_state(BodyId(body), t).0
    }
}
