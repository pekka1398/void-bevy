//! The aircraft flying on Rapier over flat ground, as the lab's `Flight.ts`: one compound body,
//! aerodynamic loads applied as impulses at each element, a simple air-breathing engine that burns
//! fuel, and fixed rolling gear. Controls only move the control surfaces: there is no direct
//! steering torque, artificial damping or SAS.

use glam::{DQuat, DVec3};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::*;
use void_math::pow;

use crate::{
    AeroState, Atmosphere, Controls, EngineKind, NEUTRAL, PartShape, Vehicle, VehicleLoads,
    VehicleResources, advance_heat, clamp, evaluate_vehicle, finite, length, part_mass, resources,
    rotate, unit,
};

/// 120 Hz, as the lab.
pub const FLIGHT_STEP: f64 = 1.0 / 120.0;
const GRAVITY: f64 = 9.80665;
/// A velocity change larger than this in one step below 8 m altitude is a crash.
const CRASH_DELTA_V: f64 = 18.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlightStart {
    /// 700 m up at 70 m/s, level.
    Cruise,
    /// At rest on the runway.
    Runway,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FlightCommand {
    pub controls: Controls,
    /// 0 to 1.
    pub throttle: f64,
    pub brakes: bool,
}

pub struct AircraftFlight {
    pub vehicle: Vehicle,
    pub atmosphere: Atmosphere,
    pub world: PhysicsWorld,
    pub body: RigidBodyHandle,
    /// One collider per part, in part order.
    pub colliders: Vec<ColliderHandle>,
    pub wheel_colliders: Vec<ColliderHandle>,
    pub resources: VehicleResources,
    pub time: f64,
    pub loads: VehicleLoads,
    /// Why the trial stopped: a part overheated or overloaded, or a crash.
    pub failure: Option<String>,
    pub wind: DVec3,
    pub fuel_used_kg: f64,
    pub thrust_n: f64,
    pub max_q_pa: f64,
    pub max_altitude: f64,
}

pub(crate) fn v32(v: DVec3) -> Vector {
    Vector::new(v.x as f32, v.y as f32, v.z as f32)
}

pub(crate) fn v64(v: Vector) -> DVec3 {
    DVec3::new(v.x as f64, v.y as f64, v.z as f64)
}

fn q32(q: DQuat) -> Rotation {
    Rotation::from_xyzw(q.x as f32, q.y as f32, q.z as f32, q.w as f32)
}

pub(crate) fn q64(q: Rotation) -> DQuat {
    DQuat::from_xyzw(q.x as f64, q.y as f64, q.z as f64, q.w as f64)
}

impl AircraftFlight {
    pub fn new(vehicle: Vehicle, start: FlightStart, atmosphere: Atmosphere) -> Self {
        let resources = resources(&vehicle);
        let mut world = PhysicsWorld {
            gravity: Vector::new(0.0, -GRAVITY as f32, 0.0),
            ..PhysicsWorld::default()
        };
        world.integration_parameters.dt = FLIGHT_STEP as f32;
        world.colliders.insert(
            ColliderBuilder::cuboid(100_000.0, 0.1, 100_000.0)
                .translation(Vector::new(0.0, -0.1, 0.0))
                .friction(0.7),
        );
        let cruise = start == FlightStart::Cruise;
        let body = world.bodies.insert(
            RigidBodyBuilder::dynamic()
                .translation(Vector::new(0.0, if cruise { 700.0 } else { 1.09 }, 0.0))
                .linvel(Vector::new(0.0, 0.0, if cruise { 70.0 } else { 0.0 }))
                .angular_damping(0.0)
                .linear_damping(0.0)
                .ccd_enabled(true),
        );
        let colliders = vehicle
            .parts
            .iter()
            .map(|p| {
                let builder = match p.shape {
                    PartShape::Box { size } => ColliderBuilder::cuboid(
                        size.x as f32 / 2.0,
                        size.y as f32 / 2.0,
                        size.z as f32 / 2.0,
                    ),
                    PartShape::Cone { radius, length } => {
                        ColliderBuilder::cone(length as f32 / 2.0, radius as f32)
                    }
                    PartShape::Cylinder { radius, length } => {
                        ColliderBuilder::cylinder(length as f32 / 2.0, radius as f32)
                    }
                };
                world.colliders.insert_with_parent(
                    builder
                        .position(Pose::from_parts(v32(p.position), q32(p.rotation)))
                        .friction(0.5),
                    body,
                    &mut world.bodies,
                )
            })
            .collect();
        let wheel_colliders = vehicle
            .wheels
            .iter()
            .map(|wheel| {
                world.colliders.insert_with_parent(
                    ColliderBuilder::ball(wheel.radius as f32)
                        .translation(v32(wheel.position))
                        .friction(0.0)
                        .friction_combine_rule(CoefficientCombineRule::Min)
                        .restitution(0.0)
                        .mass(0.0),
                    body,
                    &mut world.bodies,
                )
            })
            .collect();
        // Placeholder loads, replaced at once below.
        let loads = VehicleLoads {
            aero: crate::AeroForces {
                force: DVec3::ZERO,
                torque: DVec3::ZERO,
                elements: vec![],
                q_pa: 0.0,
                speed: 0.0,
                mach: 0.0,
            },
            heat: vec![],
        };
        let mut flight = Self {
            vehicle,
            atmosphere,
            world,
            body,
            colliders,
            wheel_colliders,
            resources,
            time: 0.0,
            loads,
            failure: None,
            wind: DVec3::ZERO,
            fuel_used_kg: 0.0,
            thrust_n: 0.0,
            max_q_pa: 0.0,
            max_altitude: 0.0,
        };
        flight.apply_mass();
        flight.loads = flight.evaluate(&NEUTRAL);
        flight
    }

    pub fn rigid_body(&self) -> &RigidBody {
        &self.world.bodies[self.body]
    }

    /// Each part's collider takes the part's current mass (fuel and ablator included) with its
    /// shape's principal inertia; the body sums them.
    fn apply_mass(&mut self) {
        for (i, p) in self.vehicle.parts.iter().enumerate() {
            let m = part_mass(&self.vehicle, i, &self.resources);
            let inertia = match p.shape {
                PartShape::Box { size } => DVec3::new(
                    m * (size.y * size.y + size.z * size.z) / 12.0,
                    m * (size.x * size.x + size.z * size.z) / 12.0,
                    m * (size.x * size.x + size.y * size.y) / 12.0,
                ),
                PartShape::Cylinder { radius, length } | PartShape::Cone { radius, length } => {
                    let side = m * (3.0 * (radius * radius) + length * length) / 12.0;
                    DVec3::new(side, m * (radius * radius) / 2.0, side)
                }
            };
            self.world.colliders[self.colliders[i]].set_mass_properties(
                MassProperties::with_principal_inertia_frame(
                    Vector::ZERO,
                    m as f32,
                    v32(inertia),
                    Rotation::IDENTITY,
                ),
            );
        }
        self.world.bodies[self.body]
            .recompute_mass_properties_from_colliders(&self.world.colliders);
    }

    fn evaluate(&self, controls: &Controls) -> VehicleLoads {
        let b = self.rigid_body();
        let center = v64(b.center_of_mass());
        evaluate_vehicle(
            &self.vehicle,
            &self.resources,
            &AeroState {
                center,
                velocity: v64(b.linvel()),
                rotation: unit(q64(*b.rotation())),
                angular_velocity: v64(b.angvel()),
            },
            &self.atmosphere.sample(center.y),
            self.wind,
            controls,
            250.0,
        )
    }

    /// The centre of mass, world.
    pub fn position(&self) -> DVec3 {
        v64(self.rigid_body().center_of_mass())
    }

    pub fn velocity(&self) -> DVec3 {
        v64(self.rigid_body().linvel())
    }

    pub fn rotation(&self) -> DQuat {
        q64(*self.rigid_body().rotation())
    }

    pub fn altitude(&self) -> f64 {
        self.position().y
    }

    pub fn fuel_kg(&self) -> f64 {
        self.resources.fuel_kg()
    }

    pub fn step(&mut self, command: &FlightCommand) {
        if self.failure.is_some() {
            return;
        }
        assert!(
            command.throttle.is_finite() && (0.0..=1.0).contains(&command.throttle),
            "Invalid throttle"
        );
        let dt = FLIGHT_STEP;
        let air = self.atmosphere.sample(self.altitude());
        self.loads = self.evaluate(&command.controls);
        let body = &mut self.world.bodies[self.body];
        for element in &self.loads.aero.elements {
            body.apply_impulse_at_point(v32(element.force * dt), v32(element.point), true);
            body.apply_torque_impulse(v32(element.moment * dt), true);
        }
        self.thrust_n = 0.0;
        for engine in &self.vehicle.engines {
            let p = &self.vehicle.parts[self.vehicle.part_index(&engine.part_id)];
            let tanks: Vec<usize> = engine
                .tank_ids
                .iter()
                .map(|id| self.vehicle.part_index(id))
                .collect();
            let available = tanks.iter().fold(0.0, |n, &i| n + self.resources.fuel[i]);
            let lapse = match engine.kind {
                EngineKind::Rocket => 1.0,
                EngineKind::Airbreathing if air.pressure_pa < engine.minimum_pressure_pa => 0.0,
                EngineKind::Airbreathing => {
                    pow(air.density / 1.225, 0.7) / (1.0 + pow(self.loads.aero.mach * 0.35, 2.0))
                }
            };
            let requested_thrust = command.throttle * engine.thrust_newtons * lapse;
            let requested_fuel = requested_thrust * dt / (engine.isp_seconds * GRAVITY);
            let used = available.min(requested_fuel);
            let actual_thrust = if requested_fuel > 0.0 {
                requested_thrust * used / requested_fuel
            } else {
                0.0
            };
            for &i in &tanks {
                let fuel = self.resources.fuel[i];
                self.resources.fuel[i] = if available > 0.0 {
                    fuel * (1.0 - used / available)
                } else {
                    0.0
                };
            }
            let attitude = q64(*body.rotation());
            let direction = rotate(attitude, rotate(p.rotation, engine.direction));
            let point = v64(body.translation()) + rotate(attitude, p.position);
            body.apply_impulse_at_point(v32(direction * (actual_thrust * dt)), v32(point), true);
            self.fuel_used_kg += used;
            self.thrust_n += actual_thrust;
        }
        // Fixed rolling gear: lateral tyre force and rolling resistance, Coulomb-limited by an
        // approximate load. Contact normals and impacts are Rapier's; no orientation or steering
        // torque is imposed.
        let attitude = q64(*body.rotation());
        let (right, forward) = (rotate(attitude, DVec3::X), rotate(attitude, DVec3::Z));
        let wheel_count = self.vehicle.wheels.len();
        for wheel in &self.vehicle.wheels {
            let point = v64(body.translation()) + rotate(attitude, wheel.position);
            if point.y - wheel.radius > 0.035 {
                continue;
            }
            let velocity =
                v64(body.linvel()) + v64(body.angvel()).cross(point - v64(body.center_of_mass()));
            let mass = body.mass() as f64 / 1.0_f64.max(wheel_count as f64);
            let limit = mass * GRAVITY * dt;
            let lateral = clamp(-velocity.dot(right) * mass, -0.7 * limit, 0.7 * limit);
            let grip = if command.brakes { 0.6 } else { 0.015 };
            let rolling = clamp(-velocity.dot(forward) * mass, -grip * limit, grip * limit);
            body.apply_impulse_at_point(v32(right * lateral + forward * rolling), v32(point), true);
        }
        let before_contact = v64(body.linvel());
        self.world.step();
        self.time += dt;
        let (_, failure) = advance_heat(&self.vehicle, &mut self.resources, &self.loads, dt);
        self.failure = failure;
        self.apply_mass();
        self.max_q_pa = self.max_q_pa.max(self.loads.aero.q_pa);
        self.max_altitude = self.max_altitude.max(self.altitude());
        let b = self.rigid_body();
        for x in v64(b.linvel())
            .to_array()
            .into_iter()
            .chain(v64(b.angvel()).to_array())
        {
            finite(x, "Rapier flight state");
        }
        // A destructive collision is a physical end of the trial; runway rolling is below this.
        if self.altitude() < 8.0 && length(v64(b.linvel()) - before_contact) > CRASH_DELTA_V {
            self.failure = Some("destructive ground impact".into());
        }
    }
}
