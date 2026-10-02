//! Contacts in the rotating frame: lab/landing's own checks (`landing-check.ts`, "P2"), with its
//! thresholds. Rapier runs natively here and as WebAssembly there, so these compare what the lab
//! measures, not bits.

use std::sync::Arc;

use glam::{DQuat, DVec3};
use void_landing::{
    BodyShape, ContactBodySpec, ContactFrame, ContactWorld, ContactWorldOptions, FrameState,
    PlanetFrame, SimpleShape, level_for_tile_size, moon_size, pebble,
};
use void_orbit::{
    BodySpec, EllipticElements, Ephemeris, EphemerisOptions, GravityField, OrbitPlane,
    PropagationRun, RotationSpec, SpinSpec, SystemSpec, Tolerances, VesselPropagator, VesselState,
    build_system, suggested_step_seconds,
};
use void_terrain::Terrain;

const TILE_SIZE_METERS: f64 = 300.0;
const RESOLUTION: usize = 33;
const FRAME_TOLERANCES: Tolerances = Tolerances {
    position_meters: 1e-6,
    velocity_meters_per_second: 1e-9,
};

struct Env {
    ephemeris: Ephemeris,
    frame: PlanetFrame,
    terrain: Arc<Terrain>,
}

/// Pebble with an exaggerated J2 and a small moon, so every frame term is exercised.
fn harsh_pebble() -> Env {
    let base = pebble();
    let mut root = base.system.root.clone();
    root.gravity_field = Some(GravityField {
        j2: 0.01,
        reference_radius_meters: 100e3,
    });
    root.children = vec![BodySpec {
        id: "pip".into(),
        name: "Pip".into(),
        color: "#999".into(),
        mass_kg: 2e19,
        radius_meters: 10e3,
        rotation: RotationSpec::Spin(SpinSpec {
            period_seconds: 36_000.0,
            obliquity_radians: 0.0,
            pole_longitude_radians: 0.0,
            angle_at_epoch_radians: 0.0,
        }),
        orbit: Some(EllipticElements {
            semi_major_axis_meters: 400e3,
            eccentricity: 0.0,
            inclination_radians: 0.3,
            longitude_of_ascending_node_radians: 0.0,
            argument_of_periapsis_radians: 0.0,
            mean_anomaly_radians: 1.0,
        }),
        orbit_plane: Some(OrbitPlane::Ecliptic),
        gravity_field: None,
        children: Vec::new(),
    }];
    let system = build_system(&SystemSpec {
        name: "harsh pebble".into(),
        root,
    });
    let mut ephemeris = Ephemeris::new(
        &system,
        EphemerisOptions {
            step_seconds: suggested_step_seconds(&system.bodies, 256.0),
            chunk_steps: 1024,
        },
    );
    ephemeris.extend_to(6000.0);
    let frame = PlanetFrame::new(&ephemeris, 0);
    Env {
        ephemeris,
        frame,
        terrain: base.terrain,
    }
}

fn plain_pebble() -> Env {
    let base = pebble();
    let system = build_system(&base.system);
    let mut ephemeris = Ephemeris::new(
        &system,
        EphemerisOptions {
            step_seconds: 60.0,
            chunk_steps: 1024,
        },
    );
    ephemeris.extend_to(60.0);
    let frame = PlanetFrame::new(&ephemeris, 0);
    Env {
        ephemeris,
        frame,
        terrain: base.terrain,
    }
}

/// A hop from 4 km over the reference sphere (above the 3 km hills): 120 m/s up, 60 m/s east.
fn hop_start(frame: &PlanetFrame) -> FrameState {
    let d = DVec3::new(0.4_f64.cos(), 0.4_f64.sin(), 0.05).normalize();
    let east = DVec3::new(-d.y, d.x, 0.0).normalize();
    let r = frame.body.radius_meters + 4000.0;
    FrameState {
        position: d * r,
        velocity: d * 120.0 + east * 60.0,
    }
}

/// The reference: the orbit crate's integrator in the inertial frame, sampled at times.
fn inertial_reference(env: &mut Env, start: FrameState, times: &[f64]) -> Vec<FrameState> {
    let inertial = env.frame.to_inertial(&env.ephemeris, 0.0, start);
    let mut run = PropagationRun::new(VesselState {
        time: 0.0,
        position: inertial.position,
        velocity: inertial.velocity,
        mass_kg: 1000.0,
    });
    let mut propagator = VesselPropagator::new(&env.ephemeris, FRAME_TOLERANCES);
    times
        .iter()
        .map(|&t| {
            propagator.advance(&mut env.ephemeris, &mut run, t, 10_000_000, None, None);
            let s = run.state();
            env.frame.to_body_fixed(
                &env.ephemeris,
                t,
                FrameState {
                    position: s.position,
                    velocity: s.velocity,
                },
            )
        })
        .collect()
}

fn options() -> ContactWorldOptions {
    ContactWorldOptions {
        step_seconds: 1.0 / 60.0,
        tile_level: level_for_tile_size(100e3, TILE_SIZE_METERS),
        tile_resolution: RESOLUTION,
        tile_reach_meters: 300.0,
        tile_keep_meters: 600.0,
        recenter_meters: 1000.0,
        sleeping: true,
    }
}

fn spec(shape: SimpleShape, mass_kg: f64, friction: f64, restitution: f64) -> ContactBodySpec {
    ContactBodySpec {
        shape: BodyShape::Simple(shape),
        mass_kg,
        friction,
        restitution,
        lock_rotations: false,
    }
}

/// A body-fixed point on the ground at a direction, lifted by some metres.
fn ground(terrain: &Terrain, d: DVec3, lift: f64) -> DVec3 {
    let u = d.normalize();
    u * (terrain.radius_meters + terrain.height(u) + lift)
}

fn clearance(terrain: &Terrain, p: DVec3) -> f64 {
    p.length() - terrain.radius_meters - terrain.height(p.normalize())
}

#[test]
#[should_panic(expected = "contact world: terrain radius")]
fn mismatched_terrain_radius_panics() {
    let Env {
        mut ephemeris,
        frame,
        ..
    } = plain_pebble();
    ContactWorld::new(
        frame,
        Some(moon_size().terrain),
        options(),
        0.0,
        DVec3::ZERO,
        &mut ephemeris,
    );
}

#[test]
fn frame_transform_round_trip() {
    let env = harsh_pebble();
    let s = hop_start(&env.frame);
    let worst = [0.0, 123.4, 5000.0]
        .iter()
        .map(|&t| {
            let back = env.frame.to_body_fixed(
                &env.ephemeris,
                t,
                env.frame.to_inertial(&env.ephemeris, t, s),
            );
            (back.position - s.position)
                .length()
                .max((back.velocity - s.velocity).length())
        })
        .fold(0.0, f64::max);
    println!("frame transform round trip: worst {worst:.2e}");
    assert!(worst < 1e-9);
}

#[test]
fn rotating_frame_equations_vs_inertial() {
    let mut env = harsh_pebble();
    let start = hop_start(&env.frame);
    let times = [30.0, 60.0, 90.0, 120.0, 150.0];
    let reference = inertial_reference(&mut env, start, &times);
    let h = 0.01;
    let (mut t, mut r, mut v, mut worst) = (0.0, start.position, start.velocity, 0.0_f64);
    let eph = &env.ephemeris;
    let acc = |t: f64, r: DVec3, v: DVec3| env.frame.acceleration(eph, t, r, v);
    for (i, &target) in times.iter().enumerate() {
        while t < target - 1e-9 {
            let (k1v, k1r) = (acc(t, r, v), v);
            let (k2v, k2r) = (
                acc(t + h / 2.0, r + k1r * (h / 2.0), v + k1v * (h / 2.0)),
                v + k1v * (h / 2.0),
            );
            let (k3v, k3r) = (
                acc(t + h / 2.0, r + k2r * (h / 2.0), v + k2v * (h / 2.0)),
                v + k2v * (h / 2.0),
            );
            let (k4v, k4r) = (acc(t + h, r + k3r * h, v + k3v * h), v + k3v * h);
            r += ((k1r + k4r) + (k2r + k3r) * 2.0) * (h / 6.0);
            v += ((k1v + k4v) + (k2v + k3v) * 2.0) * (h / 6.0);
            t += h;
        }
        worst = worst.max((r - reference[i].position).length());
    }
    println!(
        "rotating-frame equations vs inertial: 150 s hop, RK4 within {worst:.2e} m of the inertial integration"
    );
    assert!(worst < 1e-3);
}

#[test]
fn rapier_free_flight_vs_inertial() {
    let mut env = harsh_pebble();
    let start = hop_start(&env.frame);
    let times = [30.0, 60.0, 90.0, 120.0, 150.0];
    let reference = inertial_reference(&mut env, start, &times);
    let Env {
        mut ephemeris,
        frame,
        terrain,
    } = env;
    let mut world = ContactWorld::new(
        frame,
        Some(terrain),
        options(),
        0.0,
        start.position,
        &mut ephemeris,
    );
    let body = world.add_body(
        &ephemeris,
        &spec(SimpleShape::Ball { radius: 1.0 }, 1000.0, 0.5, 0.0),
        start,
        DQuat::IDENTITY,
        DVec3::ZERO,
    );
    let (mut worst_p, mut worst_v) = (0.0_f64, 0.0_f64);
    for (i, &target) in times.iter().enumerate() {
        while world.time < target - 1e-9 {
            world.step(&mut ephemeris, None);
        }
        let s = world.state(&ephemeris, body, DVec3::ZERO);
        worst_p = worst_p.max((s.position - reference[i].position).length());
        worst_v = worst_v.max((s.velocity - reference[i].velocity).length());
    }
    println!(
        "Rapier free flight vs inertial: 150 s at 60 Hz within {worst_p:.2e} m and {worst_v:.2e} m/s; {} origin moves",
        world.recenters
    );
    assert!(worst_p < 0.05 && worst_v < 1e-3 && world.recenters > 0);
}

#[test]
fn rest_on_the_ground() {
    let Env {
        mut ephemeris,
        frame,
        terrain,
    } = plain_pebble();
    let at = ground(&terrain, DVec3::new(1.0, 0.02, 0.0), 2.0);
    let mut world = ContactWorld::new(
        frame,
        Some(terrain.clone()),
        options(),
        0.0,
        at,
        &mut ephemeris,
    );
    let shape = SimpleShape::Box {
        half_extents: DVec3::ONE,
    };
    let boxed = world.add_body(
        &ephemeris,
        &spec(shape, 5000.0, 0.8, 0.0),
        FrameState {
            position: at,
            velocity: DVec3::ZERO,
        },
        DQuat::IDENTITY,
        DVec3::ZERO,
    );
    while world.time < 30.0 {
        world.step(&mut ephemeris, None);
    }
    let settled = world.state(&ephemeris, boxed, DVec3::ZERO).position;
    let mut lowest = f64::INFINITY;
    while world.time < 630.0 {
        world.step(&mut ephemeris, None);
        lowest = lowest.min(clearance(
            &terrain,
            world.state(&ephemeris, boxed, DVec3::ZERO).position,
        ));
    }
    let moved = (world.state(&ephemeris, boxed, DVec3::ZERO).position - settled).length();
    println!(
        "rest on the ground: a 2 m box on the 50 m/s equator moved {moved:.2e} m in 10 min after settling; centre at least {lowest:.2} m above the terrain"
    );
    assert!(moved < 0.01 && lowest > 0.5);
}

#[test]
fn rolling_across_tiles() {
    let Env {
        mut ephemeris,
        frame,
        terrain,
    } = plain_pebble();
    let at = ground(&terrain, DVec3::new(0.3, 0.6, 0.74), 3.0);
    let up = at.normalize();
    let east = DVec3::new(-up.y, up.x, 0.0);
    let el = east.x.hypot(east.y);
    let mut world = ContactWorld::new(
        frame,
        Some(terrain.clone()),
        options(),
        0.0,
        at,
        &mut ephemeris,
    );
    let velocity = DVec3::new(east.x / el * 30.0, east.y / el * 30.0, 0.0);
    let ball = world.add_body(
        &ephemeris,
        &spec(SimpleShape::Ball { radius: 1.0 }, 500.0, 0.6, 0.2),
        FrameState {
            position: at,
            velocity,
        },
        DQuat::IDENTITY,
        DVec3::ZERO,
    );
    let mut lowest = f64::INFINITY;
    while world.time < 120.0 {
        world.step(&mut ephemeris, None);
        lowest = lowest.min(clearance(
            &terrain,
            world.state(&ephemeris, ball, DVec3::ZERO).position,
        ));
    }
    let travelled = (world.state(&ephemeris, ball, DVec3::ZERO).position - at).length();
    println!(
        "rolling across tiles: {travelled:.0} m travelled in 120 s, centre never below {lowest:.2} m; {} tile loads, {} unloads, {} origin moves, {} loaded now",
        world.tile_loads,
        world.tile_unloads,
        world.recenters,
        world.loaded_tile_count()
    );
    // The ball's centre sits 1 m up; triangles between samples can cut below the smooth terrain.
    assert!(lowest > 0.8 && travelled > 300.0 && world.tile_unloads > 0 && world.recenters > 0);
}

#[test]
fn floating_origin_move() {
    let Env {
        mut ephemeris,
        frame,
        terrain,
    } = plain_pebble();
    let at = ground(&terrain, DVec3::new(0.0, 1.0, 0.1), 50.0);
    let mut world = ContactWorld::new(frame, Some(terrain), options(), 0.0, at, &mut ephemeris);
    let ball = world.add_body(
        &ephemeris,
        &spec(SimpleShape::Ball { radius: 1.0 }, 500.0, 0.6, 0.2),
        FrameState {
            position: at,
            velocity: DVec3::new(3.0, -2.0, 1.0),
        },
        DQuat::IDENTITY,
        DVec3::ZERO,
    );
    for _ in 0..100 {
        world.step(&mut ephemeris, None);
    }
    let before = world.state(&ephemeris, ball, DVec3::ZERO);
    let to = world.origin + DVec3::new(700.0, -300.0, 200.0);
    world.recenter(to);
    let after = world.state(&ephemeris, ball, DVec3::ZERO);
    let (dp, dv) = (
        (before.position - after.position).length(),
        (before.velocity - after.velocity).length(),
    );
    println!(
        "floating origin move: moving the origin 800 m changes the state by {dp:.2e} m, {dv:.2e} m/s"
    );
    assert!(dp < 1e-3 && dv < 1e-9);
}
