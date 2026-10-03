//! Two parts joined by one removable Rapier fixed joint, as
//! `lab/landing/src/vessel/PartJointRocket.ts`. Attached, the stack switches between contact and
//! orbital physics as one unit. After staging each part switches on its own clearance, so a spent
//! booster landing does not pull the upper stage back into Rapier. Contact parts closer than
//! `recenter_meters` share one Rapier world (they can collide); farther apart they get separate
//! worlds. All parts advance on one clock.

use std::sync::Arc;

use glam::{DQuat, DVec3};
use rapier3d::prelude::{FixedJointBuilder, ImpulseJointHandle, RigidBodyHandle};
use void_math::hypot;
use void_orbit::{
    AdvanceOutcome, AirSource, AttitudeLaw, Control, Ephemeris, PropagationRun, ThrustControl,
    VesselPropagator, VesselState, body_orientation,
};
use void_rotation::{Mat3, matrix, rotation_step};
use void_terrain::Terrain;

use crate::air::{AirField, PlanetAir, unit};
use crate::contact_world::ContactWorld;
use crate::lander::{LanderControl, LanderOptions, LanderSpec, STANDARD_GRAVITY, rotate};
use crate::planet_frame::{ContactFrame, FrameState, PlanetFrame};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RocketPart {
    Upper,
    Booster,
}

pub const PARTS: [RocketPart; 2] = [RocketPart::Upper, RocketPart::Booster];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhysicsMode {
    Flight,
    Contact,
    Destroyed,
}

const UPPER_OFFSET: f64 = 1.1;
const BOOSTER_OFFSET: f64 = -1.3;
const INITIAL_CLEARANCE_METERS: f64 = 1.0;
const FLIGHT_CHUNK_SECONDS: f64 = 1.0;
const ENTRY_SNAP_METERS: f64 = 0.1;
/// Steering torque per unit of `turn`, N m, about the upper stage's local axes.
pub const STEERING_TORQUE: f64 = 6000.0;

/// The controlled unit's attitude at the start of a physics step, as a per-step steering law sees
/// it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttitudeSample {
    /// Upper stage's orientation in the body-fixed planet frame.
    pub rotation: DQuat,
    /// Body-fixed planet frame, rad/s.
    pub angular_velocity: DVec3,
    /// Inertia of the controlled unit (stack or upper stage) about its centre of mass, kg m², in the
    /// upper stage's local axes.
    pub inertia_local: Mat3,
}

/// Steering recomputed every physics step (the SAS lab's stability assist): the `turn` command
/// (each axis in [−1, 1]) for the coming step of dt seconds.
pub type Steering<'a> = &'a mut SteerFn<'a>;

/// The steering closure itself; internal calls reborrow it per step.
pub type SteerFn<'s> = dyn FnMut(AttitudeSample, f64) -> DVec3 + 's;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RocketModeChange {
    pub time: f64,
    pub from: PhysicsMode,
    pub to: PhysicsMode,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crash {
    pub part: RocketPart,
    pub time: f64,
    pub delta_v: f64,
}

/// One part's physics state: a Rapier body in some contact world, or (after staging) its own
/// orbit-propagated state. While attached in free flight both parts have neither; the stack's
/// centre of mass follows `attached_run`.
#[derive(Clone, Debug)]
struct PartSlot {
    spec: LanderSpec,
    offset: f64,
    fuel_kg: f64,
    /// Index into the rocket's worlds, and the body there.
    world: Option<usize>,
    body: Option<RigidBodyHandle>,
    /// Thrust acceleration over the coming contact step, for the half-step velocity.
    push: DVec3,
    run: Option<PropagationRun>,
    /// Body-fixed attitude and angular velocity while not in a contact world (a Rapier body holds
    /// them otherwise).
    rotation: DQuat,
    angular_velocity: DVec3,
    /// Inertia about the part's centre of mass in its local axes, per kg of current mass.
    inertia_per_kg: Mat3,
    /// Last state before a crash; set once the part is destroyed.
    wreck: Option<FrameState>,
}

fn add_mat(a: &Mat3, b: &Mat3) -> Mat3 {
    std::array::from_fn(|i| a[i] + b[i])
}

fn scale_mat(m: &Mat3, k: f64) -> Mat3 {
    m.map(|v| v * k)
}

fn mat_mul(a: &Mat3, b: &Mat3) -> Mat3 {
    std::array::from_fn(|n| {
        let (i, j) = (n / 3, n % 3);
        a[i * 3] * b[j] + a[i * 3 + 1] * b[3 + j] + a[i * 3 + 2] * b[6 + j]
    })
}

fn transpose(m: &Mat3) -> Mat3 {
    [m[0], m[3], m[6], m[1], m[4], m[7], m[2], m[5], m[8]]
}

/// Inertia of a point mass at offset d about the origin, per kilogram (parallel-axis term).
fn parallel_axis_per_kg(d: DVec3) -> Mat3 {
    let s = d.x * d.x + d.y * d.y + d.z * d.z;
    [
        s - d.x * d.x,
        -d.x * d.y,
        -d.x * d.z,
        -d.y * d.x,
        s - d.y * d.y,
        -d.y * d.z,
        -d.z * d.x,
        -d.z * d.y,
        s - d.z * d.z,
    ]
}

/// Rotation taking local +y to a unit direction (shortest arc).
fn upright_at(direction: DVec3) -> DQuat {
    let w = 1.0 + direction.y;
    if w < 1e-12 {
        return DQuat::from_xyzw(1.0, 0.0, 0.0, 0.0);
    }
    let length = hypot([direction.z, 0.0, -direction.x, w]);
    DQuat::from_xyzw(
        direction.z / length,
        0.0 / length,
        -direction.x / length,
        w / length,
    )
}

fn q64(q: rapier3d::math::Rotation) -> DQuat {
    DQuat::from_xyzw(
        f64::from(q.x),
        f64::from(q.y),
        f64::from(q.z),
        f64::from(q.w),
    )
}

fn v64(v: rapier3d::math::Vector) -> DVec3 {
    DVec3::new(f64::from(v.x), f64::from(v.y), f64::from(v.z))
}

fn v32(v: DVec3) -> rapier3d::math::Vector {
    rapier3d::math::Vector::new(v.x as f32, v.y as f32, v.z as f32)
}

pub struct PartJointRocket {
    /// Physics-mode changes of the controlled vessel (the stack, then the upper stage).
    pub mode_changes: Vec<RocketModeChange>,
    /// Crashes in order: which part, when, and the contact speed change that broke it.
    pub crashes: Vec<Crash>,
    /// Destroy parts that exceed their crash tolerance. Off by default: impacts only collide.
    pub crash_detection: bool,
    pub frame: PlanetFrame,
    pub terrain: Arc<Terrain>,
    pub options: LanderOptions,
    pub upper_spec: LanderSpec,
    pub booster_spec: LanderSpec,
    pub full_spec: LanderSpec,
    upper: PartSlot,
    booster: PartSlot,
    worlds: Vec<Option<ContactWorld<PlanetFrame>>>,
    joint: Option<ImpulseJointHandle>,
    attached_run: Option<PropagationRun>,
    pending_seconds: f64,
    sim_time: f64,
    last_mode: PhysicsMode,
    propagator: VesselPropagator,
    air: Option<Arc<dyn AirField>>,
}

impl PartJointRocket {
    /// A rocket standing on the ground below a body-fixed direction; `body_index` is the planet in the
    /// ephemeris.
    #[allow(clippy::too_many_arguments)]
    pub fn landed(
        ephemeris: &mut Ephemeris,
        body_index: usize,
        terrain: Arc<Terrain>,
        full_spec: LanderSpec,
        upper_spec: LanderSpec,
        booster_spec: LanderSpec,
        options: LanderOptions,
        direction: DVec3,
    ) -> Self {
        assert!(
            upper_spec.contact_shape.is_some() && booster_spec.contact_shape.is_some(),
            "part-joint rocket: each part needs its own contact shape"
        );
        for part in [&upper_spec, &booster_spec] {
            let tolerance = part.crash_tolerance_meters_per_second;
            assert!(
                tolerance.is_some_and(|t| t > 0.0),
                "part-joint rocket: each part needs a crash tolerance, got {tolerance:?}"
            );
        }
        assert!(
            (full_spec.dry_mass_kg
                - upper_spec.dry_mass_kg
                - upper_spec.fuel_mass_kg
                - booster_spec.dry_mass_kg)
                .abs()
                <= 1e-9
                && full_spec.fuel_mass_kg == booster_spec.fuel_mass_kg,
            "part-joint rocket: aggregate mass must equal the two parts"
        );
        let length = hypot([direction.x, direction.y, direction.z]);
        assert!(length > 0.0, "part-joint rocket: invalid launch direction");
        let d = DVec3::new(
            direction.x / length,
            direction.y / length,
            direction.z / length,
        );
        let q = upright_at(d);
        let ground = terrain.height(d);
        let height = terrain.radius_meters + ground + 2.72 + INITIAL_CLEARANCE_METERS;
        let root = DVec3::new(d.x * height, d.y * height, d.z * height);
        let up = rotate(q, DVec3::Y);
        let slot = |spec: &LanderSpec, offset: f64| PartSlot {
            spec: spec.clone(),
            offset,
            fuel_kg: spec.fuel_mass_kg,
            world: None,
            body: None,
            push: DVec3::ZERO,
            run: None,
            rotation: q,
            angular_velocity: DVec3::ZERO,
            inertia_per_kg: [0.0; 9],
            wreck: None,
        };
        let frame = PlanetFrame::new(ephemeris, body_index);
        let mut rocket = Self {
            mode_changes: Vec::new(),
            crashes: Vec::new(),
            crash_detection: false,
            propagator: VesselPropagator::new(ephemeris, options.tolerances),
            air: None,
            terrain: terrain.clone(),
            options,
            upper: slot(&upper_spec, UPPER_OFFSET),
            booster: slot(&booster_spec, BOOSTER_OFFSET),
            upper_spec,
            booster_spec,
            full_spec,
            worlds: Vec::new(),
            joint: None,
            attached_run: None,
            pending_seconds: 0.0,
            sim_time: 0.0,
            last_mode: PhysicsMode::Contact,
            frame: frame.clone(),
        };
        let world = rocket.new_world(ephemeris, root);
        for which in PARTS {
            let offset = rocket.slot(which).offset;
            let position = DVec3::new(
                root.x + up.x * offset,
                root.y + up.y * offset,
                root.z + up.z * offset,
            );
            rocket.add_to_world(
                ephemeris,
                which,
                world,
                FrameState {
                    position,
                    velocity: DVec3::ZERO,
                },
                DVec3::ZERO,
            );
            // Rapier's mass properties from the part's colliders, valid as soon as they are attached.
            let body = rocket.body_ref(which);
            let props = &body.mass_properties().local_mprops;
            let principal = props.principal_inertia();
            let f = matrix(q64(props.principal_inertia_local_frame));
            let diagonal = [
                f64::from(principal.x),
                0.0,
                0.0,
                0.0,
                f64::from(principal.y),
                0.0,
                0.0,
                0.0,
                f64::from(principal.z),
            ];
            let mass = f64::from(body.mass());
            rocket.slot_mut(which).inertia_per_kg = scale_mat(
                &mat_mul(&mat_mul(&f, &diagonal), &transpose(&f)),
                1.0 / mass,
            );
        }
        rocket.join_parts();
        rocket
    }

    fn slot(&self, which: RocketPart) -> &PartSlot {
        match which {
            RocketPart::Upper => &self.upper,
            RocketPart::Booster => &self.booster,
        }
    }

    fn slot_mut(&mut self, which: RocketPart) -> &mut PartSlot {
        match which {
            RocketPart::Upper => &mut self.upper,
            RocketPart::Booster => &mut self.booster,
        }
    }

    fn world_ref(&self, index: usize) -> &ContactWorld<PlanetFrame> {
        self.worlds[index].as_ref().expect("a part's world exists")
    }

    fn world_mut(&mut self, index: usize) -> &mut ContactWorld<PlanetFrame> {
        self.worlds[index].as_mut().expect("a part's world exists")
    }

    fn new_world(&mut self, ephemeris: &mut Ephemeris, origin: DVec3) -> usize {
        let world = ContactWorld::new(
            self.frame.clone(),
            Some(self.terrain.clone()),
            self.options.contact,
            self.sim_time,
            origin,
            ephemeris,
        );
        match self.worlds.iter().position(Option::is_none) {
            Some(free) => {
                self.worlds[free] = Some(world);
                free
            }
            None => {
                self.worlds.push(Some(world));
                self.worlds.len() - 1
            }
        }
    }

    fn body_ref(&self, which: RocketPart) -> &rapier3d::prelude::RigidBody {
        let s = self.slot(which);
        let (world, body) = (
            s.world.expect("in a contact world"),
            s.body.expect("has a body"),
        );
        self.world_ref(world).body(body)
    }

    pub fn time(&self) -> f64 {
        self.sim_time
    }

    pub fn separated(&self) -> bool {
        self.joint.is_none()
    }

    pub fn spec(&self) -> &LanderSpec {
        if self.separated() {
            &self.upper_spec
        } else {
            &self.full_spec
        }
    }

    fn engine_part(&self) -> RocketPart {
        if self.separated() {
            RocketPart::Upper
        } else {
            RocketPart::Booster
        }
    }

    pub fn fuel_kg(&self) -> f64 {
        self.slot(self.engine_part()).fuel_kg
    }

    fn part_mass(&self, which: RocketPart) -> f64 {
        let s = self.slot(which);
        s.spec.dry_mass_kg + s.fuel_kg
    }

    pub fn mass_kg(&self) -> f64 {
        if self.separated() {
            self.part_mass(RocketPart::Upper)
        } else {
            self.part_mass(RocketPart::Upper) + self.part_mass(RocketPart::Booster)
        }
    }

    /// Physics mode of the controlled vessel.
    pub fn mode(&self) -> PhysicsMode {
        self.part_mode(RocketPart::Upper)
    }

    pub fn part_mode(&self, which: RocketPart) -> PhysicsMode {
        let s = self.slot(which);
        if s.wreck.is_some() {
            PhysicsMode::Destroyed
        } else if s.world.is_some() {
            PhysicsMode::Contact
        } else {
            PhysicsMode::Flight
        }
    }

    /// Fuel left in one part's tank.
    pub fn part_fuel_kg(&self, which: RocketPart) -> f64 {
        self.slot(which).fuel_kg
    }

    /// Δv left in one part's tank, in vacuum (Tsiolkovsky), m/s. The booster pushes the upper stage
    /// too while they are joined; the upper stage fires only after separation.
    pub fn part_delta_v(&self, which: RocketPart) -> f64 {
        let slot = self.slot(which);
        let start = if which == RocketPart::Booster && !self.separated() {
            self.mass_kg()
        } else {
            self.part_mass(which)
        };
        slot.spec.specific_impulse_seconds
            * STANDARD_GRAVITY
            * (start / (start - slot.fuel_kg)).ln()
    }

    /// Contact worlds in use (one while the parts are together, up to one per part otherwise), as
    /// indices, in part order without repeats.
    fn contact_world_indices(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for which in PARTS {
            if let Some(w) = self.slot(which).world
                && !out.contains(&w)
            {
                out.push(w);
            }
        }
        out
    }

    /// Fly through air instead of vacuum. The field is asked in the planet's body-fixed frame, in
    /// both free flight and contact; None, the default, is vacuum and the behaviour this crate had
    /// before there was any air at all.
    pub fn set_air_field(&mut self, air: Option<Arc<dyn AirField>>) {
        self.air = air;
    }

    pub fn contact_worlds(&self) -> Vec<&ContactWorld<PlanetFrame>> {
        self.contact_world_indices()
            .into_iter()
            .map(|i| self.world_ref(i))
            .collect()
    }

    /// The controlled vessel's contact world.
    pub fn world(&self) -> &ContactWorld<PlanetFrame> {
        self.world_ref(
            self.upper.world.expect(
                "part-joint rocket: the upper stage is in orbital flight, not a contact world",
            ),
        )
    }

    /// A part's body and its world, while in contact.
    pub fn part_body(
        &self,
        which: RocketPart,
    ) -> Option<(&ContactWorld<PlanetFrame>, RigidBodyHandle)> {
        let s = self.slot(which);
        Some((self.world_ref(s.world?), s.body?))
    }

    pub fn joint(&self) -> Option<ImpulseJointHandle> {
        self.joint
    }

    pub fn orientation(&self) -> DQuat {
        self.part_orientation(RocketPart::Upper)
    }

    pub fn part_state(&self, ephemeris: &Ephemeris, which: RocketPart) -> FrameState {
        let s = self.slot(which);
        if let Some(wreck) = s.wreck {
            return wreck;
        }
        if let (Some(w), Some(body)) = (s.world, s.body) {
            return self.world_ref(w).state(ephemeris, body, s.push);
        }
        if let Some(run) = &self.attached_run {
            let rs = run.state();
            let centre = self.frame.to_body_fixed(
                ephemeris,
                self.sim_time,
                FrameState {
                    position: rs.position,
                    velocity: rs.velocity,
                },
            );
            let (mu, mb) = (
                self.part_mass(RocketPart::Upper),
                self.part_mass(RocketPart::Booster),
            );
            let com_offset = (mu * UPPER_OFFSET + mb * BOOSTER_OFFSET) / (mu + mb);
            let delta = rotate(
                self.upper.rotation,
                DVec3::new(0.0, s.offset - com_offset, 0.0),
            );
            let w = self.upper.angular_velocity;
            let spin = DVec3::new(
                w.y * delta.z - w.z * delta.y,
                w.z * delta.x - w.x * delta.z,
                w.x * delta.y - w.y * delta.x,
            );
            return FrameState {
                position: centre.position + delta,
                velocity: centre.velocity + spin,
            };
        }
        let rs = s
            .run
            .as_ref()
            .unwrap_or_else(|| panic!("part-joint rocket: missing {which:?} flight state"))
            .state();
        self.frame.to_body_fixed(
            ephemeris,
            self.sim_time,
            FrameState {
                position: rs.position,
                velocity: rs.velocity,
            },
        )
    }

    /// Angular velocity in the body-fixed planet frame, rad/s.
    pub fn part_angular_velocity(&self, which: RocketPart) -> DVec3 {
        let s = self.slot(which);
        if s.body.is_some() {
            v64(self.body_ref(which).angvel())
        } else {
            s.angular_velocity
        }
    }

    pub fn part_orientation(&self, which: RocketPart) -> DQuat {
        let s = self.slot(which);
        if s.body.is_some() {
            q64(*self.body_ref(which).rotation())
        } else {
            s.rotation
        }
    }

    pub fn body_fixed_state(&self, ephemeris: &Ephemeris) -> FrameState {
        let upper = self.part_state(ephemeris, RocketPart::Upper);
        if self.separated() {
            return upper;
        }
        let booster = self.part_state(ephemeris, RocketPart::Booster);
        let (mu, mb) = (
            self.part_mass(RocketPart::Upper),
            self.part_mass(RocketPart::Booster),
        );
        let blend = |a: DVec3, b: DVec3| {
            DVec3::new(
                (a.x * mu + b.x * mb) / (mu + mb),
                (a.y * mu + b.y * mb) / (mu + mb),
                (a.z * mu + b.z * mb) / (mu + mb),
            )
        };
        FrameState {
            position: blend(upper.position, booster.position),
            velocity: blend(upper.velocity, booster.velocity),
        }
    }

    pub fn clearance(&self, ephemeris: &Ephemeris) -> f64 {
        self.clearance_at(self.body_fixed_state(ephemeris).position)
    }

    fn clearance_at(&self, p: DVec3) -> f64 {
        let r = hypot([p.x, p.y, p.z]);
        r - self.terrain.radius_meters - self.terrain.height(DVec3::new(p.x / r, p.y / r, p.z / r))
    }

    /// Height of one part's reference point above the terrain under it, m.
    pub fn part_clearance(&self, ephemeris: &Ephemeris, which: RocketPart) -> f64 {
        self.clearance_at(self.part_state(ephemeris, which).position)
    }

    /// Remove only the joint; in contact both rigid bodies and their colliders stay alive.
    pub fn separate(&mut self, ephemeris: &Ephemeris) {
        let Some(joint) = self.joint else { return };
        let axis = rotate(self.orientation(), DVec3::Y);
        let impulse = 500.0;
        if self.attached_run.is_none() {
            let w = self.upper.world.expect("attached in contact");
            let (upper, booster) = (
                self.upper.body.expect("body"),
                self.booster.body.expect("body"),
            );
            let world = &mut self.world_mut(w).world;
            world.impulse_joints.remove(joint, true);
            world.bodies[upper].apply_impulse(v32(axis * impulse), true);
            world.bodies[booster].apply_impulse(v32(-axis * impulse), true);
            self.joint = None;
            return;
        }
        let world_axis = self.to_inertial_direction(axis, self.sim_time);
        for which in PARTS {
            let state =
                self.frame
                    .to_inertial(ephemeris, self.sim_time, self.part_state(ephemeris, which));
            let mass = self.part_mass(which);
            let k = if which == RocketPart::Upper {
                impulse
            } else {
                -impulse
            } / mass;
            let velocity = DVec3::new(
                state.velocity.x + world_axis.x * k,
                state.velocity.y + world_axis.y * k,
                state.velocity.z + world_axis.z * k,
            );
            let time = self.sim_time;
            self.slot_mut(which).run = Some(PropagationRun::new(VesselState {
                time,
                position: state.position,
                velocity,
                mass_kg: mass,
            }));
        }
        self.booster.rotation = self.upper.rotation;
        self.booster.angular_velocity = self.upper.angular_velocity;
        self.attached_run = None;
        self.joint = None;
    }

    pub fn advance<'s>(
        &mut self,
        ephemeris: &mut Ephemeris,
        dt: f64,
        control: &LanderControl,
        mut steering: Option<&mut SteerFn<'s>>,
    ) {
        assert!(
            dt >= 0.0 && dt.is_finite(),
            "part-joint rocket advance({dt})"
        );
        assert!(
            (0.0..=1.0).contains(&control.throttle),
            "part-joint rocket: throttle {}",
            control.throttle
        );
        assert!(
            !(control.turn.is_some() && steering.is_some()),
            "part-joint rocket: give either turn or steering, not both"
        );
        if self.upper.wreck.is_some() {
            return;
        }
        assert!(
            control.orbital_attitude.is_none()
                || (self.upper.world.is_none() && control.rotation.is_some()),
            "part-joint rocket: orbital maneuver requires free flight and a commanded orientation"
        );
        if let Some(rotation) = control.rotation {
            let parts: &[RocketPart] = if self.separated() {
                &[RocketPart::Upper]
            } else {
                &PARTS
            };
            for &which in parts {
                let s = self.slot_mut(which);
                if s.body.is_none() {
                    s.rotation = rotation;
                    if control.orbital_attitude.is_some() {
                        s.angular_velocity = DVec3::ZERO;
                    }
                }
            }
        }
        let target = self.sim_time + self.pending_seconds + dt;
        let step = self.options.contact.step_seconds;
        loop {
            self.update_modes(ephemeris, control);
            assert!(
                control.orbital_attitude.is_none() || self.upper.world.is_none(),
                "part-joint rocket: orbital maneuver entered contact physics"
            );
            if self.upper.wreck.is_some() {
                self.pending_seconds = 0.0;
                return;
            }
            if !self.contact_world_indices().is_empty() {
                if self.sim_time + step > target + 1e-12 {
                    self.pending_seconds = (target - self.sim_time).max(0.0);
                    return;
                }
                self.step_contact(ephemeris, control, steering.as_deref_mut());
            } else {
                if self.sim_time + 1e-9 >= target {
                    self.pending_seconds = 0.0;
                    return;
                }
                self.flight_chunk(ephemeris, target, control, steering.as_deref_mut(), true);
            }
        }
    }

    /// Why the rocket cannot go on rails now, or None if it can. On rails (KSP's high time warp)
    /// nothing is simulated in Rapier and no engine burns: every live part is either coasting in
    /// orbital flight or asleep on the ground.
    pub fn rails_blocker(&self, throttle: f64) -> Option<&'static str> {
        if self.upper.wreck.is_some() {
            return Some("the vessel is destroyed");
        }
        if throttle > 0.0 && self.slot(self.engine_part()).fuel_kg > 0.0 {
            return Some("engine firing");
        }
        if self.contact_worlds().iter().any(|w| !w.asleep()) {
            return Some("moving near the ground");
        }
        None
    }

    /// Advance on rails: flight parts coast (no thrust, attitude held in the body-fixed frame, spin
    /// stopped), resting parts keep their place on the ground. Returns false, having stopped short,
    /// when a part came down into the contact band or woke up: the caller drops back to physics time.
    pub fn advance_on_rails(&mut self, ephemeris: &mut Ephemeris, dt: f64) -> bool {
        assert!(
            dt >= 0.0 && dt.is_finite(),
            "part-joint rocket advance on rails({dt})"
        );
        if let Some(blocker) = self.rails_blocker(0.0) {
            panic!("part-joint rocket advance on rails: {blocker}");
        }
        for which in PARTS {
            if self.slot(which).body.is_none() {
                self.slot_mut(which).angular_velocity = DVec3::ZERO;
            }
        }
        let target = self.sim_time + self.pending_seconds + dt;
        self.pending_seconds = 0.0;
        let coast = LanderControl::default();
        loop {
            self.update_modes(ephemeris, &coast);
            if self.rails_blocker(0.0).is_some() {
                return false;
            }
            if self.sim_time + 1e-9 >= target {
                return true;
            }
            let flying =
                self.attached_run.is_some() || PARTS.iter().any(|w| self.slot(*w).run.is_some());
            if flying {
                self.flight_chunk(ephemeris, target, &coast, None, false);
            } else {
                self.sim_time = target;
            }
            let time = self.sim_time;
            for w in self.contact_world_indices() {
                self.world_mut(w).idle_to(ephemeris, time);
            }
        }
    }

    fn to_inertial_direction(&self, local: DVec3, time: f64) -> DVec3 {
        let axes = body_orientation(&self.frame.body.rotation, time);
        let v = DVec3::new(
            local.x * axes[0].x + local.y * axes[1].x + local.z * axes[2].x,
            local.x * axes[0].y + local.y * axes[1].y + local.z * axes[2].y,
            local.x * axes[0].z + local.y * axes[1].z + local.z * axes[2].z,
        );
        let length = hypot([v.x, v.y, v.z]);
        DVec3::new(v.x / length, v.y / length, v.z / length)
    }

    /// One fixed Rapier step for every contact world; flight states follow to the same time.
    fn step_contact<'s>(
        &mut self,
        ephemeris: &mut Ephemeris,
        control: &LanderControl,
        mut steering: Option<&mut SteerFn<'s>>,
    ) {
        let step = self.options.contact.step_seconds;
        let t1 = self.sim_time + step;
        let engine_part = self.engine_part();
        let engine = self.slot(engine_part).clone();
        let (mut burned, mut push) = (0.0, DVec3::ZERO);
        if let Some(body) = engine.body {
            // Ambient pressure at the engine's body-fixed position (Rapier's translation is
            // relative to the world's floating origin).
            let (thrust_newtons, ve) = self.engine_output(
                engine_part,
                self.world_ref(engine.world.expect("the engine part is in a world"))
                    .position(body),
            );
            burned = engine
                .fuel_kg
                .min(control.throttle * thrust_newtons * step / ve);
            let thrust = burned * ve / step;
            let mean_mass = engine.spec.dry_mass_kg + engine.fuel_kg - burned / 2.0;
            let direction = rotate(q64(*self.body_ref(engine_part).rotation()), DVec3::Y);
            push = DVec3::new(
                direction.x * thrust / mean_mass,
                direction.y * thrust / mean_mass,
                direction.z * thrust / mean_mass,
            );
        }
        if let (Some(world), Some(body)) = (self.upper.world, self.upper.body) {
            let upper_body = self.world_ref(world).body(body);
            let (rotation, angular_velocity) =
                (q64(*upper_body.rotation()), v64(upper_body.angvel()));
            let parts: Vec<RocketPart> = if self.separated() {
                vec![RocketPart::Upper]
            } else {
                PARTS.to_vec()
            };
            if let Some(turn) = self.turn_for(
                &parts,
                control,
                steering.as_deref_mut(),
                rotation,
                angular_velocity,
                step,
            ) {
                self.world_mut(world)
                    .apply_local_torque(body, turn * STEERING_TORQUE);
            }
        }
        let before = engine.push;
        let average = DVec3::new(
            (before.x + push.x) / 2.0,
            (before.y + push.y) / 2.0,
            (before.z + push.z) / 2.0,
        );
        // Each Rapier body is its own rigid unit here, so the air is asked about one part at a
        // time, at the attitude Rapier is holding it in.
        let air = self.air.clone();
        let in_air: Vec<(usize, RigidBodyHandle, RocketPart, DQuat, f64)> = PARTS
            .into_iter()
            .filter_map(|which| {
                let s = self.slot(which);
                let (w, b) = (s.world?, s.body?);
                Some((
                    w,
                    b,
                    which,
                    unit(q64(*self.world_ref(w).body(b).rotation())),
                    self.part_mass(which),
                ))
            })
            .collect();
        for w in self.contact_world_indices() {
            let engine_here = engine.world == Some(w);
            let (air, in_air) = (air.as_ref(), &in_air);
            let mut extra = |body: RigidBodyHandle, state: FrameState| {
                let mut a = if engine_here && Some(body) == engine.body {
                    average
                } else {
                    DVec3::ZERO
                };
                if let Some(field) = air
                    && let Some((_, _, which, rotation, mass)) =
                        in_air.iter().find(|(iw, ib, ..)| *iw == w && *ib == body)
                {
                    a += field.force(&[*which], state, *rotation, *mass) / *mass;
                }
                a
            };
            self.world_mut(w).step(ephemeris, Some(&mut extra));
        }
        if let (Some(w), Some(body)) = (engine.world, engine.body) {
            let slot = self.slot_mut(engine_part);
            slot.fuel_kg -= burned;
            slot.push = push;
            if burned > 0.0 {
                let mass = self.part_mass(engine_part);
                self.world_mut(w).set_body_mass(body, mass);
            }
        }
        for which in PARTS {
            let Some(mut run) = self.slot_mut(which).run.take() else {
                continue;
            };
            self.propagate(ephemeris, &[which], &mut run, t1, control.throttle, control);
            self.slot_mut(which).run = Some(run);
            self.step_flight_attitude(&[which], control, steering.as_deref_mut(), step);
        }
        self.sim_time = t1;
        self.check_crashes(ephemeris);
    }

    /// A part whose contact speed change exceeds its crash tolerance is destroyed and leaves the
    /// simulation.
    fn check_crashes(&mut self, ephemeris: &Ephemeris) {
        if !self.crash_detection {
            return;
        }
        let broken: Vec<RocketPart> = PARTS
            .into_iter()
            .filter(|&which| {
                let s = self.slot(which);
                match (s.world, s.body) {
                    (Some(w), Some(b)) => {
                        self.world_ref(w).last_contact_delta_v(b)
                            > s.spec.crash_tolerance_meters_per_second.expect("checked")
                    }
                    _ => false,
                }
            })
            .collect();
        if broken.is_empty() {
            return;
        }
        // Removing a body removes its joint too, so a crash while attached also stages the stack.
        self.joint = None;
        for which in broken {
            let s = self.slot(which);
            let delta_v = self
                .world_ref(s.world.expect("in contact"))
                .last_contact_delta_v(s.body.expect("has a body"));
            let wreck = self.part_state(ephemeris, which);
            self.detach(which);
            self.slot_mut(which).wreck = Some(wreck);
            self.crashes.push(Crash {
                part: which,
                time: self.sim_time,
                delta_v,
            });
        }
    }

    /// Every part is in orbital flight (or, on rails, the rest asleep): advance them together,
    /// stopping before any can reach the contact band. `attitude` false holds attitudes (on rails).
    fn flight_chunk<'s>(
        &mut self,
        ephemeris: &mut Ephemeris,
        target: f64,
        control: &LanderControl,
        mut steering: Option<&mut SteerFn<'s>>,
        attitude: bool,
    ) {
        let units: Vec<Vec<RocketPart>> = if self.attached_run.is_some() {
            vec![PARTS.to_vec()]
        } else {
            PARTS
                .into_iter()
                .filter(|w| self.slot(*w).run.is_some())
                .map(|w| vec![w])
                .collect()
        };
        // Chunks bound the attitude steps; on rails attitudes are held and only the band limits a chunk.
        let mut end = if attitude {
            target.min(self.sim_time + FLIGHT_CHUNK_SECONDS)
        } else {
            target
        };
        let engine_part = self.engine_part();
        for parts in &units {
            let states: Vec<FrameState> = parts
                .iter()
                .map(|w| self.part_state(ephemeris, *w))
                .collect();
            let gap = states
                .iter()
                .map(|s| self.clearance_at(s.position))
                .fold(f64::INFINITY, f64::min)
                - self.options.band_enter_meters;
            let r = states
                .iter()
                .map(|s| hypot([s.position.x, s.position.y, s.position.z]))
                .fold(f64::INFINITY, f64::min);
            let speed = states
                .iter()
                .map(|s| hypot([s.velocity.x, s.velocity.y, s.velocity.z]))
                .fold(f64::NEG_INFINITY, f64::max);
            let thrusting = parts.contains(&engine_part)
                && control.throttle > 0.0
                && self.slot(engine_part).fuel_kg > 0.0;
            let unit_mass = if parts.len() == 2 {
                self.attached_run
                    .as_ref()
                    .expect("attached")
                    .state()
                    .mass_kg
            } else {
                self.slot(parts[0])
                    .run
                    .as_ref()
                    .expect("flying")
                    .state()
                    .mass_kg
            };
            let thrust = if thrusting {
                control.throttle * self.slot(engine_part).spec.thrust_newtons
            } else {
                0.0
            };
            let acceleration =
                1.2 * self.frame.body.gm / (r * r).max(1.0) + thrust / unit_mass.max(1.0);
            let safe = (-speed + (speed * speed + 2.0 * acceleration * gap.max(0.0)).sqrt())
                / acceleration;
            end = end.min(self.sim_time + safe.max(1e-3));
            // Thrust follows the attitude, which the integrator holds fixed over a call: while it turns,
            // advance one step at a time.
            if thrusting && self.attitude_changing(parts, control, steering.is_some()) {
                end = end.min(self.sim_time + self.options.contact.step_seconds);
            }
        }
        for parts in &units {
            if parts.len() == 2 {
                let mut run = self.attached_run.take().expect("attached");
                self.propagate(ephemeris, parts, &mut run, end, control.throttle, control);
                self.attached_run = Some(run);
            } else {
                let mut run = self.slot_mut(parts[0]).run.take().expect("flying");
                self.propagate(ephemeris, parts, &mut run, end, control.throttle, control);
                self.slot_mut(parts[0]).run = Some(run);
            }
        }
        let step = self.options.contact.step_seconds;
        if attitude {
            for parts in &units {
                let mut t = self.sim_time;
                while t < end - 1e-12 {
                    self.step_flight_attitude(
                        parts,
                        control,
                        steering.as_deref_mut(),
                        step.min(end - t),
                    );
                    t += step;
                }
            }
        }
        self.sim_time = end;
    }

    /// Propagate an orbit state to `end`, splitting at burnout and charging the engine part's fuel.
    fn propagate(
        &mut self,
        ephemeris: &mut Ephemeris,
        parts: &[RocketPart],
        run: &mut PropagationRun,
        end: f64,
        throttle: f64,
        command: &LanderControl,
    ) {
        // The engine burns for the unit it is flying with, if it is in this one.
        let engine_part = self.engine_part();
        let engine = parts.contains(&engine_part).then_some(engine_part);
        // The air field sees this unit at the attitude the attitude integrator is holding; the
        // propagator keeps the source only for as long as this leg.
        let air = self.air.clone().map(|field| {
            Arc::new(PlanetAir::new(
                field,
                &self.frame,
                parts,
                self.part_orientation(parts[0]),
            )) as Arc<dyn AirSource>
        });
        self.propagator.set_air_source(air);
        while run.time + 1e-12 < end {
            let thrust = engine.and_then(|e| {
                // Where the leg starts, in the frame the air is measured in.
                let state = run.state();
                let local = self.frame.to_body_fixed(
                    ephemeris,
                    run.time,
                    FrameState {
                        position: state.position,
                        velocity: state.velocity,
                    },
                );
                self.flight_control(
                    e,
                    throttle,
                    run.time,
                    local.position,
                    command.orbital_attitude,
                )
            });
            let start = run.time;
            if let (Some(thrust), Some(e)) = (thrust, engine) {
                let fuel = self.slot(e).fuel_kg;
                let burnout = start + fuel * thrust.exhaust_velocity / thrust.thrust_newtons;
                let stop = if burnout < end { burnout } else { end };
                if stop > start + 1e-12 {
                    let outcome = self.propagator.advance(
                        ephemeris,
                        run,
                        stop,
                        100_000,
                        None,
                        Some(Control::Thrust(thrust)),
                    );
                    assert!(
                        outcome == AdvanceOutcome::Reached,
                        "part-joint rocket: free-flight propagation {outcome:?}"
                    );
                }
                let slot = self.slot_mut(e);
                slot.fuel_kg = if stop < end {
                    0.0
                } else {
                    (slot.fuel_kg
                        - thrust.thrust_newtons * (stop - start) / thrust.exhaust_velocity)
                        .max(0.0)
                };
                continue;
            }
            let outcome = self
                .propagator
                .advance(ephemeris, run, end, 100_000, None, None);
            assert!(
                outcome == AdvanceOutcome::Reached,
                "part-joint rocket: free-flight propagation {outcome:?}"
            );
        }
    }

    fn flight_control(
        &self,
        which: RocketPart,
        throttle: f64,
        time: f64,
        body_fixed_position: DVec3,
        orbital_attitude: Option<AttitudeLaw>,
    ) -> Option<ThrustControl> {
        let slot = self.slot(which);
        if throttle <= 0.0 || slot.fuel_kg <= 0.0 {
            return None;
        }
        let direction = self.to_inertial_direction(rotate(slot.rotation, DVec3::Y), time);
        let minimum_mass_kg = if self.separated() {
            self.upper_spec.dry_mass_kg
        } else {
            self.upper_spec.dry_mass_kg + self.booster_spec.dry_mass_kg + self.upper.fuel_kg
        };
        // Pressure is read where the leg starts and held over it, as the whole control is.
        let (thrust_newtons, exhaust_velocity) = self.engine_output(which, body_fixed_position);
        Some(ThrustControl {
            thrust_newtons: throttle * thrust_newtons,
            exhaust_velocity,
            minimum_mass_kg,
            attitude: orbital_attitude.unwrap_or(AttitudeLaw::Inertial { direction }),
        })
    }

    /// What the engine actually gives where it is: vacuum thrust less the ambient pressure on the
    /// nozzle's exit, and the exhaust velocity that falls with it at the same mass flow. An
    /// over-expanded nozzle low down would come out negative; the flow separates there instead, so
    /// the model stops at no thrust rather than pushing the rocket backwards.
    fn engine_output(&self, which: RocketPart, position: DVec3) -> (f64, f64) {
        let spec = &self.slot(which).spec;
        let vacuum = spec.thrust_newtons;
        let ve_vacuum = spec.specific_impulse_seconds * STANDARD_GRAVITY;
        let Some(air) = &self.air else {
            return (vacuum, ve_vacuum);
        };
        let thrust = (vacuum - spec.nozzle_exit_area_m2 * air.pressure_pa(position)).max(0.0);
        // The mass flow is the engine's own, so the exhaust velocity carries the whole loss.
        (thrust, ve_vacuum * thrust / vacuum)
    }

    /// Apply band crossings (with hysteresis), then keep contact worlds local.
    fn update_modes(&mut self, ephemeris: &mut Ephemeris, control: &LanderControl) {
        let enter = self.options.band_enter_meters + ENTRY_SNAP_METERS;
        let exit = self.options.band_exit_meters;
        if !self.separated() {
            let lowest = self
                .part_clearance(ephemeris, RocketPart::Upper)
                .min(self.part_clearance(ephemeris, RocketPart::Booster));
            if self.attached_run.is_some() && lowest <= enter {
                self.stack_to_contact(ephemeris, control);
            } else if self.attached_run.is_none() && lowest > exit {
                self.stack_to_flight(ephemeris);
            }
        } else {
            for which in PARTS {
                if self.slot(which).wreck.is_some() {
                    continue;
                }
                let clearance = self.part_clearance(ephemeris, which);
                if self.slot(which).world.is_some() && clearance > exit {
                    self.part_to_flight(ephemeris, which);
                } else if self.slot(which).world.is_none() && clearance <= enter {
                    self.part_to_contact(ephemeris, which, control);
                }
            }
            self.split_distant_world(ephemeris);
        }
        let mode = self.mode();
        if mode != self.last_mode {
            self.mode_changes.push(RocketModeChange {
                time: self.sim_time,
                from: self.last_mode,
                to: mode,
            });
            self.last_mode = mode;
        }
    }

    /// Inertia of a flight unit (one part, or the attached stack about its centre of mass) in the
    /// upper stage's local axes, from each part's collider inertia and the parallel-axis term.
    fn unit_inertia(&self, parts: &[RocketPart]) -> Mat3 {
        if parts.len() == 1 {
            return scale_mat(
                &self.slot(parts[0]).inertia_per_kg,
                self.part_mass(parts[0]),
            );
        }
        let total: f64 = parts.iter().fold(0.0, |sum, w| sum + self.part_mass(*w));
        let com_offset = parts.iter().fold(0.0, |sum, w| {
            sum + self.part_mass(*w) * self.slot(*w).offset
        }) / total;
        let mut inertia = [0.0; 9];
        for &which in parts {
            let d = DVec3::new(0.0, self.slot(which).offset - com_offset, 0.0);
            inertia = add_mat(
                &inertia,
                &scale_mat(
                    &add_mat(&self.slot(which).inertia_per_kg, &parallel_axis_per_kg(d)),
                    self.part_mass(which),
                ),
            );
        }
        inertia
    }

    /// Inertia of the controlled unit (the attached stack, or the upper stage after staging) in the
    /// upper stage's local axes, kg m².
    pub fn controlled_inertia(&self) -> Mat3 {
        if self.separated() {
            self.unit_inertia(&[RocketPart::Upper])
        } else {
            self.unit_inertia(&PARTS)
        }
    }

    /// The turn command for one step of the unit `parts`, or None when it is not steered. A per-step
    /// steering law is called here exactly once per physics step, with the attitude at its start.
    fn turn_for(
        &self,
        parts: &[RocketPart],
        control: &LanderControl,
        steering: Option<&mut SteerFn<'_>>,
        rotation: DQuat,
        angular_velocity: DVec3,
        dt: f64,
    ) -> Option<DVec3> {
        if !parts.contains(&RocketPart::Upper) {
            return None;
        }
        let Some(steering) = steering else {
            return control.turn;
        };
        let turn = steering(
            AttitudeSample {
                rotation,
                angular_velocity,
                inertia_local: self.unit_inertia(parts),
            },
            dt,
        );
        assert!(
            [turn.x, turn.y, turn.z]
                .iter()
                .all(|v| (-1.0..=1.0).contains(v)),
            "part-joint rocket: steering returned {turn}"
        );
        Some(turn)
    }

    fn attitude_changing(
        &self,
        parts: &[RocketPart],
        control: &LanderControl,
        steered: bool,
    ) -> bool {
        let w = self.slot(parts[0]).angular_velocity;
        // A steering law may command torque on any step, so its unit is always stepped with its attitude.
        if steered && parts.contains(&RocketPart::Upper) {
            return true;
        }
        let turn = if parts.contains(&RocketPart::Upper) {
            control.turn
        } else {
            None
        };
        hypot([w.x, w.y, w.z]) > 1e-9 || turn.is_some_and(|t| t != DVec3::ZERO)
    }

    /// One attitude step for a unit in orbital flight; attached parts share the upper stage's attitude.
    fn step_flight_attitude(
        &mut self,
        parts: &[RocketPart],
        control: &LanderControl,
        steering: Option<&mut SteerFn<'_>>,
        dt: f64,
    ) {
        let lead = self.slot(parts[0]).clone();
        let turn = self
            .turn_for(
                parts,
                control,
                steering,
                lead.rotation,
                lead.angular_velocity,
                dt,
            )
            .unwrap_or(DVec3::ZERO);
        let torque = DVec3::new(
            turn.x * STEERING_TORQUE,
            turn.y * STEERING_TORQUE,
            turn.z * STEERING_TORQUE,
        );
        let (rotation, angular_velocity) = rotation_step(
            lead.rotation,
            lead.angular_velocity,
            &self.unit_inertia(parts),
            torque,
            self.frame.spin(),
            dt,
        );
        for &which in parts {
            let s = self.slot_mut(which);
            s.rotation = rotation;
            s.angular_velocity = angular_velocity;
        }
    }

    fn engine_push(&self, which: RocketPart, control: &LanderControl) -> DVec3 {
        let slot = self.slot(which);
        if which != self.engine_part() || control.throttle <= 0.0 || slot.fuel_kg <= 0.0 {
            return DVec3::ZERO;
        }
        let d = rotate(slot.rotation, DVec3::Y);
        let a = control.throttle * slot.spec.thrust_newtons / self.part_mass(which);
        DVec3::new(d.x * a, d.y * a, d.z * a)
    }

    fn add_to_world(
        &mut self,
        ephemeris: &Ephemeris,
        which: RocketPart,
        world: usize,
        state: FrameState,
        push: DVec3,
    ) {
        let s = self.slot(which).clone();
        let spec = crate::contact_world::ContactBodySpec {
            shape: s.spec.contact_shape.clone().expect("checked"),
            mass_kg: self.part_mass(which),
            friction: s.spec.friction,
            restitution: 0.0,
            lock_rotations: false,
        };
        let w = self.world_mut(world);
        let body = w.add_body(ephemeris, &spec, state, s.rotation, push);
        w.world.bodies[body].set_angvel(v32(s.angular_velocity), true);
        let slot = self.slot_mut(which);
        slot.push = push;
        slot.body = Some(body);
        slot.world = Some(world);
        slot.run = None;
    }

    fn join_parts(&mut self) {
        let seam = (UPPER_OFFSET + BOOSTER_OFFSET) / 2.0;
        let joint = FixedJointBuilder::new()
            .local_anchor1(v32(DVec3::new(0.0, seam - UPPER_OFFSET, 0.0)))
            .local_anchor2(v32(DVec3::new(0.0, seam - BOOSTER_OFFSET, 0.0)));
        let (upper, booster) = (
            self.upper.body.expect("in contact"),
            self.booster.body.expect("in contact"),
        );
        let w = self.upper.world.expect("in contact");
        // Contacts between the parts stay on: the fixed joint is not rigid, and without the seam
        // contact carrying the booster's thrust into the upper stage the stack bends during a burn.
        self.joint = Some(
            self.world_mut(w)
                .world
                .impulse_joints
                .insert(upper, booster, joint, true),
        );
    }

    fn stack_to_flight(&mut self, ephemeris: &Ephemeris) {
        let combined = self.body_fixed_state(ephemeris);
        for which in PARTS {
            self.detach(which);
        }
        // The fixed joint keeps both parts turning together; the stack continues as one rigid body.
        self.booster.rotation = self.upper.rotation;
        self.booster.angular_velocity = self.upper.angular_velocity;
        let inertial = self.frame.to_inertial(ephemeris, self.sim_time, combined);
        let mass = self.part_mass(RocketPart::Upper) + self.part_mass(RocketPart::Booster);
        self.attached_run = Some(PropagationRun::new(VesselState {
            time: self.sim_time,
            position: inertial.position,
            velocity: inertial.velocity,
            mass_kg: mass,
        }));
    }

    fn stack_to_contact(&mut self, ephemeris: &mut Ephemeris, control: &LanderControl) {
        let (upper, booster) = (
            self.part_state(ephemeris, RocketPart::Upper),
            self.part_state(ephemeris, RocketPart::Booster),
        );
        let world = self.new_world(ephemeris, upper.position);
        for (which, state) in [(RocketPart::Upper, upper), (RocketPart::Booster, booster)] {
            let push = self.engine_push(which, control);
            self.add_to_world(ephemeris, which, world, state, push);
        }
        self.attached_run = None;
        self.join_parts();
    }

    fn part_to_flight(&mut self, ephemeris: &Ephemeris, which: RocketPart) {
        let state = self.part_state(ephemeris, which);
        self.detach(which);
        let inertial = self.frame.to_inertial(ephemeris, self.sim_time, state);
        let (time, mass) = (self.sim_time, self.part_mass(which));
        self.slot_mut(which).run = Some(PropagationRun::new(VesselState {
            time,
            position: inertial.position,
            velocity: inertial.velocity,
            mass_kg: mass,
        }));
    }

    /// Enter the band: share the other part's world when it is close, otherwise start a new one.
    fn part_to_contact(
        &mut self,
        ephemeris: &mut Ephemeris,
        which: RocketPart,
        control: &LanderControl,
    ) {
        let state = self.part_state(ephemeris, which);
        let other = self
            .slot(if which == RocketPart::Upper {
                RocketPart::Booster
            } else {
                RocketPart::Upper
            })
            .clone();
        let near = match (other.world, other.body) {
            (Some(w), Some(b)) => {
                let p = self.world_ref(w).state(ephemeris, b, DVec3::ZERO).position;
                (p - state.position).length() < self.options.contact.recenter_meters / 2.0
            }
            _ => false,
        };
        let world = if near {
            other.world.expect("near")
        } else {
            self.new_world(ephemeris, state.position)
        };
        let push = self.engine_push(which, control);
        self.add_to_world(ephemeris, which, world, state, push);
    }

    /// Two separated parts sharing a world but drifting apart get separate floating origins.
    fn split_distant_world(&mut self, ephemeris: &mut Ephemeris) {
        if self.upper.world.is_none() || self.upper.world != self.booster.world {
            return;
        }
        let gap = (self.part_state(ephemeris, RocketPart::Upper).position
            - self.part_state(ephemeris, RocketPart::Booster).position)
            .length();
        if gap <= self.options.contact.recenter_meters {
            return;
        }
        let state = self.part_state(ephemeris, RocketPart::Booster);
        let push = self.booster.push;
        self.detach(RocketPart::Booster);
        let world = self.new_world(ephemeris, state.position);
        self.add_to_world(ephemeris, RocketPart::Booster, world, state, push);
    }

    /// Take a part out of its contact world (freeing the world when it empties), keeping its attitude.
    fn detach(&mut self, which: RocketPart) {
        let s = self.slot(which);
        let (Some(world), Some(body)) = (s.world, s.body) else {
            return;
        };
        let rotation = q64(*self.world_ref(world).body(body).rotation());
        let angular_velocity = v64(self.world_ref(world).body(body).angvel());
        let shared = PARTS
            .iter()
            .any(|&other| other != which && self.slot(other).world == Some(world));
        if shared {
            self.world_mut(world).remove_body(body);
        } else {
            self.worlds[world] = None;
        }
        let s = self.slot_mut(which);
        s.rotation = rotation;
        s.angular_velocity = angular_velocity;
        s.world = None;
        s.body = None;
        s.push = DVec3::ZERO;
    }
}
