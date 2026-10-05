//! A force and its moment about one explicit point, all in one frame, in SI/f64.
use glam::DVec3;
use void_frames::{FrameId, FrameSource, Snapshot};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wrench {
    pub frame: FrameId,
    pub reference_point: DVec3,
    pub force: DVec3,
    pub torque: DVec3,
}
impl Wrench {
    pub fn zero(frame: FrameId, reference_point: DVec3) -> Self {
        assert!(reference_point.is_finite(), "invalid wrench reference");
        Self {
            frame,
            reference_point,
            force: DVec3::ZERO,
            torque: DVec3::ZERO,
        }
    }
    /// Intrinsic moment is about the application point. This is the sole r × F conversion.
    pub fn at_point(
        frame: FrameId,
        reference_point: DVec3,
        point: DVec3,
        force: DVec3,
        moment: DVec3,
    ) -> Self {
        assert!(point.is_finite(), "invalid wrench application point");
        Self::at_offset(
            frame,
            reference_point,
            point - reference_point,
            force,
            moment,
        )
    }
    /// As at_point, but with an already known displacement from the reference. Keeps a small
    /// rigid-body moment arm exact when its absolute COM is millions of kilometres away.
    pub fn at_offset(
        frame: FrameId,
        reference_point: DVec3,
        offset: DVec3,
        force: DVec3,
        moment: DVec3,
    ) -> Self {
        assert!(
            offset.is_finite() && force.is_finite() && moment.is_finite(),
            "invalid wrench"
        );
        let torque = offset.cross(force) + moment;
        assert!(torque.is_finite(), "non-finite wrench moment");
        Self {
            force,
            torque,
            ..Self::zero(frame, reference_point)
        }
    }
    pub fn about(self, reference_point: DVec3) -> Self {
        assert!(reference_point.is_finite(), "invalid wrench reference");
        Self {
            reference_point,
            torque: self.torque + (self.reference_point - reference_point).cross(self.force),
            ..self
        }
    }
    pub fn in_frame<S: FrameSource + ?Sized>(self, at: &Snapshot<'_, S>, frame: FrameId) -> Self {
        let transform = at.transform(self.frame, frame);
        Self {
            frame,
            reference_point: transform.apply_point(self.reference_point),
            force: transform.rotation() * self.force,
            torque: transform.rotation() * self.torque,
        }
    }
    pub fn add(&mut self, other: Self) {
        assert_eq!(self.frame, other.frame, "wrench frame mismatch");
        assert_eq!(
            self.reference_point, other.reference_point,
            "wrench reference mismatch"
        );
        self.force += other.force;
        self.torque += other.torque;
        assert!(
            self.force.is_finite() && self.torque.is_finite(),
            "non-finite wrench sum"
        );
    }
}
