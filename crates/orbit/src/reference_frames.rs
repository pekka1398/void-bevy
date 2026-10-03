//! The orbit lab's four plotting frames (`ReferenceFrames.ts`). These describe where paths are
//! plotted, not the physical integration frame. Evaluate each path sample at its own time.
use glam::DVec3;
use void_frames::{BodyId, FrameId};

use crate::{EphemerisSource, SystemFrames};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
pub struct FrameEvaluator {
    pub spec: FrameSpec,
    frames: SystemFrames,
    frame: FrameId,
    positions: Vec<DVec3>,
}

impl FrameEvaluator {
    pub fn new(ephemeris: &dyn EphemerisSource, spec: FrameSpec) -> Self {
        let n = ephemeris.bodies().len();
        spec.assert_valid(n);
        let mut frames = SystemFrames::new(ephemeris);
        let frame = match spec {
            FrameSpec::Barycentric => frames.origin,
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

    /// Position from the most recent evaluate/rotation-period query, as in the TS scratch API.
    pub fn position(&self, index: usize) -> DVec3 {
        self.positions[index]
    }
}
