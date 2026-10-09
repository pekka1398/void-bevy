//! Rapier rigid bodies in a contact frame, as `lab/landing/src/physics/ContactWorld.ts`: the
//! planet's rotating frame on collision tiles streamed around them, or (no terrain) any other
//! frame.
//! - Coordinates: Rapier works in f32 relative to an f64 floating origin (body-fixed). The origin
//!   follows the bodies, so Rapier numbers stay within about `recenter_meters`.
//! - Forces: Rapier's own gravity is off. Each step first kicks every body's velocity by the frame's
//!   acceleration × dt; then Rapier resolves contacts and drifts positions.
//! - Accuracy: the velocity kept in Rapier is the half-step velocity v(n − ½): bodies enter with
//!   v − a dt/2 and are read back as u + a dt/2, which makes Rapier's kick-drift loop the
//!   second-order leapfrog in free flight.
//! - Precision: each body's position is also kept in f64. A step in which the solver left a body's
//!   velocity untouched moves it by velocity × dt in f64; only a step with contacts takes Rapier's
//!   f32 displacement. Repeated f32 rounding of the same displacement is a systematic drift of up
//!   to half an ulp per step; this removes it.
//! - Rotation: unconstrained bodies use the rotation crate's f64 momentum step. Bodies with contacts
//!   or joints keep Rapier's angular solution. Frame torque and prescribed torque are applied before
//!   solving, so constraints see them.

use void_frames::State;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::channel;

use glam::{DMat3, DQuat, DVec3};
use rapier3d::math::{Rotation, Vector};
use rapier3d::prelude::*;
use void_lod::{OrderedMap, TileMeshOptions, build_tile_indices, build_tile_mesh, tiles_around};
use void_orbit::EphemerisSource;
use void_rotation::{Mat3, fictitious_torque, rotation_step};
use void_terrain::Terrain;

use crate::planet_frame::ContactFrame;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContactWorldOptions {
    /// Fixed physics step, s.
    pub step_seconds: f64,
    pub tile_level: u32,
    /// Vertices per tile side (lod's tile resolution).
    pub tile_resolution: usize,
    /// Tiles within this distance of a body's ground point are loaded.
    pub tile_reach_meters: f64,
    /// Tiles farther than this from every body are unloaded (> reach, for hysteresis).
    pub tile_keep_meters: f64,
    /// The floating origin moves to a body that gets this far from it.
    pub recenter_meters: f64,
    /// Let Rapier put bodies at rest to sleep (a sleeping body gets no kick). True on the ground,
    /// where rest in the body-fixed frame is real; false in free fall, where tides keep every body
    /// moving relative to the frame.
    pub sleeping: bool,
}

/// A compound piece's own mass: its centre of mass at the piece origin and a principal inertia
/// per kilogram in the piece's axes. When every piece has one, they replace the uniform density
/// and must sum to the body mass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PieceMass {
    pub kg: f64,
    pub principal_inertia_per_kg: DVec3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SimpleShape {
    Box { half_extents: DVec3 },
    Ball { radius: f64 },
    Cylinder { radius: f64, half_height: f64 },
    Cone { radius: f64, half_height: f64 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub shape: SimpleShape,
    pub position: DVec3,
    pub rotation: Option<DQuat>,
    pub mass: Option<PieceMass>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BodyShape {
    Simple(SimpleShape),
    Compound(Vec<Piece>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContactBodySpec {
    pub shape: BodyShape,
    pub mass_kg: f64,
    pub friction: f64,
    pub restitution: f64,
    /// Keep the body's orientation fixed in the frame.
    pub lock_rotations: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct TileCollider {
    pub collider: ColliderHandle,
    /// Body-fixed tile origin.
    pub origin: DVec3,
}

/// One live Rapier collider, triangulated in its rigid body's local axes for observation.
#[derive(Clone, Debug)]
pub struct BodyColliderMesh {
    pub id: String,
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

/// What `ContactWorld` keeps per body beside Rapier.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct BodyRecord {
    /// Body origin in the frame, f64; Rapier's translation is this minus the floating origin.
    position: DVec3,
    /// Each collider's share of the body mass, and its inertia per kg when pieces carry their own.
    weights: Vec<f64>,
    inertia_per_kg: Option<Vec<DVec3>>,
    rotation_locked: bool,
    /// Attitude and angular velocity (frame axes, relative to the frame) in f64; Rapier holds them
    /// rounded. `published` is what Rapier actually stored after the last write.
    turn_rotation: DQuat,
    turn_angular_velocity: DVec3,
    published: Rotation,
    /// Prescribed local torque for the next step only.
    torque: Option<DVec3>,
    contact_delta_v: f64,
    /// Velocity change the solver made in the last step (contacts, joints); zero in a free step.
    solver_delta: DVec3,
    /// Proposed interval contact-model impulse, credited like native solver support in state().
    pending_contact_delta: DVec3,
}

fn v32(v: DVec3) -> Vector {
    Vector::new(v.x as f32, v.y as f32, v.z as f32)
}

fn v64(v: Vector) -> DVec3 {
    DVec3::new(f64::from(v.x), f64::from(v.y), f64::from(v.z))
}

fn q64(q: Rotation) -> DQuat {
    DQuat::from_xyzw(
        f64::from(q.x),
        f64::from(q.y),
        f64::from(q.z),
        f64::from(q.w),
    )
}

fn q32(q: DQuat) -> Rotation {
    Rotation::from_xyzw(q.x as f32, q.y as f32, q.z as f32, q.w as f32)
}

/// The JS binding's `setRotation`: the f32 quaternion, normalised.
fn q32_normalized(q: DQuat) -> Rotation {
    q32(q).normalize()
}

/// `Math.fround` equality: the f64 rounded to f32 equals Rapier's f32.
fn same32(a: DVec3, b: Vector) -> bool {
    a.x as f32 == b.x && a.y as f32 == b.y && a.z as f32 == b.z
}

/// v turned by q, in the lab's operation order.
fn rotate(q: DQuat, v: DVec3) -> DVec3 {
    let cx = q.y * v.z - q.z * v.y + q.w * v.x;
    let cy = q.z * v.x - q.x * v.z + q.w * v.y;
    let cz = q.x * v.y - q.y * v.x + q.w * v.z;
    DVec3::new(
        v.x + 2.0 * (q.y * cz - q.z * cy),
        v.y + 2.0 * (q.z * cx - q.x * cz),
        v.z + 2.0 * (q.x * cy - q.y * cx),
    )
}

fn normalise_rotation(q: DQuat) -> DQuat {
    let length = q.length();
    assert!(
        length > 0.0 && length.is_finite(),
        "contact world: invalid rotation"
    );
    DQuat::from_xyzw(q.x / length, q.y / length, q.z / length, q.w / length)
}

fn rotation_matrix(q: DQuat) -> [f64; 9] {
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

/// M diag(d) Mᵀ.
fn sandwich(m: &[f64; 9], d: [f64; 3]) -> Mat3 {
    let mut out = [0.0; 9];
    for i in 0..3 {
        for j in 0..3 {
            let mut sum = 0.0;
            for k in 0..3 {
                sum += m[i * 3 + k] * d[k] * m[j * 3 + k];
            }
            out[i * 3 + j] = sum;
        }
    }
    out
}

/// The principal inertia and its frame, as the JS binding reports them.
fn principal(body: &RigidBody) -> ([f64; 3], DQuat) {
    let props = &body.mass_properties().local_mprops;
    let p = props.principal_inertia();
    (
        [f64::from(p.x), f64::from(p.y), f64::from(p.z)],
        q64(props.principal_inertia_local_frame),
    )
}

/// A body's inertia about its mass centre in the frame's axes: R P diag(principal) Pᵀ Rᵀ.
fn world_inertia(body: &RigidBody) -> Mat3 {
    let (p, f) = principal(body);
    let q = q64(*body.rotation());
    let r = DQuat::from_xyzw(
        q.w * f.x + q.x * f.w + q.y * f.z - q.z * f.y,
        q.w * f.y - q.x * f.z + q.y * f.w + q.z * f.x,
        q.w * f.z + q.x * f.y - q.y * f.x + q.z * f.w,
        q.w * f.w - q.x * f.x - q.y * f.y - q.z * f.z,
    );
    sandwich(&rotation_matrix(r), p)
}

/// A body's inertia about its mass centre in its own axes: P diag(principal) Pᵀ.
fn local_inertia(body: &RigidBody) -> Mat3 {
    let (p, f) = principal(body);
    sandwich(&rotation_matrix(f), p)
}

fn volume(shape: &SimpleShape) -> f64 {
    use std::f64::consts::PI;
    match *shape {
        SimpleShape::Box { half_extents: h } => 8.0 * h.x * h.y * h.z,
        SimpleShape::Ball { radius } => 4.0 / 3.0 * PI * radius.powi(3),
        SimpleShape::Cone {
            radius,
            half_height,
        } => 2.0 / 3.0 * PI * radius.powi(2) * half_height,
        SimpleShape::Cylinder {
            radius,
            half_height,
        } => 2.0 * PI * radius.powi(2) * half_height,
    }
}

pub struct ContactWorld<F: ContactFrame> {
    pub world: PhysicsWorld,
    pub frame: F,
    /// Collision terrain, streamed around the bodies; only in a planet frame.
    pub terrain: Option<Arc<Terrain>>,
    pub options: ContactWorldOptions,
    /// Simulated time, s (the frame's clock).
    pub time: f64,
    /// Floating origin, body-fixed metres.
    pub origin: DVec3,
    pub tile_loads: u64,
    pub tile_unloads: u64,
    pub recenters: u64,
    /// Bodies in insertion order, as the lab's Set.
    bodies: Vec<(RigidBodyHandle, BodyRecord)>,
    tiles: OrderedMap<TileCollider>,
}

/// A suspension ray hit on the actual live collider, expressed in contact-frame axes.
#[derive(Clone, Copy, Debug)]
pub struct ContactRayHit {
    pub collider: ColliderHandle,
    pub body: Option<RigidBodyHandle>,
    pub distance_meters: f64,
    pub point: DVec3,
    pub normal: DVec3,
    pub point_velocity: DVec3,
}

/// Extra acceleration (thrust) for a body, called once per step with its state. Leapfrog kicks
/// cover the half steps on both sides of a step, so this must return the average over the step
/// just ended and the step starting; a jump (engine on or off) then lands on the step boundary.
pub type ExtraAcceleration<'a> = &'a mut dyn FnMut(RigidBodyHandle, State) -> DVec3;

impl<F: ContactFrame> ContactWorld<F> {
    pub fn new(
        frame: F,
        terrain: Option<Arc<Terrain>>,
        options: ContactWorldOptions,
        time: f64,
        origin: DVec3,
        ephemeris: &mut dyn EphemerisSource,
    ) -> Self {
        assert!(
            options.step_seconds > 0.0
                && options.tile_keep_meters > options.tile_reach_meters
                && options.recenter_meters > 0.0,
            "contact world: options {options:?}"
        );
        if let Some(terrain) = &terrain {
            let body = frame
                .terrain_body()
                .expect("contact world: terrain is body-fixed; it needs a planet frame");
            assert_eq!(
                terrain.radius_meters, body.radius_meters,
                "contact world: terrain radius differs from {}'s radius",
                body.id
            );
        }
        let mut world = PhysicsWorld {
            gravity: Vector::ZERO,
            ..PhysicsWorld::default()
        };
        world.integration_parameters.dt = options.step_seconds as f32;
        // Spaceflight speeds must not be clipped; the metre-scale contact tolerances stay.
        world.integration_parameters.normalized_max_linear_velocity = f32::MAX;
        ephemeris.extend_to(time + options.step_seconds);
        Self {
            world,
            frame,
            terrain,
            options,
            time,
            origin,
            tile_loads: 0,
            tile_unloads: 0,
            recenters: 0,
            bodies: Vec::new(),
            tiles: OrderedMap::new(),
        }
    }

    pub fn loaded_tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// Loaded terrain colliders, with their body-fixed tile origins.
    pub fn terrain_colliders(&self) -> impl Iterator<Item = &TileCollider> {
        self.tiles.values()
    }

    /// A terrain collider's own triangles, read back from Rapier: vertices relative to the tile
    /// origin, and the triangles' vertex indices.
    pub fn terrain_collider_mesh(&self, tile: &TileCollider) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
        let collider = &self.world.colliders[tile.collider];
        let mesh = collider
            .shape()
            .as_trimesh()
            .expect("contact world: a terrain collider is a triangle mesh");
        (
            mesh.vertices().iter().map(|v| [v.x, v.y, v.z]).collect(),
            mesh.indices().to_vec(),
        )
    }

    pub fn body_handles(&self) -> impl Iterator<Item = RigidBodyHandle> + '_ {
        self.bodies.iter().map(|(h, _)| *h)
    }

    fn record(&self, handle: RigidBodyHandle) -> &BodyRecord {
        &self
            .bodies
            .iter()
            .find(|(h, _)| *h == handle)
            .expect("contact world: unknown body")
            .1
    }

    fn record_mut(&mut self, handle: RigidBodyHandle) -> &mut BodyRecord {
        &mut self
            .bodies
            .iter_mut()
            .find(|(h, _)| *h == handle)
            .expect("contact world: unknown body")
            .1
    }

    fn to_local(&self, p: DVec3) -> DVec3 {
        DVec3::new(
            p.x - self.origin.x,
            p.y - self.origin.y,
            p.z - self.origin.z,
        )
    }

    /// A body's origin in the frame, f64.
    pub fn position(&self, handle: RigidBodyHandle) -> DVec3 {
        self.record(handle).position
    }

    pub fn body(&self, handle: RigidBodyHandle) -> &RigidBody {
        &self.world.bodies[handle]
    }

    /// Read the shapes attached to the body, including each collider's local transform.
    /// Curved shapes are triangulated for display; their collision solver remains analytic.
    pub fn body_collider_meshes(&self, handle: RigidBodyHandle) -> Vec<BodyColliderMesh> {
        self.body(handle)
            .colliders()
            .iter()
            .map(|h| {
                let collider = &self.world.colliders[*h];
                use rapier3d::parry::shape::TypedShape;
                let (vertices, triangles) = match collider.shape().as_typed_shape() {
                    TypedShape::Cuboid(s) => s.to_trimesh(),
                    TypedShape::Ball(s) => s.to_trimesh(16, 8),
                    TypedShape::Cylinder(s) => s.to_trimesh(24),
                    TypedShape::Cone(s) => s.to_trimesh(24),
                    other => panic!("contact overlay: unsupported collider {other:?}"),
                };
                let pose = collider
                    .position_wrt_parent()
                    .expect("body collider has a parent");
                BodyColliderMesh {
                    id: format!("{h:?}"),
                    vertices: vertices
                        .into_iter()
                        .map(|v| {
                            let v = pose * v;
                            [v.x, v.y, v.z]
                        })
                        .collect(),
                    triangles,
                }
            })
            .collect()
    }

    /// Add a body. `extra_before`: acceleration beyond gravity and the frame's over the half step
    /// before now (thrust already running), for the half-step velocity.
    pub fn add_body(
        &mut self,
        ephemeris: &dyn EphemerisSource,
        spec: &ContactBodySpec,
        state: State,
        rotation: DQuat,
        extra_before: DVec3,
    ) -> RigidBodyHandle {
        // Establish a local origin before rounding the initial pose: at planet-radius coordinates
        // f32 can erase a small drop clearance and start a standing body inside the terrain.
        let initial = self.to_local(state.position);
        if initial.length() > self.options.recenter_meters {
            self.recenter(state.position);
        }
        let p = self.to_local(state.position);
        let g = self
            .frame
            .acceleration(ephemeris, self.time, state.position, state.velocity);
        let a = g + extra_before;
        let dt = self.options.step_seconds;
        // The stored velocity is the half-step velocity v − a dt/2.
        let half = DVec3::new(
            state.velocity.x - (a.x * dt) / 2.0,
            state.velocity.y - (a.y * dt) / 2.0,
            state.velocity.z - (a.z * dt) / 2.0,
        );
        let builder = RigidBodyBuilder::dynamic()
            .pose(Pose::from_parts(v32(p), q32(rotation)))
            .linvel(v32(half))
            .can_sleep(self.options.sleeping)
            .ccd_enabled(true);
        let handle = self.world.bodies.insert(builder);
        if spec.lock_rotations {
            self.world.bodies[handle].lock_rotations(true, false);
        }
        let pieces: Vec<Piece> = match &spec.shape {
            BodyShape::Compound(parts) => parts.clone(),
            BodyShape::Simple(shape) => vec![Piece {
                shape: *shape,
                position: DVec3::ZERO,
                rotation: None,
                mass: None,
            }],
        };
        let explicit = pieces.iter().filter(|piece| piece.mass.is_some()).count();
        assert!(
            explicit == 0 || explicit == pieces.len(),
            "contact world: give every compound piece a mass, or none"
        );
        if explicit > 0 {
            let total: f64 = pieces
                .iter()
                .map(|piece| piece.mass.expect("checked").kg)
                .sum();
            assert!(
                (total - spec.mass_kg).abs() <= 1e-9 * spec.mass_kg,
                "contact world: piece masses sum to {total}, body mass {}",
                spec.mass_kg
            );
        }
        let density = spec.mass_kg / pieces.iter().map(|piece| volume(&piece.shape)).sum::<f64>();
        let (mut weights, mut inertia_per_kg) = (Vec::new(), Vec::new());
        for piece in &pieces {
            let mut builder = match piece.shape {
                SimpleShape::Box { half_extents: h } => {
                    ColliderBuilder::cuboid(h.x as f32, h.y as f32, h.z as f32)
                }
                SimpleShape::Ball { radius } => ColliderBuilder::ball(radius as f32),
                SimpleShape::Cone {
                    radius,
                    half_height,
                } => ColliderBuilder::cone(half_height as f32, radius as f32),
                SimpleShape::Cylinder {
                    radius,
                    half_height,
                } => ColliderBuilder::cylinder(half_height as f32, radius as f32),
            };
            let rotation = piece.rotation.map_or(Rotation::IDENTITY, q32_normalized);
            builder = builder.position(Pose::from_parts(v32(piece.position), rotation));
            match piece.mass {
                Some(mass) => {
                    let i = mass.principal_inertia_per_kg;
                    assert!(
                        mass.kg > 0.0 && i.x > 0.0 && i.y > 0.0 && i.z > 0.0,
                        "contact world: piece mass {mass:?}"
                    );
                    builder =
                        builder.mass_properties(MassProperties::with_principal_inertia_frame(
                            Vector::ZERO,
                            mass.kg as f32,
                            v32(i * mass.kg),
                            Rotation::IDENTITY,
                        ));
                    weights.push(mass.kg / spec.mass_kg);
                    inertia_per_kg.push(i);
                }
                None => {
                    builder = builder.density(density as f32);
                    weights.push(volume(&piece.shape) * density / spec.mass_kg);
                }
            }
            builder = builder
                .friction(spec.friction as f32)
                .restitution(spec.restitution as f32)
                .active_events(ActiveEvents::CONTACT_FORCE_EVENTS)
                .contact_force_event_threshold(0.0);
            self.world
                .colliders
                .insert_with_parent(builder, handle, &mut self.world.bodies);
        }
        let body = &self.world.bodies[handle];
        let record = BodyRecord {
            position: state.position,
            weights,
            inertia_per_kg: (explicit > 0).then_some(inertia_per_kg),
            rotation_locked: spec.lock_rotations,
            turn_rotation: normalise_rotation(q64(*body.rotation())),
            turn_angular_velocity: v64(body.angvel()),
            published: *body.rotation(),
            torque: None,
            contact_delta_v: 0.0,
            solver_delta: DVec3::ZERO,
            pending_contact_delta: DVec3::ZERO,
        };
        self.bodies.push((handle, record));
        self.stream_tiles();
        handle
    }

    /// Frame state now (velocity back from the half step). `extra`: acceleration beyond gravity and
    /// the frame's over the coming step (thrust). The solver's change in the last step counts as a
    /// contact force that goes on: half of it belongs to the half step too, so a body resting on the
    /// ground reads 0, not a dt / 2. A sleeping body is at rest in the frame.
    pub fn state(
        &self,
        ephemeris: &dyn EphemerisSource,
        handle: RigidBodyHandle,
        extra: DVec3,
    ) -> State {
        let record = self.record(handle);
        let position = record.position;
        let body = &self.world.bodies[handle];
        let u = v64(body.linvel());
        if body.is_sleeping() {
            return State {
                position,
                velocity: u,
            };
        }
        let dt = self.options.step_seconds;
        let d = record.solver_delta;
        // One fixed-point pass: the Coriolis term depends on the velocity itself.
        let total = |v: DVec3| self.frame.acceleration(ephemeris, self.time, position, v) + extra;
        let mut a = total(u);
        a = total(DVec3::new(
            u.x + (a.x * dt) / 2.0,
            u.y + (a.y * dt) / 2.0,
            u.z + (a.z * dt) / 2.0,
        ));
        State {
            position,
            velocity: DVec3::new(
                u.x + (a.x * dt + d.x) / 2.0,
                u.y + (a.y * dt + d.y) / 2.0,
                u.z + (a.z * dt + d.z) / 2.0,
            ),
        }
    }

    /// Change the accepted step without reinterpreting stored half-step velocities. A physical
    /// boundary velocity is rebased onto the new stagger; no impulse or force limit is applied.
    pub fn set_step_seconds(
        &mut self,
        ephemeris: &dyn EphemerisSource,
        seconds: f64,
        extra: &dyn Fn(RigidBodyHandle) -> DVec3,
    ) {
        assert!(
            seconds.is_finite() && seconds > 0.,
            "contact world: invalid step"
        );
        if seconds == self.options.step_seconds {
            return;
        }
        let states: Vec<_> = self
            .body_handles()
            .map(|h| (h, self.state(ephemeris, h, extra(h))))
            .collect();
        self.options.step_seconds = seconds;
        self.world.integration_parameters.dt = seconds as f32;
        for (handle, state) in states {
            let a0 = self
                .frame
                .acceleration(ephemeris, self.time, state.position, DVec3::ZERO)
                + extra(handle);
            let h = seconds / 2.;
            let spin = self.frame.spin();
            let coriolis = DMat3::from_cols(
                -2. * spin.cross(DVec3::X),
                -2. * spin.cross(DVec3::Y),
                -2. * spin.cross(DVec3::Z),
            );
            // Invert the same one-pass Coriolis reconstruction used by state().
            let matrix = DMat3::IDENTITY + coriolis * h + coriolis * coriolis * (h * h);
            let half = matrix.inverse() * (state.velocity - a0 * h - coriolis * a0 * (h * h));
            let body = &mut self.world.bodies[handle];
            if !body.is_sleeping() {
                body.set_linvel(v32(half), false);
            }
            self.record_mut(handle).solver_delta = DVec3::ZERO;
        }
    }
    pub fn remove_body(&mut self, handle: RigidBodyHandle) {
        let index = self
            .bodies
            .iter()
            .position(|(h, _)| *h == handle)
            .expect("contact world: unknown body");
        self.bodies.remove(index);
        let w = &mut self.world;
        w.bodies.remove(
            handle,
            &mut w.islands,
            &mut w.colliders,
            &mut w.impulse_joints,
            &mut w.multibody_joints,
            true,
        );
    }

    /// Keep the physical collider mass in sync with fuel a part has used.
    pub fn set_body_mass(&mut self, handle: RigidBodyHandle, mass_kg: f64) {
        let record = self.record(handle).clone();
        assert!(mass_kg > 0.0, "contact world: invalid body mass {mass_kg}");
        assert!(
            record.inertia_per_kg.is_none(),
            "set body mass: this body's pieces carry their own masses; use set_piece_masses"
        );
        let colliders = self.world.bodies[handle].colliders().to_vec();
        for (collider, weight) in colliders.iter().zip(&record.weights) {
            self.world.colliders[*collider].set_mass((mass_kg * weight) as f32);
        }
        let w = &mut self.world;
        w.bodies[handle].recompute_mass_properties_from_colliders(&w.colliders);
        w.bodies[handle].wake_up(true);
    }

    /// New masses for a body whose compound pieces carry their own mass (in piece order); each
    /// piece's inertia scales with it.
    pub fn set_piece_masses(&mut self, handle: RigidBodyHandle, masses_kg: &[f64]) {
        let inertia = self
            .record(handle)
            .inertia_per_kg
            .clone()
            .expect("set piece masses: not a body with per-piece masses");
        assert!(
            masses_kg.len() == inertia.len() && masses_kg.iter().all(|m| *m > 0.0),
            "set piece masses: {masses_kg:?}"
        );
        let total: f64 = masses_kg.iter().sum();
        let colliders = self.world.bodies[handle].colliders().to_vec();
        for (k, (&m, i)) in masses_kg.iter().zip(&inertia).enumerate() {
            self.world.colliders[colliders[k]].set_mass_properties(
                MassProperties::with_principal_inertia_frame(
                    Vector::ZERO,
                    m as f32,
                    v32(*i * m),
                    Rotation::IDENTITY,
                ),
            );
            self.record_mut(handle).weights[k] = m / total;
        }
        // Collider edits otherwise reach the body's mass properties only at the next step.
        let w = &mut self.world;
        w.bodies[handle].recompute_mass_properties_from_colliders(&w.colliders);
        w.bodies[handle].wake_up(true);
    }

    /// Normal contact impulse per kilogram in the last step, m/s. Joint forces, prescribed
    /// acceleration and velocity limits are not impact diagnostics.
    pub fn last_contact_delta_v(&self, handle: RigidBodyHandle) -> f64 {
        self.record(handle).contact_delta_v
    }

    /// Body-local torque, N m, applied during the next step only.
    pub fn apply_local_torque(&mut self, handle: RigidBodyHandle, torque: DVec3) {
        assert!(torque.is_finite(), "contact world: non-finite torque");
        if torque == DVec3::ZERO {
            return;
        }
        let record = self.record_mut(handle);
        record.torque = Some(record.torque.unwrap_or(DVec3::ZERO) + torque);
        if self.world.bodies[handle].is_sleeping() {
            self.world.bodies[handle].wake_up(true);
        }
    }

    /// Contact-model impulse in contact-frame axes, torque about this body's COM.
    /// This shares the native body; it is not an extra owner or force-trial state mutation.
    pub fn apply_wrench_impulse(
        &mut self,
        handle: RigidBodyHandle,
        linear: DVec3,
        angular_about_com: DVec3,
    ) {
        self.apply_contact_wrench_impulse(handle, linear, angular_about_com, true);
    }
    /// Actuator/support-motion loads wake; passive suspension/balance does not reset native sleep.
    /// An already sleeping equilibrium skips passive loads and gravity together.
    pub fn apply_contact_wrench_impulse(
        &mut self,
        handle: RigidBodyHandle,
        linear: DVec3,
        angular_about_com: DVec3,
        active: bool,
    ) {
        assert!(linear.is_finite() && angular_about_com.is_finite());
        let b = &mut self.world.bodies[handle];
        assert!(b.is_dynamic());
        if b.is_sleeping() && !active {
            return;
        }
        let before = v64(b.linvel());
        b.apply_impulse(v32(linear), active && linear != DVec3::ZERO);
        b.apply_torque_impulse(
            v32(angular_about_com),
            active && angular_about_com != DVec3::ZERO,
        );
        let delta = v64(b.linvel()) - before;
        self.record_mut(handle).pending_contact_delta += delta;
    }

    /// Resistance integrated at the accepted boundary, rather than an in-step
    /// contact correction. Its velocity change is already fully represented in
    /// the native half-step velocity: do not add another half impulse in state().
    pub fn apply_resistance_impulse(
        &mut self,
        handle: RigidBodyHandle,
        linear: DVec3,
        angular_about_com: DVec3,
    ) {
        assert!(linear.is_finite() && angular_about_com.is_finite());
        let b = &mut self.world.bodies[handle];
        assert!(b.is_dynamic());
        if b.is_sleeping() {
            return;
        }
        b.apply_impulse(v32(linear), false);
        b.apply_torque_impulse(v32(angular_about_com), false);
    }

    /// Query actual collider geometry, including freshly streamed tiles before broad-phase rebuild.
    /// The ray origin is f64 contact-frame position; conversion occurs only after origin subtraction.
    /// Own-body colliders are excluded. Sensors never provide mechanical support.
    pub fn suspension_ray(
        &self,
        excluded: RigidBodyHandle,
        origin: DVec3,
        direction: DVec3,
        reach: f64,
    ) -> Option<ContactRayHit> {
        assert!(
            origin.is_finite()
                && direction.is_finite()
                && (direction.length_squared() - 1.0).abs() < 1e-9
                && reach.is_finite()
                && reach > 0.0
        );
        let ray = Ray::new(v32(origin - self.origin), v32(direction));
        let mut closest: Option<ContactRayHit> = None;
        for (handle, collider) in self.world.colliders.iter() {
            if collider.is_sensor() || collider.parent() == Some(excluded) {
                continue;
            }
            let max = closest.map_or(reach, |h| h.distance_meters);
            let pose = match collider.parent() {
                Some(parent) => {
                    self.world.bodies[parent].position()
                        * collider
                            .position_wrt_parent()
                            .expect("parented collider has local pose")
                }
                None => *collider.position(),
            };
            if let Some(hit) = collider
                .shape()
                .cast_ray_and_get_normal(&pose, &ray, max as f32, true)
            {
                let distance = f64::from(hit.time_of_impact);
                let point = origin + direction * distance;
                let normal = v64(hit.normal).normalize();
                // Back-facing geometry and walls are not a road for this suspension axis.
                if normal.dot(-direction) <= 0.1 {
                    continue;
                }
                let body = collider.parent();
                let point_velocity = body.map_or(DVec3::ZERO, |b| self.point_velocity(b, point));
                closest = Some(ContactRayHit {
                    collider: handle,
                    body,
                    distance_meters: distance,
                    point,
                    normal,
                    point_velocity,
                });
            }
        }
        closest
    }
    /// Native half-step point velocity relative to this frame, including angular motion.
    /// Callers add the same translational half-kick used by state() for accepted-boundary loads.
    pub fn point_velocity(&self, handle: RigidBodyHandle, point: DVec3) -> DVec3 {
        let b = self.body(handle);
        v64(b.velocity_at_point(v32(point - self.origin)))
    }
    /// Explicit hatch clearance against actual live colliders in this owner.
    pub fn box_overlaps(
        &self,
        centre: DVec3,
        rotation: DQuat,
        half_extents: DVec3,
        excluded: Option<RigidBodyHandle>,
    ) -> bool {
        assert!(centre.is_finite() && rotation.is_finite() && half_extents.min_element() > 0.0);
        let pose = Pose::from_parts(v32(centre - self.origin), q32(rotation));
        let shape = SharedShape::cuboid(
            half_extents.x as f32,
            half_extents.y as f32,
            half_extents.z as f32,
        );
        self.world.colliders.iter().any(|(_, c)| {
            if c.is_sensor() || (excluded.is_some() && c.parent() == excluded) {
                return false;
            }
            let cp = match c.parent() {
                Some(parent) => {
                    self.world.bodies[parent].position()
                        * c.position_wrt_parent().expect("parented collider pose")
                }
                None => *c.position(),
            };
            rapier3d::parry::query::intersection_test(&pose, &*shape, &cp, c.shape())
                .expect("unsupported hatch clearance collider query")
        })
    }
    /// One-off impulse (e.g. jumping), unlike an interval suspension support load.
    pub fn apply_instantaneous_impulse(
        &mut self,
        handle: RigidBodyHandle,
        linear: DVec3,
        angular_about_com: DVec3,
    ) {
        assert!(linear.is_finite() && angular_about_com.is_finite());
        let b = &mut self.world.bodies[handle];
        assert!(b.is_dynamic());
        b.apply_impulse(v32(linear), true);
        b.apply_torque_impulse(v32(angular_about_com), true);
    }

    /// Predicted point velocity change from a proposed impulse, without mutating live state.
    pub fn impulse_point_velocity_delta(
        &self,
        handle: RigidBodyHandle,
        linear: DVec3,
        angular_about_com: DVec3,
        point: DVec3,
    ) -> DVec3 {
        let b = self.body(handle);
        if !b.is_dynamic() {
            return DVec3::ZERO;
        }
        let props = b.mass_properties();
        let dv = v64(v32(linear) * props.effective_inv_mass);
        let dw = v64(props.effective_world_inv_inertia * v32(angular_about_com));
        dv + dw.cross(point - self.origin - v64(b.center_of_mass()))
    }

    /// Inverse effective mass for a point impulse, including angular response. Fixed supports
    /// contribute zero. Both participants are added by the tire solve.
    pub fn inverse_point_mass(&self, handle: RigidBodyHandle, point: DVec3, axis: DVec3) -> f64 {
        let b = self.body(handle);
        if !b.is_dynamic() {
            return 0.0;
        }
        let r = point - self.origin - v64(b.center_of_mass());
        let props = b.mass_properties();
        let a = v32(axis);
        let angular = v32(r.cross(axis));
        f64::from(
            (a * props.effective_inv_mass).dot(a)
                + angular.dot(props.effective_world_inv_inertia * angular),
        )
    }

    /// True contact support from the last accepted native solve, not mere ray proximity.
    pub fn body_in_contact_with(
        &self,
        handle: RigidBodyHandle,
        support: ColliderHandle,
        normal: DVec3,
    ) -> bool {
        let support_body = self.world.colliders[support].parent();
        self.world.bodies[handle].colliders().iter().any(|own| {
            self.world
                .narrow_phase
                .contact_pairs_with(*own)
                .any(|pair| {
                    let other = if pair.collider1 == *own {
                        pair.collider2
                    } else {
                        pair.collider1
                    };
                    // Streamed terrain is one stationary support split across tile colliders. A foot
                    // may touch the neighbouring tile while its centre ray hits this tile.
                    let same_support =
                        other == support || self.world.colliders[other].parent() == support_body;
                    same_support
                        && pair.solver_manifolds().iter().any(|m| {
                            v64(m.data.normal).dot(normal).abs() > 0.5
                                && (!m.data.solver_contacts.is_empty()
                                    || m.points.iter().any(|p| p.dist <= 0.0))
                        })
                })
        })
    }

    /// Actual accepted normal support impulse divided by dt, shared by grounded actuators.
    /// Native solver_manifolds selects clustered vs unclustered solved contacts explicitly.
    pub fn support_normal_load(
        &self,
        handle: RigidBodyHandle,
        support: ColliderHandle,
        normal: DVec3,
    ) -> f64 {
        let support_body = self.world.colliders[support].parent();
        self.world.bodies[handle]
            .colliders()
            .iter()
            .map(|own| {
                self.world
                    .narrow_phase
                    .contact_pairs_with(*own)
                    .filter_map(|pair| {
                        let other = if pair.collider1 == *own {
                            pair.collider2
                        } else {
                            pair.collider1
                        };
                        if other != support && self.world.colliders[other].parent() != support_body
                        {
                            return None;
                        }
                        Some(
                            v64(pair.total_impulse()).dot(normal).abs() / self.options.step_seconds,
                        )
                    })
                    .sum::<f64>()
            })
            .sum()
    }

    fn contacts(&self, handle: RigidBodyHandle) -> bool {
        let w = &self.world;
        w.bodies[handle].colliders().iter().any(|&collider| {
            // A contact cluster may own the solver impulses while the exposed geometric manifold
            // has none; be conservative about preserving it.
            w.narrow_phase.contact_pairs_with(collider).any(|pair| {
                pair.manifolds
                    .iter()
                    .any(|m| !m.data.solver_contacts.is_empty() || !m.points.is_empty())
            })
        })
    }

    fn jointed(&self, handle: RigidBodyHandle) -> bool {
        self.world
            .impulse_joints
            .attached_joints(handle)
            .next()
            .is_some()
            || self
                .world
                .multibody_joints
                .attached_joints(handle)
                .next()
                .is_some()
    }

    /// Every body is asleep: at rest on the ground, nothing for a step to change.
    pub fn asleep(&self) -> bool {
        self.bodies
            .iter()
            .all(|(h, _)| self.world.bodies[*h].is_sleeping())
    }

    /// On-rails time: move the clock without stepping Rapier. Only for a world whose bodies are all
    /// asleep: at rest in the body-fixed frame, they stay where they are while the planet turns them
    /// through space.
    pub fn idle_to(&mut self, ephemeris: &mut dyn EphemerisSource, time: f64) {
        assert!(
            self.asleep(),
            "contact world: a body is awake; only resting worlds go on rails"
        );
        assert!(
            time >= self.time,
            "contact world: idle to {time}, before {}",
            self.time
        );
        ephemeris.extend_to(time + self.options.step_seconds);
        self.time = time;
    }

    pub fn step(
        &mut self,
        ephemeris: &mut dyn EphemerisSource,
        extra: Option<ExtraAcceleration<'_>>,
    ) {
        self.step_with_passive(ephemeris, extra, None);
    }

    /// Active forces wake resting bodies; passive damping does not create motion at rest.
    /// Callers must classify forces by intent, rather than by their numerical magnitude.
    pub fn step_with_passive(
        &mut self,
        ephemeris: &mut dyn EphemerisSource,
        mut extra: Option<ExtraAcceleration<'_>>,
        mut passive: Option<ExtraAcceleration<'_>>,
    ) {
        let dt = self.options.step_seconds;
        ephemeris.extend_to(self.time + dt);
        let ephemeris = &*ephemeris;
        let spin = self.frame.spin();
        let turning = spin != DVec3::ZERO;
        let mut kicked: HashMap<RigidBodyHandle, DVec3> = HashMap::new();
        let mut before: HashMap<RigidBodyHandle, (Vector, Rotation)> = HashMap::new();
        let mut spinning: HashMap<RigidBodyHandle, (DQuat, DVec3, Mat3)> = HashMap::new();
        let mut constrained: Vec<RigidBodyHandle> = Vec::new();
        let handles: Vec<RigidBodyHandle> = self.body_handles().collect();

        for &handle in &handles {
            // Raycast tires and finite boot traction are contact constraints too. Their
            // native impulse must not enter the free-attitude override (a pose write marks
            // collider POSITION and repeatedly wakes an otherwise settled supported body).
            if self.jointed(handle)
                || self.contacts(handle)
                || self.record(handle).pending_contact_delta != DVec3::ZERO
            {
                constrained.push(handle);
            }
            self.record_mut(handle).contact_delta_v = 0.0;
            let push = extra.as_mut().map(|f| {
                let state = self.state(ephemeris, handle, DVec3::ZERO);
                f(handle, state)
            });
            if push.is_some_and(|p| p != DVec3::ZERO) {
                self.world.bodies[handle].wake_up(true);
            }
            let position = self.record(handle).position;
            // Rapier gets the f64 position, rounded, when it has drifted from it; setting it every
            // step would teleport resting bodies and drop the contact solver's warm start.
            let local = self.to_local(position);
            // A contact owner keeps native-local pose authority. Reconstructing that
            // pose via a planet-scale absolute f64 subtraction can cause a new POSITION
            // mutation from rounding alone, which Rapier treats as an external teleport.
            if !constrained.contains(&handle)
                && !same32(local, self.world.bodies[handle].translation())
            {
                self.world.bodies[handle].set_translation(v32(local), false);
            }
            let body = &self.world.bodies[handle];
            before.insert(handle, (body.translation(), *body.rotation()));
            if body.is_sleeping() {
                continue;
            }
            let u = v64(body.linvel());
            // Coriolis at the mid-point velocity estimate u + a dt / 2, thrust included.
            let damping = passive.as_mut().map_or(DVec3::ZERO, |f| {
                let state = self.state(ephemeris, handle, DVec3::ZERO);
                f(handle, state)
            });
            let e = push.unwrap_or(DVec3::ZERO) + damping;
            let mut a = self.frame.acceleration(ephemeris, self.time, position, u);
            a = self.frame.acceleration(
                ephemeris,
                self.time,
                position,
                DVec3::new(
                    u.x + ((a.x + e.x) * dt) / 2.0,
                    u.y + ((a.y + e.y) * dt) / 2.0,
                    u.z + ((a.z + e.z) * dt) / 2.0,
                ),
            );
            a = DVec3::new(a.x + e.x, a.y + e.y, a.z + e.z);
            let v = DVec3::new(u.x + a.x * dt, u.y + a.y * dt, u.z + a.z * dt);
            self.world.bodies[handle].set_linvel(v32(v), false);
            kicked.insert(handle, v);

            let record = self.record(handle);
            if record.rotation_locked {
                continue;
            }
            // The f64 attitude, unless something outside changed Rapier's since the last step. The
            // comparison is with what Rapier stored after the last write, so its f32 normalisation
            // does not look like an outside change.
            let body = &self.world.bodies[handle];
            let (r, w) = (*body.rotation(), body.angvel());
            let same = record.published == r && same32(record.turn_angular_velocity, w);
            let (rotation, angular_velocity) = if same {
                (record.turn_rotation, record.turn_angular_velocity)
            } else {
                (normalise_rotation(q64(r)), v64(w))
            };
            spinning.insert(handle, (rotation, angular_velocity, local_inertia(body)));
            if let Some(torque) = record.torque {
                let tau = rotate(rotation, torque);
                self.world.bodies[handle].apply_torque_impulse(
                    v32(DVec3::new(tau.x * dt, tau.y * dt, tau.z * dt)),
                    false,
                );
            }
            if turning {
                let tau = fictitious_torque(
                    &world_inertia(&self.world.bodies[handle]),
                    angular_velocity,
                    spin,
                );
                self.world.bodies[handle].apply_torque_impulse(
                    v32(DVec3::new(tau.x * dt, tau.y * dt, tau.z * dt)),
                    false,
                );
            }
        }

        let (collision_send, _collision_recv) = channel();
        let (force_send, force_recv) = channel();
        let events = ChannelEventCollector::new(collision_send, force_send);
        self.world.step_with_events(&(), &events);
        while let Ok(event) = force_recv.try_recv() {
            // Rapier's force events use the manifolds actually solved (including clustered terrain
            // contacts), not stale geometric impulses.
            let impulse = f64::from(event.total_force_magnitude) * dt;
            for collider in [event.collider1, event.collider2] {
                let Some(body) = self.world.colliders.get(collider).and_then(|c| c.parent()) else {
                    continue;
                };
                if self.bodies.iter().any(|(h, _)| *h == body) {
                    let mass = f64::from(self.world.bodies[body].mass());
                    self.record_mut(body).contact_delta_v += impulse / mass;
                }
            }
        }

        for &handle in &handles {
            let v = kicked.get(&handle).copied();
            let contacts = self.contacts(handle);
            let body = &self.world.bodies[handle];
            let after = v64(body.linvel());
            let (start_translation, start_rotation) = before[&handle];
            let p = self.record(handle).position;
            let t = v64(body.translation());
            // Rapier moves the mass centre by v dt and turns the body about it: origin += v dt + (R0 − R1) c.
            let c = v64(body.local_center_of_mass());
            let r0 = rotate(q64(start_rotation), c);
            let r1 = rotate(q64(*body.rotation()), c);
            let exact = DVec3::new(
                after.x * dt + r0.x - r1.x,
                after.y * dt + r0.y - r1.y,
                after.z * dt + r0.z - r1.z,
            );
            // Free: the solver left the kicked velocity bit for bit and the body moved by exactly that,
            // to f32 rounding (CCD can stop a body short without touching its velocity). A body
            // asleep at the start was not kicked: it moved only if a contact woke it.
            let start = v64(start_translation);
            let moved = t - start;
            let rounding = 1e-6 * (t.length() + start.length()) + 1e-9;
            let free = !constrained.contains(&handle)
                && v.is_some_and(|v| same32(v, body.linvel()))
                && (moved - exact).length() <= rounding;
            let solver_delta = match v {
                Some(v) if !free => DVec3::new(after.x - v.x, after.y - v.y, after.z - v.z),
                _ => DVec3::ZERO,
            };
            let solver_delta = solver_delta + self.record(handle).pending_contact_delta;
            self.record_mut(handle).pending_contact_delta = DVec3::ZERO;
            let torque = self.record(handle).torque.unwrap_or(DVec3::ZERO);
            let turned = spinning
                .get(&handle)
                .copied()
                .filter(|_| free && !constrained.contains(&handle) && !contacts);
            if let Some((rotation, angular_velocity, inertia_local)) = turned {
                let (q, w) =
                    rotation_step(rotation, angular_velocity, &inertia_local, torque, spin, dt);
                let body = &mut self.world.bodies[handle];
                body.set_rotation(q32_normalized(q), false);
                body.set_angvel(v32(w), false);
                let published = *body.rotation();
                let record = self.record_mut(handle);
                (
                    record.turn_rotation,
                    record.turn_angular_velocity,
                    record.published,
                ) = (q, w, published);
            } else {
                let body = &self.world.bodies[handle];
                let (published, angvel) = (*body.rotation(), v64(body.angvel()));
                let record = self.record_mut(handle);
                (
                    record.turn_rotation,
                    record.turn_angular_velocity,
                    record.published,
                ) = (normalise_rotation(q64(published)), angvel, published);
            }
            let r2 = rotate(q64(*self.world.bodies[handle].rotation()), c);
            let record = self.record_mut(handle);
            record.solver_delta = solver_delta;
            record.position = if free {
                // The mass centre moved by v dt; the origin follows the attitude actually kept.
                DVec3::new(
                    p.x + after.x * dt + r0.x - r2.x,
                    p.y + after.y * dt + r0.y - r2.y,
                    p.z + after.z * dt + r0.z - r2.z,
                )
            } else {
                DVec3::new(
                    p.x + (t.x - start.x),
                    p.y + (t.y - start.y),
                    p.z + (t.z - start.z),
                )
            };
            record.torque = None;
        }
        self.time += dt;
        self.recenter_if_needed();
        self.stream_tiles();
    }

    /// Move the floating origin to a body-fixed point, keeping every state unchanged.
    pub fn recenter(&mut self, to: DVec3) {
        self.origin = to;
        for (handle, record) in &self.bodies {
            let local = DVec3::new(
                record.position.x - to.x,
                record.position.y - to.y,
                record.position.z - to.z,
            );
            self.world.bodies[*handle].set_translation(v32(local), false);
        }
        for tile in self.tiles.values() {
            let local = DVec3::new(
                tile.origin.x - to.x,
                tile.origin.y - to.y,
                tile.origin.z - to.z,
            );
            self.world.colliders[tile.collider].set_translation(v32(local));
        }
        self.recenters += 1;
    }

    fn recenter_if_needed(&mut self) {
        for (handle, _) in &self.bodies {
            let t = v64(self.world.bodies[*handle].translation());
            if t.length() > self.options.recenter_meters {
                let to = DVec3::new(
                    self.origin.x + t.x,
                    self.origin.y + t.y,
                    self.origin.z + t.z,
                );
                self.recenter(to);
                return;
            }
        }
    }

    fn stream_tiles(&mut self) {
        let Some(terrain) = self.terrain.clone() else {
            return;
        };
        let ContactWorldOptions {
            tile_level,
            tile_resolution,
            tile_reach_meters,
            tile_keep_meters,
            ..
        } = self.options;
        let r = terrain.radius_meters;
        let mut wanted = std::collections::HashSet::new();
        let mut keep = std::collections::HashSet::new();
        let positions: Vec<DVec3> = self
            .bodies
            .iter()
            .map(|(_, record)| record.position)
            .collect();
        let n = tile_resolution;
        for p in positions {
            // Tiles matter only once the body could reach the ground.
            if p.length() - r - terrain.max_height_meters > tile_reach_meters {
                continue;
            }
            for key in tiles_around(p, tile_reach_meters, tile_level, r) {
                let code = key.code();
                wanted.insert(code);
                if self.tiles.contains_key(code) {
                    continue;
                }
                let tile = build_tile_mesh(
                    key,
                    &*terrain,
                    TileMeshOptions {
                        radius_meters: r,
                        resolution: n,
                    },
                );
                let vertices: Vec<Vector> = tile.positions[..n * n]
                    .iter()
                    .map(|p| Vector::new(p[0], p[1], p[2]))
                    .collect();
                let indices = surface_indices(n);
                let local = self.to_local(tile.origin);
                let collider = ColliderBuilder::trimesh(vertices, indices)
                    .expect("a tile's surface is a valid triangle mesh")
                    .translation(v32(local))
                    .friction(0.8);
                let handle = self.world.colliders.insert(collider);
                self.tiles.insert(
                    code,
                    TileCollider {
                        collider: handle,
                        origin: tile.origin,
                    },
                );
                self.tile_loads += 1;
            }
            for key in tiles_around(p, tile_keep_meters, tile_level, r) {
                keep.insert(key.code());
            }
        }
        for code in self.tiles.key_snapshot() {
            if keep.contains(&code) || wanted.contains(&code) {
                continue;
            }
            let tile = self.tiles.remove(code).expect("listed");
            let w = &mut self.world;
            w.colliders
                .remove(tile.collider, &mut w.islands, &mut w.bodies, true);
            self.tile_unloads += 1;
        }
    }
}

/// Surface triangles of every tile at a resolution, outward counter-clockwise, without skirts.
pub fn surface_indices(resolution: usize) -> Vec<[u32; 3]> {
    let (indices, grid) = build_tile_indices(resolution);
    indices[..grid]
        .chunks(3)
        .map(|t| [t[0], t[1], t[2]])
        .collect()
}

/// The logical frame and terrain are supplied by the owner; the native physics cache is stored
/// as one versioned unit so its arena handles never escape into the game's persistent IDs.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactWorldCheckpoint {
    physics_abi: String,
    physics: Vec<u8>,
    options: ContactWorldOptions,
    time: f64,
    origin: DVec3,
    tile_loads: u64,
    tile_unloads: u64,
    recenters: u64,
    bodies: Vec<(RigidBodyHandle, BodyRecord)>,
    tiles: Vec<(u64, TileCollider)>,
}
impl<F: ContactFrame> ContactWorld<F> {
    pub fn checkpoint(&self) -> ContactWorldCheckpoint {
        ContactWorldCheckpoint {
            physics_abi: "rapier3d-0.35.1/f32/v1".into(),
            physics: bincode::serialize(&self.world)
                .expect("contact checkpoint: serialize physics"),
            options: self.options,
            time: self.time,
            origin: self.origin,
            tile_loads: self.tile_loads,
            tile_unloads: self.tile_unloads,
            recenters: self.recenters,
            bodies: self.bodies.clone(),
            tiles: self
                .tiles
                .keys()
                .map(|key| (key, *self.tiles.get(key).unwrap()))
                .collect(),
        }
    }
    pub fn from_checkpoint(
        frame: F,
        terrain: Option<Arc<Terrain>>,
        saved: ContactWorldCheckpoint,
    ) -> Self {
        assert_eq!(
            saved.physics_abi, "rapier3d-0.35.1/f32/v1",
            "contact checkpoint: incompatible native cache"
        );
        assert!(
            saved.time.is_finite() && saved.origin.is_finite(),
            "contact checkpoint: invalid clock/origin"
        );
        assert!(
            saved.options.step_seconds.is_finite() && saved.options.step_seconds > 0.0,
            "contact checkpoint: invalid step"
        );
        let world: PhysicsWorld = bincode::deserialize(&saved.physics)
            .expect("contact checkpoint: invalid physics cache");
        assert_eq!(
            world.gravity,
            Vector::ZERO,
            "contact checkpoint: native gravity must be disabled"
        );
        assert_eq!(
            world.integration_parameters.dt, saved.options.step_seconds as f32,
            "contact checkpoint: inconsistent native timestep"
        );
        if let Some(terrain) = &terrain {
            assert_eq!(
                terrain.radius_meters,
                frame
                    .terrain_body()
                    .expect("contact checkpoint: missing ground frame")
                    .radius_meters,
                "contact checkpoint: terrain/body mismatch"
            );
        } else {
            assert!(
                saved.tiles.is_empty(),
                "contact checkpoint: terrain tiles without terrain"
            );
        }
        let mut seen = std::collections::HashSet::new();
        for (handle, record) in &saved.bodies {
            assert!(
                seen.insert(*handle) && world.bodies.get(*handle).is_some(),
                "contact checkpoint: missing/duplicate body"
            );
            assert!(
                record.position.is_finite()
                    && record.turn_rotation.is_finite()
                    && record.turn_angular_velocity.is_finite()
                    && record.solver_delta.is_finite()
                    && record.pending_contact_delta.is_finite(),
                "contact checkpoint: invalid body record"
            );
        }
        let mut tiles = OrderedMap::new();
        for (key, tile) in saved.tiles {
            assert!(
                !tiles.contains_key(key)
                    && world.colliders.get(tile.collider).is_some()
                    && tile.origin.is_finite(),
                "contact checkpoint: invalid terrain tile"
            );
            tiles.insert(key, tile);
        }
        Self {
            world,
            frame,
            terrain,
            options: saved.options,
            time: saved.time,
            origin: saved.origin,
            tile_loads: saved.tile_loads,
            tile_unloads: saved.tile_unloads,
            recenters: saved.recenters,
            bodies: saved.bodies,
            tiles,
        }
    }
}
