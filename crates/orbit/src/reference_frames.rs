//! The orbit lab's four plotting frames (`ReferenceFrames.ts`). These describe where paths are
//! plotted, not the physical integration frame. Evaluate each path sample at its own time.
use glam::DVec3;
use serde::{Deserialize, Serialize};
use void_frames::{BodyId, FrameId, State, SystemId};

use crate::{EphemerisSource, SystemFrames};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameSpec {
    Barycentric,
    BodyInertial { body: usize },
    BodySurface { body: usize },
    TwoBodyRotating { primary: usize, secondary: usize },
}

impl FrameSpec {
    pub fn assert_valid(self, body_count: usize) {
        match self {
            Self::Barycentric => {}
            Self::BodyInertial { body } | Self::BodySurface { body } => {
                assert!(body < body_count, "frame body {body}");
            }
            Self::TwoBodyRotating { primary, secondary } => {
                assert!(
                    primary < body_count && secondary < body_count && primary != secondary,
                    "two-body frame {primary}/{secondary}"
                );
            }
        }
    }

    pub fn centred_on(self, index: usize) -> bool {
        matches!(self, Self::BodyInertial { body } | Self::BodySurface { body } if body == index)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PlotFrameState {
    pub origin: DVec3,
    /// Unit x, y, z axes in barycentric ecliptic coordinates.
    pub axes: [DVec3; 3],
}

pub fn direction_to_frame(frame: &PlotFrameState, direction: DVec3) -> DVec3 {
    DVec3::new(
        direction.dot(frame.axes[0]),
        direction.dot(frame.axes[1]),
        direction.dot(frame.axes[2]),
    )
}

pub fn to_frame(frame: &PlotFrameState, barycentric: DVec3) -> DVec3 {
    direction_to_frame(frame, barycentric - frame.origin)
}

/// A plotting frame as a frame of the ephemeris' tree: barycentric is the origin system,
/// body-inertial and body-surface the body's frames, two-body rotating a node of its own.
#[derive(Clone, Debug)]
pub struct FrameEvaluator {
    pub spec: FrameSpec,
    frames: SystemFrames,
    frame: FrameId,
    positions: Vec<DVec3>,
}

impl FrameEvaluator {
    pub fn new(ephemeris: &dyn EphemerisSource, spec: FrameSpec) -> Self {
        Self::new_in_system(ephemeris, spec, ephemeris.origin_system())
    }

    pub fn new_in_system(
        ephemeris: &dyn EphemerisSource,
        spec: FrameSpec,
        system: SystemId,
    ) -> Self {
        assert!(
            system.0 < ephemeris.system_count(),
            "plot system out of range"
        );
        let n = ephemeris.bodies().len();
        spec.assert_valid(n);
        let mut frames = SystemFrames::new(ephemeris);
        let frame = match spec {
            FrameSpec::Barycentric => frames.systems[system.0],
            FrameSpec::BodyInertial { body } => frames.inertial[body],
            FrameSpec::BodySurface { body } => frames.surface[body],
            FrameSpec::TwoBodyRotating { primary, secondary } => {
                let system = ephemeris.system_of(primary);
                assert_eq!(
                    system,
                    ephemeris.system_of(secondary),
                    "two-body frame across systems"
                );
                let bodies = ephemeris.bodies();
                frames.tree.add_two_body(
                    frames.systems[system.0],
                    (BodyId(primary), bodies[primary].gm),
                    (BodyId(secondary), bodies[secondary].gm),
                )
            }
        };
        Self {
            spec,
            frames,
            frame,
            positions: vec![DVec3::ZERO; n],
        }
    }

    pub fn rotation_period_seconds(&mut self, ephemeris: &dyn EphemerisSource, t: f64) -> f64 {
        match self.spec {
            FrameSpec::BodySurface { body } => ephemeris.bodies()[body].rotation.period_seconds,
            FrameSpec::TwoBodyRotating { .. } => {
                ephemeris.positions_at(t, &mut self.positions);
                let omega = self
                    .frames
                    .tree
                    .at(t, ephemeris)
                    .motion_to_parent(self.frame)
                    .angular_velocity
                    .length();
                assert!(
                    omega > 0.0 && omega.is_finite(),
                    "two-body frame has no rotation"
                );
                std::f64::consts::TAU / omega
            }
            _ => f64::INFINITY,
        }
    }

    pub fn evaluate(&mut self, ephemeris: &dyn EphemerisSource, t: f64) -> PlotFrameState {
        ephemeris.positions_at(t, &mut self.positions);
        let to = self
            .frames
            .tree
            .at(t, ephemeris)
            .transform(self.frame, self.frames.origin);
        PlotFrameState {
            origin: to.apply_point(DVec3::ZERO),
            axes: [DVec3::X, DVec3::Y, DVec3::Z].map(|axis| to.apply_direction(axis)),
        }
    }

    /// Transform both position and its time derivative, including origin motion and rotation.
    pub fn state_at(&self, ephemeris: &dyn EphemerisSource, t: f64, state: State) -> State {
        assert!(
            t.is_finite() && state.position.is_finite() && state.velocity.is_finite(),
            "nonfinite plotting state"
        );
        let at = self.frames.tree.at(t, ephemeris);
        let mut transformed = at
            .transform(self.frames.origin, self.frame)
            .apply_state(state);
        if matches!(self.spec, FrameSpec::TwoBodyRotating { .. }) {
            // The physical tree's two-body angular velocity includes in-plane motion only.
            // For plotting, include changing orbital-normal orientation under N-body forces.
            // Differencing directions avoids differencing large absolute positions.
            let dt = 0.01_f64.min((ephemeris.end_time() - ephemeris.start_time()) / 4.0);
            assert!(
                dt > 0.0,
                "pair plotting velocity needs ephemeris time coverage"
            );
            let lo = (t - dt).max(ephemeris.start_time());
            let hi = (t + dt).min(ephemeris.end_time());
            assert!(hi > lo, "pair plotting velocity outside coverage");
            let current = at.transform(self.frame, self.frames.origin);
            let relative = state.position - current.apply_point(DVec3::ZERO);
            let old_omega = current.to_motion().angular_velocity;
            let before = self
                .frames
                .tree
                .at(lo, ephemeris)
                .transform(self.frame, self.frames.origin);
            let after = self
                .frames
                .tree
                .at(hi, ephemeris)
                .transform(self.frame, self.frames.origin);
            let correction = [DVec3::X, DVec3::Y, DVec3::Z].map(|axis| {
                let direction = current.apply_direction(axis);
                let derivative =
                    (after.apply_direction(axis) - before.apply_direction(axis)) / (hi - lo);
                derivative.dot(relative) + direction.dot(old_omega.cross(relative))
            });
            transformed.velocity += DVec3::from_array(correction);
        }
        assert!(
            transformed.position.is_finite() && transformed.velocity.is_finite(),
            "nonfinite transformed plotting state"
        );
        transformed
    }

    /// Position from the most recent evaluate/rotation-period query, as in the TS scratch API.
    pub fn position(&self, index: usize) -> DVec3 {
        self.positions[index]
    }
}
