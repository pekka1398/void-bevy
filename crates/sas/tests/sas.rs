//! The stability assist on a rigid craft integrated with `rotation_step`: holding against a kick,
//! pilot pass-through, release and relock, and rejected input.

use std::f64::consts::{FRAC_1_SQRT_2, PI};

use glam::{DMat3, DQuat, DVec3};
use void_rotation::rotation_step;
use void_sas::{SAS_TUNING, SasPhase, SasTuning, StabilityAssist, attitude_error};

const DT: f64 = 1.0 / 60.0;
/// The largest steering torque, N m.
const STEERING_TORQUE: f64 = 6000.0;
const DEG: f64 = 180.0 / PI;

fn angle(a: DQuat, b: DQuat) -> f64 {
    attitude_error(a, b).length()
}

fn tilted() -> DQuat {
    let (x, y, z, w) = (0.12, -0.3, 0.2, 0.92);
    let l = DQuat::from_xyzw(x, y, z, w).length();
    DQuat::from_xyzw(x / l, y / l, z / l, w / l)
}

fn local(rotation: DQuat, w: DVec3) -> DVec3 {
    rotation.conjugate() * w
}

/// A two-stage rocket's inertia (kg m², +y the long axis) as a stack and as the upper stage.
fn inertias() -> (DMat3, DMat3) {
    (
        DMat3::from_diagonal(DVec3::new(16000.0, 6000.0, 16000.0)),
        DMat3::from_diagonal(DVec3::new(1400.0, 1000.0, 1400.0)),
    )
}

/// A rigid craft on the attitude integrator with no damping.
struct Craft {
    rotation: DQuat,
    angular_velocity: DVec3,
    largest_command: f64,
    inertia: DMat3,
    sas: StabilityAssist,
}

impl Craft {
    fn new(inertia: DMat3) -> Self {
        Self {
            rotation: tilted(),
            angular_velocity: DVec3::ZERO,
            largest_command: 0.0,
            inertia,
            sas: StabilityAssist::new(STEERING_TORQUE, SAS_TUNING),
        }
    }

    fn step(&mut self, pilot: DVec3) -> DVec3 {
        let u = self.sas.command(
            self.rotation,
            self.angular_velocity,
            &self.inertia,
            pilot,
            DT,
        );
        self.largest_command = self.largest_command.max(u.abs().max_element());
        (self.rotation, self.angular_velocity) = rotation_step(
            self.rotation,
            self.angular_velocity,
            &self.inertia,
            u * STEERING_TORQUE,
            DVec3::ZERO,
            DT,
        );
        u
    }

    fn run(&mut self, seconds: f64, pilot: DVec3, mut each: impl FnMut(&Self, f64)) {
        for k in 0..(seconds / DT).round() as usize {
            self.step(pilot);
            each(self, k as f64 * DT);
        }
    }
}

fn scaled(m: DMat3, k: f64) -> DMat3 {
    m * k
}

#[test]
fn holds_against_a_kick() {
    let (stack, upper) = inertias();
    println!(
        "inertia (kg m², local diagonal): stack {:.0} / {:.0} / {:.0}, upper {:.0} / {:.0} / {:.0}",
        stack.x_axis.x,
        stack.y_axis.y,
        stack.z_axis.z,
        upper.x_axis.x,
        upper.y_axis.y,
        upper.z_axis.z
    );
    for (label, inertia) in [
        ("full stack", stack),
        ("upper stage", upper),
        ("stack x4 (heavier craft)", scaled(stack, 4.0)),
        ("upper x0.25 (nearly empty)", scaled(upper, 0.25)),
    ] {
        let mut craft = Craft::new(inertia);
        craft.sas.set_enabled(true);
        craft.run(0.5, DVec3::ZERO, |_, _| {});
        let lock = craft.sas.target().expect("locked");
        let locked_at_once =
            craft.sas.phase() == SasPhase::Holding && angle(lock, tilted()) < 1e-12;
        // Kick: an off-axis spin of 0.2 rad/s in the frame.
        craft.angular_velocity = DVec3::new(0.12, -0.1, 0.13);
        let kick = craft.angular_velocity.length();
        let (mut peak, mut settled, mut close_in, mut overshoot) =
            (0.0_f64, f64::INFINITY, f64::INFINITY, 0.0_f64);
        craft.run(30.0, DVec3::ZERO, |c, t| {
            let a = angle(lock, c.rotation);
            peak = peak.max(a);
            if close_in == f64::INFINITY && peak > 0.0 && a < 0.01 * peak {
                close_in = t;
            }
            if close_in != f64::INFINITY {
                overshoot = overshoot.max(a);
            }
            if a < 0.1 / DEG && c.angular_velocity.length() < 1e-3 {
                if settled == f64::INFINITY {
                    settled = t;
                }
            } else {
                settled = f64::INFINITY;
            }
        });
        let fin = angle(lock, craft.rotation);
        println!(
            "{label}: kick {kick:.2} rad/s turned it {:.2}° away; back within 0.1° and 1e-3 rad/s after {settled:.2} s, overshoot {:.2}% of the peak, final {:.1e}°, largest command {:.3}",
            peak * DEG,
            overshoot / peak * 100.0,
            fin * DEG,
            craft.largest_command
        );
        assert!(
            locked_at_once
                && fin < 0.01 / DEG
                && settled < 15.0
                && overshoot < 0.05 * peak
                && craft.largest_command <= 1.0 + 1e-12,
            "holds against a kick: {label}"
        );
    }
}

#[test]
fn pilot_input_release_and_new_lock() {
    let (stack, _) = inertias();
    let mut craft = Craft::new(stack);
    craft.sas.set_enabled(true);
    craft.run(0.5, DVec3::ZERO, |_, _| {});
    let original = craft.sas.target().unwrap();
    // Off-axis drift while the pilot pitches: SAS must stop it on the free axes.
    craft.angular_velocity = craft.rotation * DVec3::new(0.0, 0.05, 0.05);
    let mut pass_through = true;
    for _ in 0..60 {
        if craft.step(DVec3::X).x != 1.0 {
            pass_through = false;
        }
    }
    let phase_while_held = craft.sas.phase();
    let spin = local(craft.rotation, craft.angular_velocity);
    let (pitch_rate, off_axis) = (spin.x.abs(), spin.y.hypot(spin.z));
    let (mut locked_at, mut lock_time) = (None, f64::INFINITY);
    craft.run(15.0, DVec3::ZERO, |c, t| {
        if locked_at.is_none() && c.sas.phase() == SasPhase::Holding {
            locked_at = c.sas.target();
            lock_time = t;
        }
    });
    let locked_at = locked_at.expect("locked again");
    let (drift, moved) = (angle(locked_at, craft.rotation), angle(original, locked_at));
    println!(
        "1 s of full pitch: command stayed 1, pitch rate {pitch_rate:.3} rad/s; roll/yaw drift 0.071 -> {off_axis:.2e} rad/s"
    );
    println!(
        "locked {lock_time:.2} s after release, {:.1}° from the old lock; 15 s later {:.1e}° from the new one",
        moved * DEG,
        drift * DEG
    );
    assert!(
        pass_through
            && phase_while_held == SasPhase::Pilot
            && off_axis < 0.1 * 0.05 * 2.0 * FRAC_1_SQRT_2
            && pitch_rate > 0.2,
        "pilot axis passes through, other axes are damped"
    );
    assert!(
        moved > 5.0 / DEG && drift < 0.01 / DEG,
        "release damps, then locks the new attitude"
    );
}

#[test]
fn off_passes_through_and_does_not_stop_a_spin() {
    let (_, upper) = inertias();
    let mut craft = Craft::new(upper);
    let pilot = DVec3::new(0.3, -1.0, 0.0);
    let same = craft.step(pilot);
    // Spin about the long axis (a principal axis): with SAS off and no damping it keeps going.
    craft.rotation = DQuat::IDENTITY;
    craft.angular_velocity = DVec3::new(0.0, 0.4, 0.0);
    craft.run(10.0, DVec3::ZERO, |_, _| {});
    let kept = craft.angular_velocity.length();
    println!("spin 0.4 rad/s after 10 s: {kept:.12} rad/s");
    assert!(same == pilot && craft.sas.phase() == SasPhase::Off && (kept - 0.4).abs() < 1e-12);
}

#[test]
fn turned_on_while_spinning() {
    let (stack, _) = inertias();
    let mut craft = Craft::new(stack);
    craft.angular_velocity = DVec3::new(0.3, 0.2, -0.1);
    craft.sas.set_enabled(true);
    let (mut lock_time, mut rate_at_lock) = (f64::INFINITY, f64::INFINITY);
    craft.run(20.0, DVec3::ZERO, |c, t| {
        if lock_time == f64::INFINITY && c.sas.phase() == SasPhase::Holding {
            lock_time = t;
            rate_at_lock = c.angular_velocity.length();
        }
    });
    let drift = angle(craft.sas.target().unwrap(), craft.rotation);
    println!(
        "0.37 rad/s stopped and locked after {lock_time:.2} s (spin {rate_at_lock:.1e} rad/s); then {:.1e}° from the lock",
        drift * DEG
    );
    assert!(
        lock_time < 10.0
            && rate_at_lock < 3e-3
            && drift < 0.01 / DEG
            && craft.largest_command <= 1.0 + 1e-12
    );
}

#[test]
fn bad_input_panics() {
    let stack = DMat3::from_diagonal(DVec3::new(9000.0, 2000.0, 9000.0));
    let panics =
        |run: &dyn Fn()| std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).is_err();
    let on = || {
        let mut sas = StabilityAssist::new(STEERING_TORQUE, SAS_TUNING);
        sas.set_enabled(true);
        sas
    };
    let cases = [
        panics(&|| {
            StabilityAssist::new(0.0, SAS_TUNING);
        }),
        panics(&|| {
            StabilityAssist::new(
                STEERING_TORQUE,
                SasTuning {
                    rate_seconds: 0.2,
                    attitude_seconds: 0.5,
                    brake_fraction: 0.5,
                    lock_rate: 1e-3,
                },
            );
        }),
        panics(&|| {
            on().command(tilted(), DVec3::ZERO, &stack, DVec3::new(1.5, 0.0, 0.0), DT);
        }),
        panics(&|| {
            on().command(tilted(), DVec3::ZERO, &stack, DVec3::ZERO, 0.0);
        }),
        panics(&|| {
            on().command(
                tilted(),
                DVec3::ZERO,
                &DMat3::from_diagonal(DVec3::new(0.0, 1.0, 1.0)),
                DVec3::ZERO,
                DT,
            );
        }),
        panics(&|| {
            on().command(
                tilted(),
                DVec3::ZERO,
                &DMat3::from_diagonal(DVec3::new(1.0, f64::NAN, 1.0)),
                DVec3::ZERO,
                DT,
            );
        }),
    ];
    println!("thrown: {cases:?} (torque 0, loose tuning, pilot 1.5, dt 0, zero and NaN inertia)");
    assert!(cases.iter().all(|&c| c));
}
