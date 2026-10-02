//! KSP's stability assist on a craft's steering torque, as `lab/sas/src/StabilityAssist.ts`.
//!
//! It returns the same `turn` command a pilot gives (local axes, each in [−1, 1], times the unit's
//! maximum torque), so SAS never has more authority than the keys.
//! - With no keys held, it first stops the spin, then locks the attitude it came to rest at and
//!   holds it.
//! - While a key is held, that axis is the pilot's; the other axes are only kept from spinning.
//!   Releasing every key damps, then locks the new attitude, so the craft is not pulled back to
//!   where it was.
//!
//! Control law, per local axis: a desired spin toward the target that allows braking to rest
//! within `brake_fraction` of the axis's angular acceleration (√(2 b a |e|)), and linear
//! (|e| / `attitude_seconds`) close in; the rate loop then asks for the angular acceleration
//! (desired − spin) / `rate_seconds`. Torque is Iα plus the gyroscopic ω × Iω; when it exceeds the
//! unit's torque on some axis the whole vector is scaled down, keeping its direction.
//!
//! Attitudes and spin are in the frame the physics integrates in (the planet's body-fixed frame),
//! so "holding" means holding still in that frame.

use glam::{DQuat, DVec3};
use void_math::{atan2, hypot};
use void_rotation::{Mat3, matrix};

/// Off: the pilot's command passes through. Pilot: a key is held; SAS stops spin on the other
/// axes. Damping: keys released, SAS brings the spin down before it locks. Holding: SAS holds the
/// locked attitude.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SasPhase {
    Off,
    Pilot,
    Damping,
    Holding,
}

impl SasPhase {
    /// The lab's phase text.
    pub fn label(self) -> &'static str {
        match self {
            SasPhase::Off => "off",
            SasPhase::Pilot => "pilot input: other axes damped",
            SasPhase::Damping => "stopping the spin",
            SasPhase::Holding => "holding attitude",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SasTuning {
    /// Rate loop: the spin error is closed with this time constant, s.
    pub rate_seconds: f64,
    /// Attitude loop, s. At least 4 × `rate_seconds`, so the linear loop is critically damped or
    /// slower.
    pub attitude_seconds: f64,
    /// Fraction of the unit's angular acceleration the approach plans to brake with; the rest is
    /// margin.
    pub brake_fraction: f64,
    /// Spin below which damping locks the current attitude, rad/s.
    pub lock_rate: f64,
}

pub const SAS_TUNING: SasTuning = SasTuning {
    rate_seconds: 0.15,
    attitude_seconds: 0.6,
    brake_fraction: 0.5,
    lock_rate: 0.002,
};

fn multiply(a: DQuat, b: DQuat) -> DQuat {
    DQuat::from_xyzw(
        a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
        a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
        a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
        a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
    )
}

fn mat_vec(m: &Mat3, v: DVec3) -> DVec3 {
    DVec3::new(
        m[0] * v.x + m[1] * v.y + m[2] * v.z,
        m[3] * v.x + m[4] * v.y + m[5] * v.z,
        m[6] * v.x + m[7] * v.y + m[8] * v.z,
    )
}

/// JavaScript's `Math.sign`: zero (of either sign) and NaN come back unchanged.
fn sign(v: f64) -> f64 {
    if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        v
    }
}

/// Rotation vector (axis times angle, rad) that turns the target attitude into the current one,
/// in the craft's local axes. conj(target) · current maps current local coordinates to target
/// local ones; its axis is the same vector in both. The shorter way round is taken.
pub fn attitude_error(target: DQuat, current: DQuat) -> DVec3 {
    let mut r = multiply(
        DQuat::from_xyzw(-target.x, -target.y, -target.z, target.w),
        current,
    );
    if r.w < 0.0 {
        r = DQuat::from_xyzw(-r.x, -r.y, -r.z, -r.w);
    }
    let s = hypot([r.x, r.y, r.z]);
    if s == 0.0 {
        return DVec3::ZERO;
    }
    let angle = 2.0 * atan2(s, r.w);
    DVec3::new(r.x / s * angle, r.y / s * angle, r.z / s * angle)
}

#[derive(Clone, Debug)]
pub struct StabilityAssist {
    max_torque: f64,
    tuning: SasTuning,
    phase: SasPhase,
    locked: Option<DQuat>,
}

impl StabilityAssist {
    /// `max_torque`: N m per unit of turn command on each axis (lab/landing's `STEERING_TORQUE`).
    pub fn new(max_torque: f64, tuning: SasTuning) -> Self {
        assert!(
            max_torque > 0.0 && max_torque.is_finite(),
            "stability assist: max torque {max_torque}"
        );
        let t = tuning;
        assert!(
            t.rate_seconds > 0.0
                && t.attitude_seconds >= 4.0 * t.rate_seconds
                && t.brake_fraction > 0.0
                && t.brake_fraction <= 1.0
                && t.lock_rate > 0.0,
            "stability assist: bad tuning {t:?}"
        );
        Self {
            max_torque,
            tuning,
            phase: SasPhase::Off,
            locked: None,
        }
    }

    pub fn max_torque(&self) -> f64 {
        self.max_torque
    }

    pub fn tuning(&self) -> SasTuning {
        self.tuning
    }

    pub fn enabled(&self) -> bool {
        self.phase != SasPhase::Off
    }

    pub fn phase(&self) -> SasPhase {
        self.phase
    }

    /// The held attitude while holding, else None.
    pub fn target(&self) -> Option<DQuat> {
        self.locked
    }

    pub fn set_enabled(&mut self, on: bool) {
        self.locked = None;
        self.phase = if on { SasPhase::Damping } else { SasPhase::Off };
    }

    pub fn toggle(&mut self) {
        self.set_enabled(!self.enabled());
    }

    /// The turn command for the coming step of `dt` seconds. `rotation` and `angular_velocity`
    /// are the unit's at the step's start in the integrating frame, `inertia` its inertia in its
    /// local axes, `pilot` the pilot's turn command (each axis in [−1, 1]).
    pub fn command(
        &mut self,
        rotation: DQuat,
        angular_velocity: DVec3,
        inertia: &Mat3,
        pilot: DVec3,
        dt: f64,
    ) -> DVec3 {
        assert!(dt > 0.0 && dt.is_finite(), "stability assist: dt {dt}");
        assert!(
            pilot.to_array().iter().all(|v| (-1.0..=1.0).contains(v)),
            "stability assist: pilot command {pilot}"
        );
        if self.phase == SasPhase::Off {
            return pilot;
        }
        for (i, v) in inertia.iter().enumerate() {
            assert!(
                v.is_finite() && (i % 4 != 0 || *v > 0.0),
                "stability assist: inertia {inertia:?}"
            );
        }
        let r = matrix(rotation);
        // The spin in local axes: Rᵀ ω.
        let w = mat_vec(
            &[r[0], r[3], r[6], r[1], r[4], r[7], r[2], r[5], r[8]],
            angular_velocity,
        );
        let held = pilot.to_array().map(|v| v != 0.0);
        let SasTuning {
            rate_seconds,
            attitude_seconds,
            brake_fraction,
            lock_rate,
        } = self.tuning;

        let mut desired = DVec3::ZERO;
        if held.contains(&true) {
            self.phase = SasPhase::Pilot;
            self.locked = None;
        } else {
            if self.phase != SasPhase::Holding {
                self.phase = SasPhase::Damping;
                if hypot([w.x, w.y, w.z]) < lock_rate {
                    self.phase = SasPhase::Holding;
                    self.locked = Some(rotation);
                }
            }
            if let Some(locked) = self.locked {
                let e = attitude_error(locked, rotation);
                let spin = |e: f64, diagonal: f64| {
                    let size = e.abs();
                    let reach = (2.0 * brake_fraction * (self.max_torque / diagonal) * size)
                        .sqrt()
                        .min(size / attitude_seconds);
                    -sign(e) * reach
                };
                desired = DVec3::new(
                    spin(e.x, inertia[0]),
                    spin(e.y, inertia[4]),
                    spin(e.z, inertia[8]),
                );
            }
        }

        // Pilot axes take no acceleration from SAS; theirs is the pilot's command, set below.
        let (desired, spin) = (desired.to_array(), w.to_array());
        let alpha: [f64; 3] = std::array::from_fn(|i| {
            if held[i] {
                0.0
            } else {
                (desired[i] - spin[i]) / rate_seconds
            }
        });
        let alpha = DVec3::from_array(alpha);
        let iw = mat_vec(inertia, w);
        let ia = mat_vec(inertia, alpha);
        let torque = DVec3::new(
            ia.x + w.y * iw.z - w.z * iw.y,
            ia.y + w.z * iw.x - w.x * iw.z,
            ia.z + w.x * iw.y - w.y * iw.x,
        );
        let out = [
            torque.x / self.max_torque,
            torque.y / self.max_torque,
            torque.z / self.max_torque,
        ];
        let largest = (0..3)
            .filter(|&i| !held[i])
            .fold(0.0_f64, |m, i| m.max(out[i].abs()));
        let scale = if largest > 1.0 { 1.0 / largest } else { 1.0 };
        let pilot = pilot.to_array();
        DVec3::from_array(std::array::from_fn(|i| {
            if held[i] { pilot[i] } else { out[i] * scale }
        }))
    }
}
