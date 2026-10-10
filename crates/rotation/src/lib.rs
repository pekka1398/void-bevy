//! Rigid-body rotation seen from a frame that itself turns at a constant angular velocity Ω (a
//! planet's body-fixed frame).
//!
//! All vectors are in the frame's axes. ω is the body's angular velocity relative to the frame
//! (what a physics engine working in the frame stores), I the body's inertia about its mass centre
//! in the frame's axes at this instant. The body's inertial angular velocity is ω + Ω.
//!
//! Inertially, d/dt [I (ω + Ω)] = τ. Written in the turning frame, where I turns with the body at
//! ω and a vector's rate gains Ω × (itself):
//!
//!   I ω̇ = τ − ω × Iω − [ ω × IΩ + Ω × Iω + Ω × IΩ − I (ω × Ω) ]
//!
//! An engine that treats the frame as inertial (Rapier) solves the first two terms. The bracket is
//! what it leaves out: the frame's torque on the body, returned by `fictitious_torque`.
//! - A body not turning in space, ω = −Ω: the frame adds Ω × IΩ, which cancels the engine's
//!   gyroscopic −ω × Iω, so it keeps its inertial attitude while the frame turns under it.
//! - A body at rest in the frame, ω = 0: it needs −Ω × IΩ to keep turning with the frame. On the
//!   ground, contacts supply it.

use glam::{DMat3, DQuat, DVec3};

fn finite(v: DVec3, what: &str) {
    assert!(v.is_finite(), "rotating frame: {what} {v}");
}

/// The torque a frame turning at `frame_spin` adds to a rigid body with world inertia `inertia`
/// turning at `angular_velocity` relative to it, all in the frame's axes:
/// −[ ω × IΩ + Ω × Iω + Ω × IΩ − I (ω × Ω) ].
pub fn fictitious_torque(inertia: &DMat3, angular_velocity: DVec3, frame_spin: DVec3) -> DVec3 {
    finite(angular_velocity, "angular velocity");
    finite(frame_spin, "frame spin");
    for (i, v) in inertia.to_cols_array().iter().enumerate() {
        assert!(
            v.is_finite() && (i % 4 != 0 || *v > 0.0),
            "rotating frame: inertia {inertia:?}"
        );
    }
    let (w, o) = (angular_velocity, frame_spin);
    let (io, iw) = (*inertia * o, *inertia * w);
    let (a, b, c, d) = (w.cross(io), o.cross(iw), o.cross(io), *inertia * w.cross(o));
    DVec3::new(
        -(a.x + b.x + c.x - d.x),
        -(a.y + b.y + c.y - d.y),
        -(a.z + b.z + c.z - d.z),
    )
}

fn solve(m: &DMat3, v: DVec3) -> DVec3 {
    let det = m.determinant();
    assert!(
        det.abs() > 0.0 && det.is_finite(),
        "rotating frame: singular inertia {m:?}"
    );
    m.inverse() * v
}

/// R I Rᵀ: a body-axes inertia turned into the frame by q.
pub fn inertia_in(q: DQuat, inertia_local: &DMat3) -> DMat3 {
    let r = DMat3::from_quat(q);
    r * *inertia_local * r.transpose()
}

/// q turned by the rotation vector w dt (exact for a constant w).
fn advance(q: DQuat, w: DVec3, dt: f64) -> DQuat {
    let rate = w.length();
    if rate == 0.0 {
        return q;
    }
    let half = rate * dt / 2.0;
    let s = half.sin() / rate;
    (DQuat::from_xyzw(w.x * s, w.y * s, w.z * s, half.cos()) * q).normalize()
}

/// One torque-free step of a rigid body in a frame turning at `frame_spin`, in f64, all in frame
/// axes (`rotation`: body axes to frame). Built on the inertial angular momentum L = I (ω + Ω),
/// which nothing changes, so seen from the frame it only turns by −Ω dt. Midpoint in the attitude,
/// so second order for a tumble:
///   L(t) = rot(−Ω t) L₀;  q½ = exp(ω₀ dt/2) q₀;  ω½ = I(q½)⁻¹ L(dt/2) − Ω
///   q₁ = exp(ω½ dt) q₀;  ω₁ = I(q₁)⁻¹ L(dt) − Ω
/// A body still in space (ω = −Ω) and a spin about a principal axis come out exact; |L| is kept
/// exactly always.
pub fn free_rotation_step(
    rotation: DQuat,
    angular_velocity: DVec3,
    inertia_local: &DMat3,
    frame_spin: DVec3,
    dt: f64,
) -> (DQuat, DVec3) {
    finite(angular_velocity, "angular velocity");
    finite(frame_spin, "frame spin");
    assert!(dt > 0.0 && dt.is_finite(), "rotating frame: dt {dt}");
    let l0 = inertia_in(rotation, inertia_local) * (angular_velocity + frame_spin);
    let rate = frame_spin.length();
    let at = |t: f64| {
        if rate == 0.0 {
            l0
        } else {
            DQuat::from_axis_angle(frame_spin / rate, -rate * t) * l0
        }
    };
    let half = advance(rotation, angular_velocity, dt / 2.0);
    let mid = solve(&inertia_in(half, inertia_local), at(dt / 2.0)) - frame_spin;
    let next = advance(rotation, mid, dt);
    (
        next,
        solve(&inertia_in(next, inertia_local), at(dt)) - frame_spin,
    )
}

/// Local torque half-impulses around a free angular-momentum step (second-order splitting).
pub fn rotation_step(
    rotation: DQuat,
    angular_velocity: DVec3,
    inertia_local: &DMat3,
    torque_local: DVec3,
    frame_spin: DVec3,
    dt: f64,
) -> (DQuat, DVec3) {
    finite(torque_local, "local torque");
    let kick = |q: DQuat| q * solve(inertia_local, torque_local) * (dt / 2.0);
    let (q, w) = free_rotation_step(
        rotation,
        angular_velocity + kick(rotation),
        inertia_local,
        frame_spin,
        dt,
    );
    (q, w + kick(q))
}

/// Body rotation with internal rotor angular momentum, expressed in body axes.
/// The carrier inertia already includes locked rotor mass/inertia. Internal motor, brake and
/// steering changes exchange momentum with the carrier; only `torque_local` is external.
/// Pure trial evaluation: callers commit rotor state only after accepting the whole step.
pub fn rotation_step_with_rotor(
    rotation: DQuat,
    angular_velocity: DVec3,
    inertia_local: &DMat3,
    torque_local: DVec3,
    rotor_initial_local: DVec3,
    rotor_final_local: DVec3,
    dt: f64,
) -> (DQuat, DVec3) {
    assert!(dt.is_finite() && dt > 0.0);
    finite(angular_velocity, "rotor carrier angular velocity");
    finite(torque_local, "rotor external torque");
    finite(rotor_initial_local, "initial rotor momentum");
    finite(rotor_final_local, "final rotor momentum");
    if rotor_initial_local == DVec3::ZERO && rotor_final_local == DVec3::ZERO {
        return rotation_step(
            rotation,
            angular_velocity,
            inertia_local,
            torque_local,
            DVec3::ZERO,
            dt,
        );
    }
    let inertia = *inertia_local;
    let inverse = inertia.inverse();
    assert!(inverse.is_finite(), "singular rotor carrier inertia");
    let half_momentum = rotation
        * (inertia * (rotation.conjugate() * angular_velocity)
            + rotor_initial_local
            + torque_local * (dt / 2.0));
    let midpoint_rotor = (rotor_initial_local + rotor_final_local) / 2.0;
    let mut midpoint_rotation =
        (DQuat::from_scaled_axis(angular_velocity * (dt / 2.0)) * rotation).normalize();
    let mut midpoint_velocity = angular_velocity;
    for _ in 0..16 {
        midpoint_velocity = midpoint_rotation
            * (inverse * (midpoint_rotation.conjugate() * half_momentum - midpoint_rotor));
        midpoint_rotation =
            (DQuat::from_scaled_axis(midpoint_velocity * (dt / 2.0)) * rotation).normalize();
    }
    let next_rotation = (DQuat::from_scaled_axis(midpoint_velocity * dt) * rotation).normalize();
    let final_momentum = half_momentum + next_rotation * torque_local * (dt / 2.0);
    let next_velocity = next_rotation
        * (inverse * (next_rotation.conjugate() * final_momentum - rotor_final_local));
    finite(next_velocity, "rotor carrier result");
    assert!(next_rotation.is_finite());
    (next_rotation, next_velocity)
}
