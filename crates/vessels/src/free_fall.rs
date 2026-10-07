use glam::DVec3;
use std::cell::RefCell;
use void_landing::{ContactFrame, FrameState};
use void_orbit::{
    AdvanceOutcome, CelestialBody, EphemerisSource, PropagationRun, Tolerances, VesselPropagator,
    VesselState,
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
        ephemeris: &dyn EphemerisSource,
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
    /// Accepted scene boundary: translate the coordinate anchor by `delta` while preserving
    /// both prepared physical endpoints and velocities. The owner commits the matching split
    /// anchor change; this method alone is not a physical movement request.
    pub fn reanchor_origin(&mut self, delta: DVec3) {
        assert!(delta.is_finite(), "free fall: nonfinite reanchor");
        for (i, value) in delta.to_array().iter().enumerate() {
            self.run.y[i] -= value;
        }
        self.previous.position -= delta;
        self.run = self.run.restarted();
    }
    /// Latest propagated origin time.
    pub fn origin_time(&self) -> f64 {
        self.run.time
    }
    /// Query a current or future origin, propagating it on demand as in the TS lab.
    /// The mutable ephemeris is explicit because propagation extends its history.
    pub fn origin_at(&mut self, ephemeris: &mut dyn EphemerisSource, time: f64) -> FrameState {
        self.advance_origin(ephemeris, time);
        self.origin(time)
    }
    pub fn to_inertial_at(
        &mut self,
        ephemeris: &mut dyn EphemerisSource,
        time: f64,
        local: FrameState,
    ) -> FrameState {
        self.origin_at(ephemeris, time);
        self.to_inertial(time, local)
    }
    pub fn from_inertial_at(
        &mut self,
        ephemeris: &mut dyn EphemerisSource,
        time: f64,
        inertial: FrameState,
    ) -> FrameState {
        self.origin_at(ephemeris, time);
        self.from_inertial(time, inertial)
    }
    pub fn advance_origin(&mut self, ephemeris: &mut dyn EphemerisSource, time: f64) {
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
    fn terrain_body(&self) -> Option<&CelestialBody> {
        None
    }

    fn acceleration(&self, ephemeris: &dyn EphemerisSource, t: f64, r: DVec3, _: DVec3) -> DVec3 {
        let o = self.origin(t).position;
        let mut p = self.propagator.borrow_mut();
        let g0 = p.gravity_at(ephemeris, t, o);
        p.gravity_at(ephemeris, t, o + r) - g0
    }
    fn spin(&self) -> DVec3 {
        DVec3::ZERO
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FreeFallCheckpoint {
    run: PropagationRun,
    previous: VesselState,
}
impl FreeFallFrame {
    pub fn checkpoint(&self) -> FreeFallCheckpoint {
        FreeFallCheckpoint {
            run: self.run.clone(),
            previous: self.previous,
        }
    }
    pub fn from_checkpoint(
        ephemeris: &dyn EphemerisSource,
        tolerances: Tolerances,
        saved: FreeFallCheckpoint,
    ) -> Self {
        assert!(
            saved.run.time.is_finite()
                && saved
                    .run
                    .y
                    .iter()
                    .chain(&saved.run.dy)
                    .all(|v| v.is_finite())
                && saved.previous.time.is_finite()
                && saved.previous.position.is_finite()
                && saved.previous.velocity.is_finite()
                && saved.previous.time <= saved.run.time,
            "free fall checkpoint: invalid origin"
        );
        Self {
            propagator: RefCell::new(VesselPropagator::new(ephemeris, tolerances)),
            run: saved.run,
            previous: saved.previous,
        }
    }
}
