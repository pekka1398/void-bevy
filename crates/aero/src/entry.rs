//! A capsule's 6-DOF reentry on spinning, spherical Terra, as the lab's `Entry.ts`: position,
//! velocity, attitude and spin integrated together in landing's body-fixed `PlanetFrame` with
//! orbit's Dormand–Prince stepper; heat and ablator change only after an accepted step.

use glam::{DQuat, DVec3};
use void_environment::{BodyEnvironment, Environment};
use void_frames::State;
use void_landing::{ContactFrame, PlanetFrame, earth_size, planet_ephemeris};
use void_math::{cos, pow, sin};
use void_orbit::{Dopri5, Ephemeris};

use crate::{
    AeroState, Air, Atmosphere, DEG, NEUTRAL, Vehicle, VehicleLoads, VehicleResources,
    advance_heat, align, evaluate_vehicle, finite, inverse, length, mass_properties, quat_multiply,
    resources, rotate, unit,
};
use void_math::hypot;

/// Initial conditions: altitude, airspeed relative to the turning ground, flight-path angle
/// (negative descends), angle of attack and bank.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EntryOptions {
    pub altitude_meters: f64,
    pub speed: f64,
    pub flight_path_degrees: f64,
    pub angle_of_attack_degrees: f64,
    pub bank_degrees: f64,
}

pub const DEFAULT_ENTRY: EntryOptions = EntryOptions {
    altitude_meters: 120_000.0,
    speed: 7600.0,
    flight_path_degrees: -2.0,
    angle_of_attack_degrees: 0.0,
    bank_degrees: 0.0,
};

const N: usize = 13;
/// Radiative background in the entry scene, K.
const BACKGROUND_K: f64 = 180.0;

pub struct EntryFlight {
    pub vehicle: Vehicle,
    pub options: EntryOptions,
    /// Terra's gravity and the chosen atmosphere; air is read at the body-fixed state.
    pub environment: Environment,
    pub ephemeris: Ephemeris,
    pub frame: PlanetFrame,
    pub resources: VehicleResources,
    /// Body-fixed position and velocity, body-to-planet quaternion, and the inertial angular
    /// velocity in body axes.
    pub y: [f64; N],
    stepper: Dopri5<N>,
    pub time: f64,
    pub step_hint: f64,
    pub accepted_steps: u64,
    pub loads: VehicleLoads,
    /// Why the run stopped: a failure, or touchdown with its speed.
    pub terminal: Option<String>,
    pub max_q_pa: f64,
    pub max_flux_wm2: f64,
    pub max_g: f64,
    pub heat_j: f64,
}

/// The parts the derivative reads, so the stepper can be borrowed beside them.
struct Model<'a> {
    vehicle: &'a Vehicle,
    resources: &'a VehicleResources,
    environment: &'a Environment,
    ephemeris: &'a Ephemeris,
    frame: &'a PlanetFrame,
}

fn quat_of(y: &[f64; N]) -> DQuat {
    unit(DQuat::from_xyzw(y[6], y[7], y[8], y[9]))
}

impl Model<'_> {
    fn evaluate(&self, y: &[f64; N]) -> VehicleLoads {
        let position = DVec3::new(y[0], y[1], y[2]);
        let velocity = DVec3::new(y[3], y[4], y[5]);
        let rotation = quat_of(y);
        let spin = DVec3::new(0.0, 0.0, self.frame.omega);
        // Body-fixed velocities already exclude the planet's turning; still air is zero here.
        evaluate_vehicle(
            self.vehicle,
            self.resources,
            &AeroState {
                center: position,
                velocity,
                rotation,
                angular_velocity: rotate(rotation, DVec3::new(y[10], y[11], y[12])) - spin,
            },
            &self
                .environment
                .surroundings_local(self.frame.body.index, State { position, velocity })
                .air
                .map_or(Air::VACUUM, |a| a.air),
            DVec3::ZERO,
            &NEUTRAL,
            BACKGROUND_K,
        )
    }

    fn derivative(&self, t: f64, y: &[f64; N], dy: &mut [f64; N]) {
        let position = DVec3::new(y[0], y[1], y[2]);
        let velocity = DVec3::new(y[3], y[4], y[5]);
        let q = quat_of(y);
        let w = DVec3::new(y[10], y[11], y[12]);
        let loads = self.evaluate(y);
        let props = mass_properties(self.vehicle, self.resources);
        let acceleration = self
            .frame
            .acceleration(self.ephemeris, t, position, velocity)
            + loads.aero.force * (1.0 / props.mass);
        let torque = rotate(inverse(q), loads.aero.torque);
        let inertia = props.inertia;
        let gyroscopic = w.cross(DVec3::new(
            inertia.x * w.x,
            inertia.y * w.y,
            inertia.z * w.z,
        ));
        let rw = w - rotate(inverse(q), DVec3::new(0.0, 0.0, self.frame.omega));
        *dy = [
            velocity.x,
            velocity.y,
            velocity.z,
            acceleration.x,
            acceleration.y,
            acceleration.z,
            0.5 * (q.w * rw.x + q.y * rw.z - q.z * rw.y),
            0.5 * (q.w * rw.y + q.z * rw.x - q.x * rw.z),
            0.5 * (q.w * rw.z + q.x * rw.y - q.y * rw.x),
            -0.5 * (q.x * rw.x + q.y * rw.y + q.z * rw.z),
            (torque.x - gyroscopic.x) / inertia.x,
            (torque.y - gyroscopic.y) / inertia.y,
            (torque.z - gyroscopic.z) / inertia.z,
        ];
    }
}

impl EntryFlight {
    pub fn new(vehicle: Vehicle, options: EntryOptions, atmosphere: Atmosphere) -> Self {
        for (value, key) in [
            (options.altitude_meters, "altitudeMeters"),
            (options.speed, "speed"),
            (options.flight_path_degrees, "flightPathDegrees"),
            (options.angle_of_attack_degrees, "angleOfAttackDegrees"),
            (options.bank_degrees, "bankDegrees"),
        ] {
            finite(value, key);
        }
        let resources = resources(&vehicle);
        let (ephemeris, body_index) = planet_ephemeris(&earth_size());
        let frame = PlanetFrame::new(&ephemeris, body_index);
        let environment = Environment::new(&ephemeris).with(
            body_index,
            BodyEnvironment {
                atmosphere: Some(atmosphere),
                air_datum_meters: 0.0,
                terrain: None,
                sea_level_meters: None,
            },
        );
        assert!(
            options.altitude_meters >= 1000.0
                && options.speed > 0.0
                && options.flight_path_degrees.abs() <= 89.0
                && options.angle_of_attack_degrees.abs() <= 180.0,
            "Invalid reentry initial condition"
        );
        let gamma = options.flight_path_degrees * DEG;
        let position = DVec3::new(frame.body.radius_meters + options.altitude_meters, 0.0, 0.0);
        let velocity = DVec3::new(options.speed * sin(gamma), options.speed * cos(gamma), 0.0);
        let pitch = gamma + options.angle_of_attack_degrees * DEG;
        let heading = DVec3::new(sin(pitch), cos(pitch), 0.0);
        let half_bank = options.bank_degrees * DEG / 2.0;
        let q = quat_multiply(
            align(DVec3::Z, heading),
            DQuat::from_xyzw(0.0, 0.0, sin(half_bank), cos(half_bank)),
        );
        let w = rotate(inverse(q), DVec3::new(0.0, 0.0, frame.omega));
        let y = [
            position.x, position.y, position.z, velocity.x, velocity.y, velocity.z, q.x, q.y, q.z,
            q.w, w.x, w.y, w.z,
        ];
        let loads = Model {
            vehicle: &vehicle,
            resources: &resources,
            environment: &environment,
            ephemeris: &ephemeris,
            frame: &frame,
        }
        .evaluate(&y);
        Self {
            vehicle,
            options,
            environment,
            ephemeris,
            frame,
            resources,
            y,
            stepper: Dopri5::default(),
            time: 0.0,
            step_hint: 0.05,
            accepted_steps: 0,
            loads,
            terminal: None,
            max_q_pa: 0.0,
            max_flux_wm2: 0.0,
            max_g: 0.0,
            heat_j: 0.0,
        }
    }

    pub fn position(&self) -> DVec3 {
        DVec3::new(self.y[0], self.y[1], self.y[2])
    }

    pub fn velocity(&self) -> DVec3 {
        DVec3::new(self.y[3], self.y[4], self.y[5])
    }

    pub fn rotation(&self) -> DQuat {
        quat_of(&self.y)
    }

    pub fn altitude(&self) -> f64 {
        length(self.position()) - self.frame.body.radius_meters
    }

    /// Integrate `seconds` forward, at most `max_steps` tries (accepted or not), stopping early at
    /// a terminal event. Steps adapt to the error; none crosses 1 m altitude in one go.
    pub fn advance(&mut self, seconds: f64, max_steps: u32) {
        assert!(
            seconds >= 0.0 && seconds.is_finite(),
            "Invalid reentry duration"
        );
        assert!(max_steps > 0, "Invalid reentry step budget");
        let target = self.time + seconds;
        let radius = self.frame.body.radius_meters;
        let (mut dy, mut next, mut next_dy) = ([0.0; N], [0.0; N], [0.0; N]);
        let mut iterations = 0;
        while self.time < target - 1e-10 && self.terminal.is_none() && iterations < max_steps {
            iterations += 1;
            let dt = self.step_hint.min(0.1).min(target - self.time);
            assert!(dt >= 1e-8, "Reentry integrator step underflow");
            self.ephemeris.extend_to(self.time + dt);
            let model = Model {
                vehicle: &self.vehicle,
                resources: &self.resources,
                environment: &self.environment,
                ephemeris: &self.ephemeris,
                frame: &self.frame,
            };
            model.derivative(self.time, &self.y, &mut dy);
            self.stepper.step(
                &mut |t, y, out| model.derivative(t, y, out),
                self.time,
                &self.y,
                &dy,
                dt,
                &mut next,
                &mut next_dy,
            );
            let mut error: f64 = 0.0;
            for (i, e) in self.stepper.error.iter().enumerate() {
                let scale = if i < 3 {
                    0.01
                } else if i < 6 {
                    0.001
                } else {
                    1e-6
                };
                error = error.max(e.abs() / scale);
            }
            assert!(error.is_finite(), "Reentry non-finite integration error");
            if error > 1.0 {
                self.step_hint = dt * 0.2_f64.max(0.9 * pow(error, -0.2));
                continue;
            }
            // Stop at a physical ground event: bisect by shortening the step, not by teleporting.
            let next_altitude = hypot([next[0], next[1], next[2]]) - radius;
            if next_altitude < 1.0 && self.altitude() > 1.0 && dt > 0.0001 {
                self.step_hint = dt * 0.5;
                continue;
            }
            self.y = next;
            self.time += dt;
            self.accepted_steps += 1;
            let q = self.rotation();
            self.y[6..10].copy_from_slice(&[q.x, q.y, q.z, q.w]);
            self.loads = Model {
                vehicle: &self.vehicle,
                resources: &self.resources,
                environment: &self.environment,
                ephemeris: &self.ephemeris,
                frame: &self.frame,
            }
            .evaluate(&self.y);
            let (budgets, failure) =
                advance_heat(&self.vehicle, &mut self.resources, &self.loads, dt);
            self.terminal = failure;
            if self.altitude() <= 1.0 && self.terminal.is_none() {
                self.terminal = Some(format!("touchdown at {:.1} m/s", length(self.velocity())));
            }
            self.max_q_pa = self.max_q_pa.max(self.loads.aero.q_pa);
            let mass = mass_properties(&self.vehicle, &self.resources).mass;
            self.max_g = self
                .max_g
                .max(length(self.loads.aero.force) / mass / 9.80665);
            for heat in &self.loads.heat {
                self.max_flux_wm2 = self.max_flux_wm2.max(heat.load.flux_wm2);
            }
            for b in &budgets {
                self.heat_j += b.incoming_j;
            }
            self.step_hint = dt
                * if error == 0.0 {
                    2.0
                } else {
                    2.0_f64.min(0.5_f64.max(0.9 * pow(error, -0.2)))
                };
            for v in self.y {
                finite(v, "reentry state");
            }
        }
    }
}
