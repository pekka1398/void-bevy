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

use glam::{DQuat, DVec3};

/// Row-major 3 × 3.
pub type Mat3 = [f64; 9];

fn cross(a: DVec3, b: DVec3) -> DVec3 {
    DVec3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

fn mul(m: &Mat3, v: DVec3) -> DVec3 {
    DVec3::new(
        m[0] * v.x + m[1] * v.y + m[2] * v.z,
        m[3] * v.x + m[4] * v.y + m[5] * v.z,
        m[6] * v.x + m[7] * v.y + m[8] * v.z,
    )
}

fn finite(v: DVec3, what: &str) {
    assert!(v.is_finite(), "rotating frame: {what} {v}");
}

/// The torque a frame turning at `frame_spin` adds to a rigid body with world inertia `inertia`
/// turning at `angular_velocity` relative to it, all in the frame's axes:
/// −[ ω × IΩ + Ω × Iω + Ω × IΩ − I (ω × Ω) ].
pub fn fictitious_torque(inertia: &Mat3, angular_velocity: DVec3, frame_spin: DVec3) -> DVec3 {
    finite(angular_velocity, "angular velocity");
    finite(frame_spin, "frame spin");
    for (i, v) in inertia.iter().enumerate() {
        assert!(
            v.is_finite() && (i % 4 != 0 || *v > 0.0),
            "rotating frame: inertia {inertia:?}"
        );
    }
    let (w, o) = (angular_velocity, frame_spin);
    let (io, iw) = (mul(inertia, o), mul(inertia, w));
    let (a, b, c, d) = (
        cross(w, io),
        cross(o, iw),
        cross(o, io),
        mul(inertia, cross(w, o)),
    );
    DVec3::new(
        -(a.x + b.x + c.x - d.x),
        -(a.y + b.y + c.y - d.y),
        -(a.z + b.z + c.z - d.z),
    )
}

fn solve(m: &Mat3, v: DVec3) -> DVec3 {
    let [a, b, c, d, e, f, g, h, i] = *m;
    let (big_a, big_b, big_c) = (e * i - f * h, -(d * i - f * g), d * h - e * g);
    let det = a * big_a + b * big_b + c * big_c;
    assert!(
        det.abs() > 0.0 && det.is_finite(),
        "rotating frame: singular inertia {m:?}"
    );
    mul(
        &[
            big_a / det,
            -(b * i - c * h) / det,
            (b * f - c * e) / det,
            big_b / det,
            (a * i - c * g) / det,
            -(a * f - c * d) / det,
            big_c / det,
            -(a * h - b * g) / det,
            (a * e - b * d) / det,
        ],
        v,
    )
}

/// v turned by `angle` about the unit `axis` (Rodrigues).
fn turn(v: DVec3, k: DVec3, angle: f64) -> DVec3 {
    let (c, s) = (angle.cos(), angle.sin());
    let kv = cross(k, v);
    let kd = k.x * v.x + k.y * v.y + k.z * v.z;
    DVec3::new(
        v.x * c + kv.x * s + k.x * kd * (1.0 - c),
        v.y * c + kv.y * s + k.y * kd * (1.0 - c),
        v.z * c + kv.z * s + k.z * kd * (1.0 - c),
    )
}

/// Rotation matrix of a unit quaternion, row-major.
pub fn matrix(q: DQuat) -> Mat3 {
    let (x, y, z, w) = (q.x, q.y, q.z, q.w);
    [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y - z * w),
        2.0 * (x * z + y * w),
        2.0 * (x * y + z * w),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z - x * w),
        2.0 * (x * z - y * w),
        2.0 * (y * z + x * w),
        1.0 - 2.0 * (x * x + y * y),
    ]
}

/// R I Rᵀ: a body-axes inertia turned into the frame by q.
pub fn inertia_in(q: DQuat, inertia_local: &Mat3) -> Mat3 {
    let r = matrix(q);
    let mut out = [0.0; 9];
    for i in 0..3 {
        for j in 0..3 {
            let mut s = 0.0;
            for a in 0..3 {
                for b in 0..3 {
                    s += r[i * 3 + a] * inertia_local[a * 3 + b] * r[j * 3 + b];
                }
            }
            out[i * 3 + j] = s;
        }
    }
    out
}

/// q turned by the rotation vector w dt (exact for a constant w).
fn advance(q: DQuat, w: DVec3, dt: f64) -> DQuat {
    let rate = w.length();
    if rate == 0.0 {
        return q;
    }
    let half = rate * dt / 2.0;
    let s = half.sin() / rate;
    let (dx, dy, dz, dw) = (w.x * s, w.y * s, w.z * s, half.cos());
    let r = DQuat::from_xyzw(
        dw * q.x + dx * q.w + dy * q.z - dz * q.y,
        dw * q.y - dx * q.z + dy * q.w + dz * q.x,
        dw * q.z + dx * q.y - dy * q.x + dz * q.w,
        dw * q.w - dx * q.x - dy * q.y - dz * q.z,
    );
    let l = r.length();
    DQuat::from_xyzw(r.x / l, r.y / l, r.z / l, r.w / l)
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
    inertia_local: &Mat3,
    frame_spin: DVec3,
    dt: f64,
) -> (DQuat, DVec3) {
    finite(angular_velocity, "angular velocity");
    finite(frame_spin, "frame spin");
    assert!(dt > 0.0 && dt.is_finite(), "rotating frame: dt {dt}");
    let l0 = mul(
        &inertia_in(rotation, inertia_local),
        angular_velocity + frame_spin,
    );
    let rate = frame_spin.length();
    let at = |t: f64| {
        if rate == 0.0 {
            l0
        } else {
            turn(
                l0,
                DVec3::new(
                    frame_spin.x / rate,
                    frame_spin.y / rate,
                    frame_spin.z / rate,
                ),
                -rate * t,
            )
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
    inertia_local: &Mat3,
    torque_local: DVec3,
    frame_spin: DVec3,
    dt: f64,
) -> (DQuat, DVec3) {
    finite(torque_local, "local torque");
    let kick = |q: DQuat| {
        let a = mul(&matrix(q), solve(inertia_local, torque_local));
        DVec3::new(a.x * dt / 2.0, a.y * dt / 2.0, a.z * dt / 2.0)
    };
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
    inertia_local: &Mat3,
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
    let inertia = glam::DMat3::from_cols_array(inertia_local).transpose();
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
