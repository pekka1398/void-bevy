use glam::{DQuat, DVec3};

/// Rigid motion of a child frame relative to its parent, as Principia's `RigidMotion`.
///
/// A point `p` and velocity `v` in the child frame are, in the parent frame,
/// `R p + T` and `R v + ω × (R p) + V`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    /// The child's origin, in parent coordinates.
    pub translation: DVec3,
    /// Velocity of the child's origin, in parent coordinates.
    pub velocity: DVec3,
    /// Child axes to parent axes.
    pub rotation: DQuat,
    /// Angular velocity of the child relative to the parent, in parent coordinates.
    pub angular_velocity: DVec3,
}

/// A position and velocity in one frame's coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub position: DVec3,
    pub velocity: DVec3,
}

/// Quaternions further than this from unit length are a bug upstream, not rounding.
const UNIT_TOLERANCE: f64 = 1e-12;

impl Motion {
    pub const IDENTITY: Self = Self {
        translation: DVec3::ZERO,
        velocity: DVec3::ZERO,
        rotation: DQuat::IDENTITY,
        angular_velocity: DVec3::ZERO,
    };

    /// A motion with every field finite and a unit rotation; anything else panics.
    pub fn new(translation: DVec3, velocity: DVec3, rotation: DQuat, angular_velocity: DVec3) -> Self {
        let motion = Self { translation, velocity, rotation, angular_velocity };
        motion.assert_valid();
        motion
    }

    /// A motion that does not change with time.
    pub fn fixed(translation: DVec3, rotation: DQuat) -> Self {
        Self::new(translation, DVec3::ZERO, rotation, DVec3::ZERO)
    }

    pub fn assert_valid(&self) {
        assert!(
            self.translation.is_finite() && self.velocity.is_finite() && self.angular_velocity.is_finite(),
            "motion not finite: {self:?}"
        );
        assert!(self.rotation.is_finite(), "rotation not finite: {:?}", self.rotation);
        assert!(
            (self.rotation.length() - 1.0).abs() < UNIT_TOLERANCE,
            "rotation not unit: |q| = {}",
            self.rotation.length()
        );
    }

    pub fn apply_point(&self, p: DVec3) -> DVec3 {
        self.rotation * p + self.translation
    }

    /// Turns a direction; translation and motion do not apply.
    pub fn apply_direction(&self, d: DVec3) -> DVec3 {
        self.rotation * d
    }

    pub fn apply_state(&self, s: State) -> State {
        let turned = self.rotation * s.position;
        State {
            position: turned + self.translation,
            velocity: self.rotation * s.velocity + self.angular_velocity.cross(turned) + self.velocity,
        }
    }

    /// Parent coordinates to child coordinates. Subtracting before turning keeps the digits that
    /// `inverse().apply_point` loses when the translation is large.
    pub fn unapply_point(&self, p: DVec3) -> DVec3 {
        self.rotation.inverse() * (p - self.translation)
    }

    pub fn unapply_state(&self, s: State) -> State {
        let back = self.rotation.inverse();
        let relative = s.position - self.translation;
        State {
            position: back * relative,
            velocity: back * (s.velocity - self.velocity - self.angular_velocity.cross(relative)),
        }
    }

    /// `self` maps A to B and `outer` maps B to C; the result maps A to C.
    pub fn then(&self, outer: &Motion) -> Motion {
        let turned_translation = outer.rotation * self.translation;
        Motion {
            translation: turned_translation + outer.translation,
            velocity: outer.rotation * self.velocity
                + outer.angular_velocity.cross(turned_translation)
                + outer.velocity,
            rotation: (outer.rotation * self.rotation).normalize(),
            angular_velocity: outer.angular_velocity + outer.rotation * self.angular_velocity,
        }
    }

    /// Maps parent to child.
    pub fn inverse(&self) -> Motion {
        let back = self.rotation.inverse();
        Motion {
            translation: -(back * self.translation),
            velocity: back * (self.angular_velocity.cross(self.translation) - self.velocity),
            rotation: back,
            angular_velocity: -(back * self.angular_velocity),
        }
    }
}
