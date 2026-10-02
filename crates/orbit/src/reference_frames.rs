//! The orbit lab's four plotting frames (`ReferenceFrames.ts`). These describe where paths are
//! plotted, not the physical integration frame. Evaluate each path sample at its own time.
use glam::DVec3;
use void_math::hypot;

use crate::{EphemerisSource, body_orientation};

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

pub struct FrameEvaluator {
    pub spec: FrameSpec,
    positions: Vec<DVec3>,
    velocities: Vec<DVec3>,
}

fn unit(v: DVec3) -> DVec3 {
    let length = hypot([v.x, v.y, v.z]);
    assert!(
        length > 0.0 && length.is_finite(),
        "frame has a degenerate axis"
    );
    v / length
}

impl FrameEvaluator {
    pub fn new(ephemeris: &dyn EphemerisSource, spec: FrameSpec) -> Self {
        let n = ephemeris.bodies().len();
        spec.assert_valid(n);
        Self {
            spec,
            positions: vec![DVec3::ZERO; n],
            velocities: vec![DVec3::ZERO; n],
        }
    }

    pub fn rotation_period_seconds(&mut self, ephemeris: &dyn EphemerisSource, t: f64) -> f64 {
        match self.spec {
            FrameSpec::BodySurface { body } => ephemeris.bodies()[body].rotation.period_seconds,
            FrameSpec::TwoBodyRotating { primary, secondary } => {
                ephemeris.states_at(t, &mut self.positions, Some(&mut self.velocities));
                let r = self.positions[secondary] - self.positions[primary];
                let v = self.velocities[secondary] - self.velocities[primary];
                let h = r.cross(v);
                let omega = hypot([h.x, h.y, h.z]) / r.length_squared();
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
        match self.spec {
            FrameSpec::Barycentric => {
                ephemeris.positions_at(t, &mut self.positions);
                PlotFrameState {
                    origin: DVec3::ZERO,
                    axes: [DVec3::X, DVec3::Y, DVec3::Z],
                }
            }
            FrameSpec::BodyInertial { body } => {
                ephemeris.positions_at(t, &mut self.positions);
                PlotFrameState {
                    origin: self.positions[body],
                    axes: ephemeris.bodies()[body].rotation.equatorial_basis(),
                }
            }
            FrameSpec::BodySurface { body } => {
                ephemeris.positions_at(t, &mut self.positions);
                PlotFrameState {
                    origin: self.positions[body],
                    axes: body_orientation(&ephemeris.bodies()[body].rotation, t),
                }
            }
            FrameSpec::TwoBodyRotating { primary, secondary } => {
                ephemeris.states_at(t, &mut self.positions, Some(&mut self.velocities));
                let p = self.positions[primary];
                let s = self.positions[secondary];
                let r = s - p;
                let x = unit(r);
                let z = unit(r.cross(self.velocities[secondary] - self.velocities[primary]));
                let m1 = ephemeris.bodies()[primary].gm;
                let m2 = ephemeris.bodies()[secondary].gm;
                PlotFrameState {
                    origin: (m1 * p + m2 * s) / (m1 + m2),
                    axes: [x, z.cross(x), z],
                }
            }
        }
    }

    /// Position from the most recent evaluate/rotation-period query, as in the TS scratch API.
    pub fn position(&self, index: usize) -> DVec3 {
        self.positions[index]
    }
}
