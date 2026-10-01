//! One vessel near a rotating planet, as `lab/landing/src/vessel/Lander.ts`, in one of two modes:
//! - flight: the orbit crate's integrator in the inertial frame, while above the terrain band
//!   (highest terrain + `band_enter_meters`). Chunks are bounded so the band cannot be crossed
//!   unseen between checks.
//! - contact: a Rapier body in the planet's rotating frame, inside the band. Contact remains a live
//!   rigid body after touchdown, including at rest.
//!
//! States cross between modes unchanged (converted between frames).

use std::sync::Arc;

use glam::{DQuat, DVec3};
use rapier3d::prelude::RigidBodyHandle;
use void_math::hypot;
use void_orbit::{
    AdvanceOutcome, AttitudeLaw, Control, Ephemeris, PropagationRun, ThrustControl, Tolerances,
    VesselPropagator, VesselState, body_orientation,
};
use void_terrain::Terrain;

use crate::contact_world::{
    BodyShape, ContactBodySpec, ContactWorld, ContactWorldOptions, SimpleShape,
};
use crate::planet_frame::{FrameState, PlanetFrame};

/// Standard gravity for specific impulse, m/s².
pub const STANDARD_GRAVITY: f64 = 9.80665;

#[derive(Clone, Debug, PartialEq)]
pub struct LanderSpec {
    pub thrust_newtons: f64,
    pub specific_impulse_seconds: f64,
    pub dry_mass_kg: f64,
    pub fuel_mass_kg: f64,
    /// The hull is a box; its local +y is the thrust axis ("up").
    pub half_extents: DVec3,
    pub contact_shape: Option<BodyShape>,
    pub friction: f64,
    /// Impact speed change a part survives in one contact step, m/s (KSP's crash tolerance).
    pub crash_tolerance_meters_per_second: Option<f64>,
}

/// Engine command, constant over one advance call. Thrust points along the orbit crate's surface
/// law: the normalised up × (local vertical) + prograde × (direction of the velocity over the
/// ground), or along `direction` when given.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LanderControl {
    pub throttle: f64,
    pub up: f64,
    pub prograde: f64,
    /// Optional body-fixed thrust axis and collider orientation for manual steering.
    pub direction: Option<DVec3>,
    pub rotation: Option<DQuat>,
    /// Angular steering about the craft's local pitch, roll and yaw axes.
    pub turn: Option<DVec3>,
    /// Planned orbital burn direction (a Frenet law); only valid for a freely flying rocket.
    pub orbital_attitude: Option<AttitudeLaw>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LanderOptions {
    pub contact: ContactWorldOptions,
    pub tolerances: Tolerances,
    /// Contact physics starts this far above the highest terrain...
    pub band_enter_meters: f64,
    /// ...and hands back to free flight this far above it (> enter).
    pub band_exit_meters: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LanderMode {
    Flight,
    Contact,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModeChange {
    pub time: f64,
    pub from: LanderMode,
    pub to: LanderMode,
}

/// Distance within which a coast chunk switches to contact instead of creeping up on the band.
const BAND_SNAP_METERS: f64 = 1.0;

pub struct Lander {
    pub frame: PlanetFrame,
    pub terrain: Arc<Terrain>,
    pub spec: LanderSpec,
    pub options: LanderOptions,
    pub mode_changes: Vec<ModeChange>,
    pub mode: LanderMode,
    pub time: f64,
    pub mass_kg: f64,
    propagator: VesselPropagator,
    run: Option<PropagationRun>,
    contact: Option<(ContactWorld<PlanetFrame>, RigidBodyHandle)>,
    flight_rotation: DQuat,
    /// Thrust acceleration over the last contact step (or the half step before entering contact),
    /// for the leapfrog average.
    previous_push: DVec3,
}

impl Lander {
    fn new(
        ephemeris: &Ephemeris,
        body_index: usize,
        terrain: Arc<Terrain>,
        spec: LanderSpec,
        options: LanderOptions,
        time: f64,
    ) -> Self {
        assert!(
            options.band_exit_meters > options.band_enter_meters && options.band_enter_meters > 0.0,
            "lander: band {}..{}",
            options.band_enter_meters,
            options.band_exit_meters
        );
        assert!(
            spec.thrust_newtons > 0.0
                && spec.specific_impulse_seconds > 0.0
                && spec.dry_mass_kg > 0.0
                && spec.fuel_mass_kg >= 0.0,
            "lander: spec {spec:?}"
        );
        Self {
            frame: PlanetFrame::new(ephemeris, body_index),
            terrain,
            propagator: VesselPropagator::new(ephemeris, options.tolerances),
            mass_kg: spec.dry_mass_kg + spec.fuel_mass_kg,
            spec,
            options,
            mode_changes: Vec::new(),
            mode: LanderMode::Flight,
            time,
            run: None,
            contact: None,
            flight_rotation: DQuat::IDENTITY,
            previous_push: DVec3::ZERO,
        }
    }

    /// A lander resting on the ground below a body-fixed direction.
    #[allow(clippy::too_many_arguments)]
    pub fn landed(
        ephemeris: &mut Ephemeris,
        body_index: usize,
        terrain: Arc<Terrain>,
        spec: LanderSpec,
        options: LanderOptions,
        time: f64,
        direction: DVec3,
    ) -> Self {
        let mut lander = Self::new(ephemeris, body_index, terrain, spec, options, time);
        let d = direction / hypot([direction.x, direction.y, direction.z]);
        // Rest the hull on the highest ground under its footprint (centre and bottom corners), a
        // centimetre up, so it starts touching nothing.
        let q = upright_at(d);
        let r0 = lander.terrain.radius_meters;
        let mut ground = lander.ground_height_under(d);
        for (sx, sz) in [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)] {
            let c = rotate(
                q,
                DVec3::new(
                    sx * lander.spec.half_extents.x,
                    0.0,
                    sz * lander.spec.half_extents.z,
                ),
            );
            let p = d * r0 + c;
            ground = ground.max(lander.ground_height_under(p / hypot([p.x, p.y, p.z])));
        }
        let r = r0 + ground + lander.spec.half_extents.y + 0.01;
        lander.enter_contact(
            ephemeris,
            FrameState {
                position: d * r,
                velocity: DVec3::ZERO,
            },
            DVec3::ZERO,
            Some(upright_at(d)),
        );
        lander
    }

    /// A lander in free flight with a body-fixed state, engine off.
    #[allow(clippy::too_many_arguments)]
    pub fn flying(
        ephemeris: &mut Ephemeris,
        body_index: usize,
        terrain: Arc<Terrain>,
        spec: LanderSpec,
        options: LanderOptions,
        time: f64,
        state: FrameState,
    ) -> Self {
        let mut lander = Self::new(ephemeris, body_index, terrain, spec, options, time);
        lander.enter_flight(ephemeris, state);
        lander.check_band(ephemeris);
        lander
    }

    pub fn exhaust_velocity(&self) -> f64 {
        self.spec.specific_impulse_seconds * STANDARD_GRAVITY
    }

    pub fn fuel_kg(&self) -> f64 {
        self.mass_kg - self.spec.dry_mass_kg
    }

    /// Actual body-fixed orientation, including collision-induced tumbling.
    pub fn orientation(&self) -> DQuat {
        match &self.contact {
            Some((world, body)) if self.mode == LanderMode::Contact => {
                let q = world.body(*body).rotation();
                DQuat::from_xyzw(
                    f64::from(q.x),
                    f64::from(q.y),
                    f64::from(q.z),
                    f64::from(q.w),
                )
            }
            _ => self.flight_rotation,
        }
    }

    /// The contact world while in contact mode.
    pub fn contact_world(&self) -> Option<&ContactWorld<PlanetFrame>> {
        self.contact.as_ref().map(|(world, _)| world)
    }

    /// Body-fixed state now.
    pub fn body_fixed_state(&self, ephemeris: &Ephemeris) -> FrameState {
        if let Some((world, body)) = &self.contact {
            return world.state(ephemeris, *body, self.previous_push);
        }
        let s = self
            .run
            .as_ref()
            .expect("a flying lander has a run")
            .state();
        self.frame.to_body_fixed(
            ephemeris,
            self.time,
            FrameState {
                position: s.position,
                velocity: s.velocity,
            },
        )
    }

    pub fn inertial_state(&self, ephemeris: &Ephemeris) -> FrameState {
        if let Some(run) = &self.run {
            let s = run.state();
            return FrameState {
                position: s.position,
                velocity: s.velocity,
            };
        }
        self.frame
            .to_inertial(ephemeris, self.time, self.body_fixed_state(ephemeris))
    }

    /// Height of the lander's reference point above the terrain directly below it.
    pub fn clearance(&self, ephemeris: &Ephemeris) -> f64 {
        let p = self.body_fixed_state(ephemeris).position;
        let r = hypot([p.x, p.y, p.z]);
        r - self.terrain.radius_meters - self.ground_height_under(p / r)
    }

    /// Advance simulated time by dt under one control. Contact mode moves in whole physics steps,
    /// so the lander may stop short of the target by less than one step; time is never skipped.
    pub fn advance(&mut self, ephemeris: &mut Ephemeris, dt: f64, control: &LanderControl) {
        assert!(dt >= 0.0 && dt.is_finite(), "lander advance({dt})");
        assert!(
            (0.0..=1.0).contains(&control.throttle),
            "throttle {}",
            control.throttle
        );
        let target = self.time + dt;
        let step = self.options.contact.step_seconds;
        ephemeris.extend_to(target);
        loop {
            if self.mode == LanderMode::Contact {
                if self.time + step > target + 1e-9 {
                    return;
                }
                self.contact_step(ephemeris, control);
                continue;
            }
            if self.time >= target {
                return;
            }
            self.flight_chunk(ephemeris, target, control);
        }
    }

    fn ground_height_under(&self, direction: DVec3) -> f64 {
        self.terrain.height(direction)
    }

    fn band_radius(&self, extra: f64) -> f64 {
        self.terrain.radius_meters + self.terrain.max_height_meters + extra
    }

    fn thrust_control(&self, control: &LanderControl) -> Option<ThrustControl> {
        if control.throttle == 0.0 || self.fuel_kg() <= 0.0 {
            return None;
        }
        Some(ThrustControl {
            thrust_newtons: control.throttle * self.spec.thrust_newtons,
            exhaust_velocity: self.exhaust_velocity(),
            minimum_mass_kg: self.spec.dry_mass_kg,
            attitude: match control.direction {
                Some(d) => AttitudeLaw::Inertial {
                    direction: self.body_direction_to_inertial(d),
                },
                None => AttitudeLaw::Surface {
                    reference_body: self.frame.body.index,
                    up: control.up,
                    prograde: control.prograde,
                },
            },
        })
    }

    fn body_direction_to_inertial(&self, d: DVec3) -> DVec3 {
        let axes = body_orientation(&self.frame.body.rotation, self.time);
        DVec3::new(
            d.x * axes[0].x + d.y * axes[1].x + d.z * axes[2].x,
            d.x * axes[0].y + d.y * axes[1].y + d.z * axes[2].y,
            d.x * axes[0].z + d.y * axes[1].z + d.z * axes[2].z,
        )
    }

    /// One bounded piece of free flight toward target.
    fn flight_chunk(&mut self, ephemeris: &mut Ephemeris, target: f64, control: &LanderControl) {
        if let Some(rotation) = control.rotation {
            self.flight_rotation = rotation;
        }
        let (planet_p, planet_v) = void_frames::BodyStates::body_state(
            &*ephemeris,
            void_frames::BodyId(self.frame.body.index),
            self.time,
        );
        let s = self
            .run
            .as_ref()
            .expect("a flying lander has a run")
            .state();
        let rel = s.position - planet_p;
        let u = s.velocity - planet_v;
        let band = self.band_radius(self.options.band_enter_meters);
        let gap = hypot([rel.x, rel.y, rel.z]) - band;
        if gap <= BAND_SNAP_METERS {
            // The engine setting carries over from flight into contact.
            let state = self.body_fixed_state(ephemeris);
            let push = match self.thrust_control(control) {
                Some(thrust) => {
                    surface_direction(state, control) * (thrust.thrust_newtons / self.mass_kg)
                }
                None => DVec3::ZERO,
            };
            self.enter_contact(ephemeris, state, push, control.rotation);
            return;
        }
        // Longest time the band cannot be reached in: |v| t + A t² / 2 = gap, A bounding every
        // acceleration (gravity at the band, 20% for J2 and tides, thrust).
        let speed = hypot([u.x, u.y, u.z]);
        let a = 1.2 * self.frame.body.gm / band.powi(2)
            + self.spec.thrust_newtons / self.spec.dry_mass_kg;
        let safe = (-speed + (speed * speed + 2.0 * a * gap).sqrt()) / a;
        let thrust = self.thrust_control(control);
        let mut end = target.min(self.time + safe.max(1e-3));
        let mut burnout = false;
        if let Some(t) = &thrust {
            let out = self.time + (self.fuel_kg() * t.exhaust_velocity) / t.thrust_newtons;
            if out <= end {
                end = out;
                burnout = true;
            }
        }
        let run = self.run.as_mut().expect("a flying lander has a run");
        let outcome = self.propagator.advance(
            ephemeris,
            run,
            end,
            10_000_000,
            None,
            thrust.map(Control::Thrust),
        );
        match outcome {
            AdvanceOutcome::Impact { .. } => panic!(
                "lander: hit the reference sphere in flight; the terrain band should have caught it"
            ),
            AdvanceOutcome::Budget => panic!("lander: flight step budget exhausted"),
            AdvanceOutcome::Reached => {}
        }
        self.time = run.time;
        if burnout {
            run.y[6] = self.spec.dry_mass_kg;
        }
        self.mass_kg = run.y[6];
    }

    fn contact_step(&mut self, ephemeris: &mut Ephemeris, control: &LanderControl) {
        let dt = self.options.contact.step_seconds;
        let exhaust = self.exhaust_velocity();
        let flow = (control.throttle * self.spec.thrust_newtons) / exhaust;
        // A step that would empty the tanks burns only what is left.
        let burned = (flow * dt).min(self.fuel_kg());
        let thrust = if burned > 0.0 {
            (burned * exhaust) / dt
        } else {
            0.0
        };
        // Mean mass over the step: the mass falls linearly while burning.
        let mean_mass = self.mass_kg - burned / 2.0;
        let (world, body) = self
            .contact
            .as_mut()
            .expect("a lander in contact has a world");
        if let Some(turn) = control.turn {
            world.apply_local_torque(*body, turn * 6000.0);
        }
        let before = self.previous_push;
        let mut push = DVec3::ZERO;
        let rotation = {
            let q = world.body(*body).rotation();
            DQuat::from_xyzw(
                f64::from(q.x),
                f64::from(q.y),
                f64::from(q.z),
                f64::from(q.w),
            )
        };
        let mut extra = |_: RigidBodyHandle, state: FrameState| {
            let direction = if control.direction.is_some() {
                rotate(rotation, DVec3::Y)
            } else {
                surface_direction(state, control)
            };
            push = if thrust > 0.0 {
                direction * (thrust / mean_mass)
            } else {
                DVec3::ZERO
            };
            DVec3::new(
                (before.x + push.x) / 2.0,
                (before.y + push.y) / 2.0,
                (before.z + push.z) / 2.0,
            )
        };
        world.step(ephemeris, Some(&mut extra));
        self.previous_push = push;
        self.mass_kg -= burned;
        if self.fuel_kg() < 1e-9 * self.spec.dry_mass_kg {
            self.mass_kg = self.spec.dry_mass_kg;
        }
        let (world, body) = self.contact.as_ref().expect("still in contact");
        self.time = world.time;
        let state = world.state(ephemeris, *body, push);
        if hypot([state.position.x, state.position.y, state.position.z])
            > self.band_radius(self.options.band_exit_meters)
        {
            self.enter_flight(ephemeris, state);
        }
    }

    /// `push_before`: thrust acceleration over the half step before now (zero from rest).
    fn enter_contact(
        &mut self,
        ephemeris: &mut Ephemeris,
        state: FrameState,
        push_before: DVec3,
        rotation: Option<DQuat>,
    ) {
        let initial_rotation = rotation.unwrap_or_else(|| self.orientation());
        self.leave();
        self.previous_push = push_before;
        let mut world = ContactWorld::new(
            self.frame.clone(),
            Some(self.terrain.clone()),
            self.options.contact,
            self.time,
            state.position,
            ephemeris,
        );
        let shape =
            self.spec
                .contact_shape
                .clone()
                .unwrap_or(BodyShape::Simple(SimpleShape::Box {
                    half_extents: self.spec.half_extents,
                }));
        let spec = ContactBodySpec {
            shape,
            mass_kg: self.mass_kg,
            friction: self.spec.friction,
            restitution: 0.0,
            lock_rotations: false,
        };
        let body = world.add_body(ephemeris, &spec, state, initial_rotation, push_before);
        self.contact = Some((world, body));
        self.switch_to(LanderMode::Contact);
    }

    fn enter_flight(&mut self, ephemeris: &Ephemeris, state: FrameState) {
        if self.contact.is_some() {
            self.flight_rotation = self.orientation();
        }
        self.leave();
        let inertial = self.frame.to_inertial(ephemeris, self.time, state);
        self.run = Some(PropagationRun::new(VesselState {
            time: self.time,
            position: inertial.position,
            velocity: inertial.velocity,
            mass_kg: self.mass_kg,
        }));
        self.switch_to(LanderMode::Flight);
    }

    fn leave(&mut self) {
        self.contact = None;
        self.run = None;
    }

    fn switch_to(&mut self, mode: LanderMode) {
        if mode != self.mode {
            self.mode_changes.push(ModeChange {
                time: self.time,
                from: self.mode,
                to: mode,
            });
        }
        self.mode = mode;
    }

    fn check_band(&mut self, ephemeris: &mut Ephemeris) {
        let p = self.body_fixed_state(ephemeris).position;
        if hypot([p.x, p.y, p.z]) <= self.band_radius(self.options.band_enter_meters) {
            let state = self.body_fixed_state(ephemeris);
            self.enter_contact(ephemeris, state, DVec3::ZERO, None);
        }
    }
}

/// The surface law in body-fixed axes, where the ground is at rest (the orbit crate's definition).
pub fn surface_direction(state: FrameState, control: &LanderControl) -> DVec3 {
    if let Some(d) = control.direction {
        return d;
    }
    let (p, v) = (state.position, state.velocity);
    let r = hypot([p.x, p.y, p.z]);
    let g = hypot([v.x, v.y, v.z]);
    assert!(
        control.prograde == 0.0 || g > 0.0,
        "surface attitude undefined: no velocity over the ground"
    );
    let k = if control.prograde == 0.0 {
        0.0
    } else {
        control.prograde / g
    };
    let d = DVec3::new(
        (control.up * p.x) / r + k * v.x,
        (control.up * p.y) / r + k * v.y,
        (control.up * p.z) / r + k * v.z,
    );
    let l = hypot([d.x, d.y, d.z]);
    assert!(
        l > 0.0,
        "surface attitude undefined: up and ground velocity cancel"
    );
    DVec3::new(d.x / l, d.y / l, d.z / l)
}

/// Rotate a vector by a unit quaternion: v + 2 q_v × (q_v × v + w v).
pub fn rotate(q: DQuat, v: DVec3) -> DVec3 {
    let cx = q.y * v.z - q.z * v.y + q.w * v.x;
    let cy = q.z * v.x - q.x * v.z + q.w * v.y;
    let cz = q.x * v.y - q.y * v.x + q.w * v.z;
    DVec3::new(
        v.x + 2.0 * (q.y * cz - q.z * cy),
        v.y + 2.0 * (q.z * cx - q.x * cz),
        v.z + 2.0 * (q.x * cy - q.y * cx),
    )
}

/// Rotation taking local +y to the outward vertical at a body-fixed point.
pub fn upright_at(p: DVec3) -> DQuat {
    let l = hypot([p.x, p.y, p.z]);
    let u = DVec3::new(p.x / l, p.y / l, p.z / l);
    // Shortest arc from (0, 1, 0) to u.
    let w = 1.0 + u.y;
    if w < 1e-12 {
        return DQuat::from_xyzw(1.0, 0.0, 0.0, 0.0);
    }
    let n = hypot([u.z, 0.0, -u.x, w]);
    DQuat::from_xyzw(u.z / n, 0.0 / n, -u.x / n, w / n)
}
