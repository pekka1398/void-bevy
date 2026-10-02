use glam::DVec3;
use std::cell::RefCell;
use void_landing::{ContactFrame, FrameState};
use void_orbit::{
    AdvanceOutcome, Ephemeris, PropagationRun, Tolerances, VesselPropagator, VesselState,
};

/// Nonrotating contact frame following a coasting orbit. The contact owner prepares
/// the next fixed-step origin before stepping; acceleration queries stay read-only.
pub struct FreeFallFrame {
    propagator: RefCell<VesselPropagator>,
    run: PropagationRun,
    previous: VesselState,
}
impl FreeFallFrame {
    pub fn new(
        ephemeris: &Ephemeris,
        tolerances: Tolerances,
        time: f64,
        anchor: FrameState,
    ) -> Self {
        let state = VesselState {
            time,
            position: anchor.position,
            velocity: anchor.velocity,
            mass_kg: 1.0,
        };
        Self {
            propagator: RefCell::new(VesselPropagator::new(ephemeris, tolerances)),
            run: PropagationRun::new(state),
            previous: state,
        }
    }
    pub fn advance_origin(&mut self, ephemeris: &mut Ephemeris, time: f64) {
        assert!(time >= self.run.time, "free fall: time reversed");
        if time == self.run.time {
            return;
        }
        self.previous = self.run.state();
        let outcome =
            self.propagator
                .get_mut()
                .advance(ephemeris, &mut self.run, time, 100_000, None, None);
        assert_eq!(
            outcome,
            AdvanceOutcome::Reached,
            "free fall origin propagation failed"
        );
    }
    pub fn origin(&self, time: f64) -> FrameState {
        let s = if time == self.run.time {
            self.run.state()
        } else {
            assert_eq!(
                time, self.previous.time,
                "free fall: origin not prepared for {time}"
            );
            self.previous
        };
        FrameState {
            position: s.position,
            velocity: s.velocity,
        }
    }
    pub fn to_inertial(&self, time: f64, local: FrameState) -> FrameState {
        let o = self.origin(time);
        FrameState {
            position: o.position + local.position,
            velocity: o.velocity + local.velocity,
        }
    }
    pub fn from_inertial(&self, time: f64, inertial: FrameState) -> FrameState {
        let o = self.origin(time);
        FrameState {
            position: inertial.position - o.position,
            velocity: inertial.velocity - o.velocity,
        }
    }
}
impl ContactFrame for FreeFallFrame {
    fn acceleration(&self, ephemeris: &Ephemeris, t: f64, r: DVec3, _: DVec3) -> DVec3 {
        let o = self.origin(t).position;
        let mut p = self.propagator.borrow_mut();
        let g0 = p.gravity_at(ephemeris, t, o);
        p.gravity_at(ephemeris, t, o + r) - g0
    }
    fn spin(&self) -> DVec3 {
        DVec3::ZERO
    }
}
