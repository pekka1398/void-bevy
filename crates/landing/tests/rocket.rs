//! The part-joint rocket and the encounter gate: lab/landing's own checks (`landing-check.ts`)
//! with its thresholds.

use std::sync::Arc;

use glam::{DQuat, DVec3};
use void_landing::{
    BodyShape, ContactBodySpec, ContactFrame, ContactWorld, ContactWorldOptions,
    EncounterPhysicsGate, FrameState, LanderControl, LanderOptions, LanderSpec, PartJointRocket,
    PhysicsMode, PlanetFrame, RocketPart, SimpleShape, level_for_tile_size, pebble,
};
use void_orbit::{Ephemeris, EphemerisOptions, Tolerances, build_system};
use void_rotation::{matrix, rotation_step};
use void_terrain::Terrain;

const LAUNCH_SITE: DVec3 = DVec3::new(0.8, 0.55, 0.25);

fn contact_options() -> ContactWorldOptions {
    ContactWorldOptions {
        step_seconds: 1.0 / 60.0,
        tile_level: level_for_tile_size(100e3, 300.0),
        tile_resolution: 33,
        tile_reach_meters: 300.0,
        tile_keep_meters: 600.0,
        recenter_meters: 1000.0,
        sleeping: true,
    }
}

fn options() -> LanderOptions {
    LanderOptions {
        contact: contact_options(),
        tolerances: Tolerances {
            position_meters: 1e-6,
            velocity_meters_per_second: 1e-9,
        },
        band_enter_meters: 200.0,
        band_exit_meters: 400.0,
    }
}

fn boxed(half: DVec3) -> Option<BodyShape> {
    Some(BodyShape::Simple(SimpleShape::Box { half_extents: half }))
}

/// The check's small two-part rocket: (full, upper, booster).
fn specs() -> (LanderSpec, LanderSpec, LanderSpec) {
    let upper = LanderSpec {
        thrust_newtons: 8000.0,
        specific_impulse_seconds: 330.0,
        dry_mass_kg: 300.0,
        fuel_mass_kg: 200.0,
        half_extents: DVec3::new(1.0, 1.05, 1.0),
        contact_shape: boxed(DVec3::new(1.0, 1.05, 1.0)),
        friction: 0.8,
        crash_tolerance_meters_per_second: Some(10.0),
    };
    let booster = LanderSpec {
        thrust_newtons: 28000.0,
        specific_impulse_seconds: 280.0,
        dry_mass_kg: 500.0,
        fuel_mass_kg: 900.0,
        half_extents: DVec3::new(1.0, 1.35, 1.0),
        contact_shape: boxed(DVec3::new(1.0, 1.35, 1.0)),
        friction: 0.8,
        crash_tolerance_meters_per_second: Some(10.0),
    };
    let full = LanderSpec {
        dry_mass_kg: 1000.0,
        fuel_mass_kg: 900.0,
        half_extents: DVec3::new(1.0, 2.05, 1.0),
        ..booster.clone()
    };
    (full, upper, booster)
}

fn plain_pebble() -> (Ephemeris, Arc<Terrain>) {
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
    (ephemeris, base.terrain)
}

fn rocket(eph: &mut Ephemeris, terrain: &Arc<Terrain>) -> PartJointRocket {
    let (full, upper, booster) = specs();
    PartJointRocket::landed(
        eph,
        0,
        terrain.clone(),
        full,
        upper,
        booster,
        options(),
        LAUNCH_SITE,
    )
}

fn up() -> LanderControl {
    LanderControl {
        throttle: 1.0,
        up: 1.0,
        ..Default::default()
    }
}

fn coast() -> LanderControl {
    LanderControl {
        up: 1.0,
        ..Default::default()
    }
}

fn gap(r: &PartJointRocket, eph: &Ephemeris) -> f64 {
    (r.part_state(eph, RocketPart::Upper).position
        - r.part_state(eph, RocketPart::Booster).position)
        .length()
}

/// The angle between two attitudes; atan2 stays accurate for tiny differences.
fn angle_between(a: DQuat, b: DQuat) -> f64 {
    let x = a.w * b.x - a.x * b.w - a.y * b.z + a.z * b.y;
    let y = a.w * b.y + a.x * b.z - a.y * b.w - a.z * b.x;
    let z = a.w * b.z - a.x * b.y + a.y * b.x - a.z * b.w;
    let w = a.w * b.w + a.x * b.x + a.y * b.y + a.z * b.z;
    2.0 * DVec3::new(x, y, z).length().atan2(w.abs())
}

#[test]
fn staging_keeps_both_parts() {
    let (mut eph, terrain) = plain_pebble();
    let mut r = rocket(&mut eph, &terrain);
    r.advance(&mut eph, 1.0, &coast(), None);
    let (world, upper) = r.part_body(RocketPart::Upper).unwrap();
    let booster = r.part_body(RocketPart::Booster).unwrap().1;
    let colliders = (
        world.body(upper).colliders()[0],
        world.body(booster).colliders()[0],
    );
    let joined = world.world.impulse_joints.len() == 1;
    let before = (
        r.part_state(&eph, RocketPart::Upper).position,
        r.part_state(&eph, RocketPart::Booster).position,
    );
    let momentum = |r: &PartJointRocket| {
        let w = r.world();
        let (u, b) = (w.body(upper).linvel(), w.body(booster).linvel());
        DVec3::new(
            f64::from(u.x) * 500.0 + f64::from(b.x) * 1400.0,
            f64::from(u.y) * 500.0 + f64::from(b.y) * 1400.0,
            f64::from(u.z) * 500.0 + f64::from(b.z) * 1400.0,
        )
    };
    let before_momentum = momentum(&r);
    r.separate(&eph);
    let after_momentum = momentum(&r);
    let w = r.world();
    let same = r.part_body(RocketPart::Upper).unwrap().1 == upper
        && r.part_body(RocketPart::Booster).unwrap().1 == booster
        && w.body(upper).colliders()[0] == colliders.0
        && w.body(booster).colliders()[0] == colliders.1;
    let jump = (before.0 - r.part_state(&eph, RocketPart::Upper).position)
        .length()
        .max((before.1 - r.part_state(&eph, RocketPart::Booster).position).length());
    let momentum_jump = (before_momentum - after_momentum).length();
    println!(
        "part-joint staging keeps both parts: joint {} -> {}; same bodies/colliders {same}; position jump {jump:.2e} m; momentum jump {momentum_jump:.2e} kg m/s",
        u8::from(joined),
        w.world.impulse_joints.len()
    );
    assert!(
        joined && w.world.impulse_joints.is_empty() && same && jump < 1e-8 && momentum_jump < 0.1
    );
}

#[test]
fn burn_and_release() {
    let (mut eph, terrain) = plain_pebble();
    let (_, _, booster) = specs();
    let mut r = rocket(&mut eph, &terrain);
    r.advance(&mut eph, 3.0, &up(), None);
    let joined_distance = gap(&r, &eph);
    let burned = booster.fuel_mass_kg - r.fuel_kg();
    let attached = !r.separated() && r.world().world.impulse_joints.len() == 1;
    r.separate(&eph);
    let at_split = gap(&r, &eph);
    r.advance(&mut eph, 1.0, &coast(), None);
    let after = gap(&r, &eph);
    println!(
        "part-joint burn and release: attached distance {joined_distance:.3} m, burned {burned:.1} kg, one second after release gap {after:.3} m"
    );
    assert!(
        attached && burned > 20.0 && (joined_distance - 2.4).abs() < 0.15 && after > at_split + 0.2
    );
}

#[test]
fn two_stage_rocket_switches_between_orbital_and_contact_physics() {
    let (mut eph, terrain) = plain_pebble();
    let mut r = rocket(&mut eph, &terrain);
    r.advance(&mut eph, 15.0, &up(), None);
    let reached = r.mode() == PhysicsMode::Flight
        && r.mode_changes
            .first()
            .is_some_and(|c| c.from == PhysicsMode::Contact && c.to == PhysicsMode::Flight);
    r.separate(&eph);
    let booster_before = r.part_state(&eph, RocketPart::Booster).position;
    r.advance(&mut eph, 2.0, &coast(), None);
    let booster_advanced =
        (booster_before - r.part_state(&eph, RocketPart::Booster).position).length() > 1.0;
    let mut samples = 0;
    while r.mode() == PhysicsMode::Flight && r.time() < 500.0 && samples < 1000 {
        r.advance(&mut eph, 0.5, &coast(), None);
        samples += 1;
    }
    let returned = r.mode() == PhysicsMode::Contact
        && r.mode_changes
            .iter()
            .any(|c| c.from == PhysicsMode::Flight && c.to == PhysicsMode::Contact);
    let changes: Vec<String> = r
        .mode_changes
        .iter()
        .map(|c| format!("{:?}→{:?}", c.from, c.to))
        .collect();
    println!(
        "two-stage rocket switches: {}; coasted {} s after ascent",
        changes.join(" → "),
        f64::from(samples) * 0.5
    );
    assert!(reached && booster_advanced && returned && r.mode_changes.len() == 2);
}

#[test]
fn staged_parts_switch_independently_and_a_fast_impact_destroys_the_booster() {
    let (mut eph, terrain) = plain_pebble();
    let mut r = rocket(&mut eph, &terrain);
    r.crash_detection = true;
    r.advance(&mut eph, 15.0, &up(), None);
    r.separate(&eph);
    let mut waited = 0.0;
    while r.part_mode(RocketPart::Booster) == PhysicsMode::Flight && waited < 600.0 {
        r.advance(&mut eph, 0.5, &up(), None);
        waited += 0.5;
    }
    let booster_in_contact = r.part_mode(RocketPart::Booster) == PhysicsMode::Contact;
    let upper_in_flight = r.mode() == PhysicsMode::Flight;
    let one_world = r.contact_worlds().len() == 1;
    r.advance(&mut eph, 1.0, &coast(), None);
    let still_flying =
        r.mode() == PhysicsMode::Flight && r.part_mode(RocketPart::Booster) != PhysicsMode::Flight;
    println!(
        "staged parts switch independently: booster entered contact after {waited} s; upper in {:?} at {:.1} km; transitions {}",
        r.mode(),
        r.clearance(&eph) / 1000.0,
        r.mode_changes.len()
    );
    assert!(
        booster_in_contact
            && upper_in_flight
            && one_world
            && still_flying
            && r.mode_changes.len() == 1
    );

    // The booster meets the ground at about 200 m/s: it must be destroyed, not bounced back up.
    let mut fell = 0.0;
    while r.part_mode(RocketPart::Booster) == PhysicsMode::Contact && fell < 30.0 {
        r.advance(&mut eph, 0.25, &coast(), None);
        fell += 0.25;
    }
    let crash = r.crashes.first().copied();
    println!(
        "high-speed impact destroys the part: booster {:?} after {fell} s in contact (speed change {:.1} m/s); upper {:?}",
        r.part_mode(RocketPart::Booster),
        crash.map_or(f64::NAN, |c| c.delta_v),
        r.mode()
    );
    assert!(
        r.part_mode(RocketPart::Booster) == PhysicsMode::Destroyed
            && crash.is_some_and(|c| c.part == RocketPart::Booster)
            && r.crashes.len() == 1
            && r.mode() == PhysicsMode::Flight
            && r.contact_worlds().is_empty()
    );
}

#[test]
fn gentle_touchdown_survives() {
    let (mut eph, terrain) = plain_pebble();
    let mut r = rocket(&mut eph, &terrain);
    r.crash_detection = true;
    r.advance(&mut eph, 5.0, &coast(), None);
    let w = r.world();
    let dv = w
        .last_contact_delta_v(r.part_body(RocketPart::Upper).unwrap().1)
        .max(w.last_contact_delta_v(r.part_body(RocketPart::Booster).unwrap().1));
    println!(
        "gentle touchdown survives: {} crashes; resting contact speed change {dv:.3} m/s per step",
        r.crashes.len()
    );
    assert!(r.crashes.is_empty() && r.mode() == PhysicsMode::Contact);
}

#[test]
fn contact_and_flight_free_attitude_agree() {
    // ContactWorld's free body and flight use the same frame and prescribed torque: 1 s on, 1 s off.
    let (mut eph, terrain) = plain_pebble();
    let frame = PlanetFrame::new(&eph, 0);
    let spin = frame.spin();
    let high = DVec3::new(0.0, 0.0, terrain.radius_meters + 50_000.0);
    let mut world = ContactWorld::new(frame, Some(terrain), contact_options(), 0.0, high, &mut eph);
    let q0 = DQuat::from_xyzw(0.1, 0.2, 0.3, 0.93);
    let q0 = DQuat::from_xyzw(
        q0.x / q0.length(),
        q0.y / q0.length(),
        q0.z / q0.length(),
        q0.w / q0.length(),
    );
    let (_, upper, _) = specs();
    let spec = ContactBodySpec {
        shape: upper.contact_shape.unwrap(),
        mass_kg: 500.0,
        friction: 0.8,
        restitution: 0.0,
        lock_rotations: false,
    };
    let body = world.add_body(
        &eph,
        &spec,
        FrameState {
            position: high,
            velocity: DVec3::ZERO,
        },
        q0,
        DVec3::ZERO,
    );
    let props = &world.body(body).mass_properties().local_mprops;
    let p = props.principal_inertia();
    let f = props.principal_inertia_local_frame;
    let m = matrix(DQuat::from_xyzw(
        f64::from(f.x),
        f64::from(f.y),
        f64::from(f.z),
        f64::from(f.w),
    ));
    let d = [f64::from(p.x), f64::from(p.y), f64::from(p.z)];
    let inertia: [f64; 9] = std::array::from_fn(|n| {
        let (i, j) = (n / 3, n % 3);
        (0..3).map(|k| m[i * 3 + k] * d[k] * m[j * 3 + k]).sum()
    });
    let turn = DVec3::new(0.06, 0.03, -0.08);
    let dt = contact_options().step_seconds;
    let (mut rotation, mut angular_velocity) = (q0, DVec3::ZERO);
    for k in 0..120 {
        let torque = if k < 60 { turn * 6000.0 } else { DVec3::ZERO };
        world.apply_local_torque(body, torque);
        world.step(&mut eph, None);
        (rotation, angular_velocity) =
            rotation_step(rotation, angular_velocity, &inertia, torque, spin, dt);
    }
    let b = world.body(body);
    let q = DQuat::from_xyzw(
        f64::from(b.rotation().x),
        f64::from(b.rotation().y),
        f64::from(b.rotation().z),
        f64::from(b.rotation().w),
    );
    let w = DVec3::new(
        f64::from(b.angvel().x),
        f64::from(b.angvel().y),
        f64::from(b.angvel().z),
    );
    let angle = angle_between(q, rotation);
    let spin_gap = (w - angular_velocity).length();
    println!(
        "contact and flight free attitude agree: turned {:.1} deg in ContactWorld; flight within {:.2e} deg, spin within {spin_gap:.2e} of {:.2e} rad/s",
        angle_between(q0, q).to_degrees(),
        angle.to_degrees(),
        w.length()
    );
    assert!(angle < 1e-6 && spin_gap < 1e-3 * w.length());
}

#[test]
fn steering_in_flight_and_spin_across_hand_offs() {
    let (mut eph, terrain) = plain_pebble();
    let mut craft = rocket(&mut eph, &terrain);
    let dt = contact_options().step_seconds;
    let steer = LanderControl {
        turn: Some(DVec3::new(0.05, 0.0, 0.0)),
        ..up()
    };
    // Climb straight to just below the band exit, then steer lightly across it one step at a time.
    let mut g = 0;
    while craft.mode() == PhysicsMode::Contact
        && craft.clearance(&eph) < options().band_exit_meters - 60.0
        && g < 60 * 40
    {
        craft.advance(&mut eph, dt, &up(), None);
        g += 1;
    }
    let (mut before, mut exit_jump) = (
        craft.part_angular_velocity(RocketPart::Upper),
        f64::INFINITY,
    );
    let mut g = 0;
    while craft.mode() == PhysicsMode::Contact && g < 60 * 20 {
        before = craft.part_angular_velocity(RocketPart::Upper);
        craft.advance(&mut eph, dt, &steer, None);
        g += 1;
    }
    if craft.mode() == PhysicsMode::Flight {
        exit_jump = (craft.part_angular_velocity(RocketPart::Upper) - before).length()
            / before.length().max(1e-9);
    }
    // In flight: one second of pitch turns the stack; released, nothing damps the spin.
    let q1 = craft.orientation();
    craft.advance(
        &mut eph,
        1.0,
        &LanderControl {
            turn: Some(DVec3::X),
            ..coast()
        },
        None,
    );
    let turned = angle_between(q1, craft.orientation());
    let spinning = craft.part_angular_velocity(RocketPart::Upper).length();
    craft.advance(&mut eph, 2.0, &coast(), None);
    let decayed = craft.part_angular_velocity(RocketPart::Upper).length();
    let flying = craft.mode() == PhysicsMode::Flight;
    // Coast back down and step across the band entry.
    let mut g = 0;
    while craft.mode() == PhysicsMode::Flight && g < 2000 && craft.clearance(&eph) > 400.0 {
        craft.advance(&mut eph, 0.5, &coast(), None);
        g += 1;
    }
    let light = LanderControl {
        turn: Some(DVec3::new(0.05, 0.0, 0.0)),
        ..coast()
    };
    let (mut return_jump, mut spin_at_entry) = (f64::INFINITY, 0.0);
    let mut g = 0;
    while craft.mode() == PhysicsMode::Flight && g < 60 * 30 {
        let w0 = craft.part_angular_velocity(RocketPart::Upper);
        spin_at_entry = w0.length();
        craft.advance(&mut eph, dt, &light, None);
        if craft.mode() == PhysicsMode::Contact {
            return_jump = (craft.part_angular_velocity(RocketPart::Upper) - w0).length()
                / w0.length().max(1e-9);
        }
        g += 1;
    }
    println!(
        "steering in orbital flight: 1 s of pitch turned the stack {:.1} deg; released 2 s, spin {spinning:.2e} -> {decayed:.2e} rad/s (x{:.9})",
        turned.to_degrees(),
        decayed / spinning
    );
    println!(
        "angular velocity carried across physics hand-offs: contact -> flight at {:.2e} rad/s changed {:.2}%; flight -> contact at {spin_at_entry:.2e} rad/s: {:.2}%",
        before.length(),
        exit_jump * 100.0,
        return_jump * 100.0
    );
    assert!(flying && turned > 5f64.to_radians() && (decayed / spinning - 1.0).abs() < 1e-6);
    assert!(
        exit_jump < 0.05 && return_jump < 0.05 && before.length() > 0.01 && spin_at_entry > 0.01
    );
}

#[test]
fn encounter_gate() {
    let mut gate = EncounterPhysicsGate::default();
    let first = FrameState {
        position: DVec3::new(10_000.0, -50.0, 200.0),
        velocity: DVec3::ZERO,
    };
    let at = |d: f64| FrameState {
        position: first.position + DVec3::new(d, 0.0, 0.0),
        velocity: DVec3::ZERO,
    };
    let far = gate.update("active", first, "target", at(10_001.0), 0.0);
    let enter = gate.update("active", first, "target", at(9_999.0), 0.0);
    let stay = gate.update("target", at(12_000.0), "active", first, 0.0);
    let exit = gate.update("active", first, "target", at(15_001.0), 0.0);
    assert!(!far.physics && enter.physics && stay.physics && exit.changed && !exit.physics);
    assert!(!gate.is_physics_active("active") && gate.active_pairs().is_empty());

    let fast = gate.update(
        "active",
        FrameState {
            position: first.position,
            velocity: DVec3::new(30_000.0, 0.0, 0.0),
        },
        "target",
        FrameState {
            position: first.position + DVec3::new(0.0, 20_000.0, 0.0),
            velocity: DVec3::new(30_000.0, -30_000.0, 0.0),
        },
        1.0,
    );
    println!(
        "encounter gate: a 30 km/s pass predicted to miss by {:.3} m in {:.4} s",
        fast.closest_approach_meters, fast.time_to_closest_approach_seconds
    );
    assert!(
        fast.physics && fast.distance_meters == 20_000.0 && fast.closest_approach_meters < 1e-8
    );
    assert!(
        fast.time_to_closest_approach_seconds > 0.0 && fast.time_to_closest_approach_seconds < 1.0
    );
}
