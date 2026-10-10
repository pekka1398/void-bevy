//! KSP's stability assist on a craft's steering torque.
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

use glam::{DMat3, DQuat, DVec3};
use serde::{Deserialize, Serialize};

/// Off: the pilot's command passes through. Pilot: a key is held; SAS stops spin on the other
/// axes. Damping: keys released, SAS brings the spin down before it locks. Holding: SAS holds the
/// locked attitude.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SasPhase {
    Off,
    Pilot,
    Damping,
    Holding,
}

impl SasPhase {
    /// The phase's display text.
    pub fn label(self) -> &'static str {
        match self {
            SasPhase::Off => "off",
            SasPhase::Pilot => "pilot input: other axes damped",
            SasPhase::Damping => "stopping the spin",
            SasPhase::Holding => "holding attitude",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
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

/// The sign: zero (of either sign) and NaN come back unchanged.
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
    let mut r = target.conjugate() * current;
    if r.w < 0.0 {
        r = -r;
    }
    let s = r.length();
    if s == 0.0 {
        return DVec3::ZERO;
    }
    let angle = 2.0 * f64::atan2(s, r.w);
    DVec3::new(r.x / s * angle, r.y / s * angle, r.z / s * angle)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StabilityAssist {
    max_torque: f64,
    tuning: SasTuning,
    phase: SasPhase,
    locked: Option<DQuat>,
}

impl StabilityAssist {
    /// `max_torque`: N m per unit of turn command on each axis.
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
        inertia: &DMat3,
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
        for (i, v) in inertia.to_cols_array().iter().enumerate() {
            assert!(
                v.is_finite() && (i % 4 != 0 || *v > 0.0),
                "stability assist: inertia {inertia:?}"
            );
        }
        // The spin in local axes.
        let w = rotation.conjugate() * angular_velocity;
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
                if w.length() < lock_rate {
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
                    spin(e.x, inertia.x_axis.x),
                    spin(e.y, inertia.y_axis.y),
                    spin(e.z, inertia.z_axis.z),
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
        let torque = *inertia * alpha + w.cross(*inertia * w);
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
