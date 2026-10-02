//! The main game's wiring against lab/flight's checks (`flight-check.ts`), with its thresholds.
//! They test the wiring, not the features: each feature crate has its own checks.

use glam::{DQuat, DVec3};
use void_app::flight::{GameTerrain, game_planet_by_id, vessel_axes};
use void_landing::{
    DemoRocket, LanderControl, PartJointRocket, PhysicsMode, RocketPart, demo_rocket,
    planet_ephemeris, predict_coast,
};
use void_lod::{TileMeshOptions, build_tile_mesh, tile_containing};
use void_navball::{NavballInput, navball_basis, to_ball};
use void_orbit::{
    AttitudeLaw, DominanceTree, Ephemeris, FlightPlan, ManeuverSpec, PlanEngine, PropagationRun,
    ReferenceMode, STANDARD_GRAVITY, VesselState, body_orientation,
};
use void_terrain::{Terrain, check_terrain_contract};
use void_view::{FocusGeometry, FocusKind, OrbitCamera, ViewMode, view_state};

/// atan2 of |a × b| and a · b stays accurate for tiny angles, where acos of the dot does not.
fn angle(a: DVec3, b: DVec3) -> f64 {
    a.cross(b).length().atan2(a.dot(b))
}

struct Setup {
    ephemeris: Ephemeris,
    index: usize,
    demo: DemoRocket,
    planet: void_app::flight::GamePlanet,
}

fn setup() -> Setup {
    let planet = game_planet_by_id("aurelia", None);
    let (ephemeris, index) = planet_ephemeris(&planet.planet);
    let mut demo = demo_rocket(&planet.planet.terrain);
    if let Some(site) = planet.launch_site {
        demo.launch_site = site;
    }
    Setup {
        ephemeris,
        index,
        demo,
        planet,
    }
}

fn launch(s: &mut Setup) -> PartJointRocket {
    PartJointRocket::landed(
        &mut s.ephemeris,
        s.index,
        s.planet.planet.terrain.clone(),
        s.demo.full.clone(),
        s.demo.upper.clone(),
        s.demo.booster.clone(),
        s.demo.options,
        s.demo.launch_site,
    )
}

fn idle() -> LanderControl {
    LanderControl {
        up: 1.0,
        turn: Some(DVec3::ZERO),
        ..Default::default()
    }
}

#[test]
fn launch_site_and_rocket_attitude() {
    // The game draws in the planet's body-fixed axes, so the drawn attitude is the part's own
    // body-fixed attitude; carried by the planet's axes it is the attitude in space.
    let mut s = setup();
    let mut rocket = launch(&mut s);
    let home = s.ephemeris.bodies()[s.index].clone();
    let upright = |rocket: &PartJointRocket, eph: &Ephemeris| {
        let axis = rocket.part_orientation(RocketPart::Upper) * DVec3::Y;
        let radial = rocket
            .part_state(eph, RocketPart::Upper)
            .position
            .normalize();
        angle(axis, radial)
    };
    let at_start = upright(&rocket, &s.ephemeris);
    rocket.advance(&mut s.ephemeris, 30.0, &idle(), None);
    let dry = s
        .planet
        .planet
        .terrain
        .height(s.demo.launch_site.normalize())
        > s.planet.sea_level;
    let intact = rocket.part_mode(RocketPart::Upper) != PhysicsMode::Destroyed
        && rocket.part_mode(RocketPart::Booster) != PhysicsMode::Destroyed;
    // In space: the body-fixed axis turned by the planet's axes, against the inertial radial.
    let t = rocket.time();
    let axes = body_orientation(&home.rotation, t);
    let u = rocket.part_orientation(RocketPart::Upper) * DVec3::Y;
    let in_space = axes[0] * u.x + axes[1] * u.y + axes[2] * u.z;
    let inertial = rocket.frame.to_inertial(
        &s.ephemeris,
        t,
        rocket.part_state(&s.ephemeris, RocketPart::Upper),
    );
    let radial = (inertial.position - s.ephemeris.body_position(s.index, t)).normalize();
    let tilt = angle(in_space, radial);
    let body_fixed_tilt = upright(&rocket, &s.ephemeris);
    println!(
        "upright at launch to {at_start:.1e} rad; after 30 s on the slope tilted {:.2} deg, in space {:.2} deg (difference {:.1e} rad); sea {} m",
        body_fixed_tilt.to_degrees(),
        tilt.to_degrees(),
        (tilt - body_fixed_tilt).abs(),
        s.planet.sea_level
    );
    assert!(
        dry && intact,
        "layered launch stays on dry ground with both parts intact"
    );
    // Rapier keeps attitudes as f32 quaternions: agreement is to f32.
    assert!(at_start < 1e-6 && (tilt - body_fixed_tilt).abs() < 1e-6);
}

#[test]
fn corotating_camera_stays_fixed_to_the_ground() {
    let s = setup();
    let home = &s.ephemeris.bodies()[s.index];
    let mut camera = OrbitCamera::new(DVec3::new(0.3, -0.8, 0.5).normalize(), 45.0);
    let local = |camera: &OrbitCamera, t: f64| {
        let a = body_orientation(&home.rotation, t);
        DVec3::new(
            camera.direction.dot(a[0]),
            camera.direction.dot(a[1]),
            camera.direction.dot(a[2]),
        )
    };
    let start = local(&camera, 0.0);
    let (mut t, mut worst) = (0.0, 0.0_f64);
    for _ in 0..360 {
        camera.corotate(home.rotation.axis(), home.rotation.rate() * 60.0);
        t += 60.0;
        worst = worst.max((local(&camera, t) - start).length());
    }
    println!("6 h in 1 min steps: drift {worst:.1e} (unit vector)");
    assert!(worst < 1e-9);
    let landed = view_state(
        ViewMode::Single,
        false,
        &FocusGeometry {
            kind: FocusKind::Vessel,
            radial: Some(DVec3::X),
            north: home.rotation.axis(),
            reference_radius: home.radius_meters,
            altitude: 0.0,
            focus_radius: 0.0,
        },
        45.0,
    );
    assert!(landed.map_weight == 0.0 && landed.corotation == 1.0);
}

#[test]
fn map_path_matches_the_body_fixed_forecast() {
    let mut s = setup();
    let mut rocket = launch(&mut s);
    rocket.advance(&mut s.ephemeris, 2.0, &idle(), None);
    // Full burn with the booster (liftoff thrust-to-weight about 2 on Aurelia), then the coast.
    let full = LanderControl {
        throttle: 1.0,
        ..idle()
    };
    rocket.advance(&mut s.ephemeris, 60.0, &full, None);
    let state = rocket.body_fixed_state(&s.ephemeris);
    let prediction = predict_coast(
        &mut s.ephemeris,
        &rocket.frame,
        &s.planet.planet.terrain,
        s.demo.options.tolerances,
        rocket.time(),
        state,
        rocket.mass_kg(),
        6000.0,
    );
    let trajectory = &prediction.trajectory;
    let (mut worst, mut compared) = (0.0_f64, 0);
    for &(time, position) in &prediction.points {
        if time > trajectory.last_time() || prediction.impact.is_some_and(|(at, _)| at == time) {
            continue;
        }
        let (p, v) = trajectory.sample(time);
        let fixed = rocket.frame.to_body_fixed(
            &s.ephemeris,
            time,
            void_landing::FrameState {
                position: p,
                velocity: v,
            },
        );
        worst = worst.max((fixed.position - position).length());
        compared += 1;
    }
    let bodies = s.ephemeris.bodies().to_vec();
    let mut positions = vec![DVec3::ZERO; bodies.len()];
    s.ephemeris.positions_at(rocket.time(), &mut positions);
    let inertial = rocket.frame.to_inertial(&s.ephemeris, rocket.time(), state);
    let reference = DominanceTree::new(&bodies).dominant(&positions, inertial.position);
    let past_impact = prediction
        .impact
        .map_or(0.0, |(at, _)| trajectory.last_time() - at);
    println!(
        "{compared} points, largest difference {worst:.1e} m; {} inertial samples to T+{:.0} s; impact {:?}, path ends {past_impact:.2} s after it; reference {}",
        trajectory.count(),
        trajectory.last_time(),
        prediction.impact.map(|(at, _)| at),
        bodies[reference].name
    );
    assert!(compared >= 5 && worst < 1e-3 && reference == s.index);
    assert!((0.0..=15.0).contains(&past_impact));
}

#[test]
fn navball_follows_the_steering_keys() {
    // S (torque about local +x) moves the nose up the ball, D (about local +z) to the right.
    let attitude = DQuat::from_xyzw(0.3, -0.5, 0.2, 0.8).normalize();
    let (nose, top) = vessel_axes(attitude);
    let basis = navball_basis(&NavballInput {
        nose,
        top,
        up: DVec3::new(0.2, 0.9, 0.4).normalize(),
        pole: DVec3::Z,
        prime_meridian: DVec3::X,
        velocity: DVec3::ZERO,
    });
    let a = 0.01;
    let turned = |axis: DVec3| {
        let (nose, _) = vessel_axes(attitude * DQuat::from_axis_angle(axis, a));
        to_ball(&basis, nose)
    };
    let (s, d) = (turned(DVec3::X), turned(DVec3::Z));
    println!(
        "a 0.01 rad turn: S moves the nose to ball ({:.1e}, {:.2e}), D to ({:.2e}, {:.1e})",
        s.x, s.y, d.x, d.y
    );
    assert!(s.y > 0.9 * a && s.x.abs() < 1e-9 && d.x > 0.9 * a && d.y.abs() < 1e-9);
}

#[test]
fn scenery_terrain_for_landing_and_drawing() {
    let s = setup();
    let planet = &s.planet;
    assert_eq!(planet.terrain_id, GameTerrain::Layered);
    let contract = check_terrain_contract(&planet.planet.terrain, 256);
    assert!(contract.is_empty(), "landing contract: {contract:?}");
    // The drawn tiles rebuild the terrain from its config; collision tiles use the planet's own.
    let rebuilt = Terrain::from_config(&planet.planet.terrain_config.clone());
    let mut worst = 0.0_f64;
    for cell in [1.0, 10.0, 100.0, 1000.0] {
        for d in [s.demo.launch_site.normalize(), DVec3::X, DVec3::Z] {
            worst = worst.max(
                (rebuilt.sample(d, Some(cell)).0 - planet.planet.terrain.sample(d, Some(cell)).0)
                    .abs(),
            );
        }
    }
    let contact = s.demo.options.contact;
    let key = tile_containing(s.demo.launch_site.normalize(), contact.tile_level);
    let options = TileMeshOptions {
        radius_meters: planet.planet.terrain.radius_meters,
        resolution: contact.tile_resolution,
    };
    let collision = build_tile_mesh(key, &*planet.planet.terrain, options);
    let drawn = build_tile_mesh(key, &rebuilt, options);
    println!(
        "sample difference {worst} m; {} matching tile heights",
        collision.heights.len()
    );
    assert!(
        worst == 0.0
            && collision.positions == drawn.positions
            && collision.heights == drawn.heights
    );
    let luna = game_planet_by_id("luna", None);
    assert!(!luna.atmosphere && luna.terrain_id == GameTerrain::Hills);
}

#[test]
fn upper_stage_maneuver_follows_the_flight_plan() {
    // The upper stage consumes the same fuel and follows the same finite Frenet burn as the
    // orbit crate's FlightPlan.
    let mut s = setup();
    let mut rocket = launch(&mut s);
    let still = LanderControl {
        up: 1.0,
        ..Default::default()
    };
    rocket.advance(&mut s.ephemeris, 2.0, &still, None);
    rocket.advance(
        &mut s.ephemeris,
        60.0,
        &LanderControl {
            throttle: 1.0,
            ..still.clone()
        },
        None,
    );
    rocket.separate(&s.ephemeris);
    let live = rocket.frame.to_inertial(
        &s.ephemeris,
        rocket.time(),
        rocket.part_state(&s.ephemeris, RocketPart::Upper),
    );
    let mut plan = FlightPlan::new(
        &s.ephemeris,
        s.demo.options.tolerances,
        PlanEngine {
            thrust_newtons: s.demo.upper.thrust_newtons,
            exhaust_velocity: s.demo.upper.specific_impulse_seconds * STANDARD_GRAVITY,
            dry_mass_kg: s.demo.upper.dry_mass_kg,
        },
        60.0,
    );
    plan.rebase(&PropagationRun::new(VesselState {
        time: rocket.time(),
        position: live.position,
        velocity: live.velocity,
        mass_kg: rocket.mass_kg(),
    }));
    plan.add(ManeuverSpec {
        start_time: rocket.time() + 2.0,
        reference_body: s.index,
        reference_mode: ReferenceMode::Fixed,
        prograde: 5.0,
        normal: 0.0,
        radial: 0.0,
    });
    plan.extend(&mut s.ephemeris, 100_000);
    let burn = plan.burns()[0];
    rocket.advance(&mut s.ephemeris, 2.0, &still, None);
    rocket.advance(
        &mut s.ephemeris,
        burn.end_time - burn.start_time,
        &LanderControl {
            throttle: 1.0,
            orbital_attitude: Some(AttitudeLaw::Frenet {
                reference_body: s.index,
                tangent: 1.0,
                normal: 0.0,
                radial: 0.0,
            }),
            rotation: Some(DQuat::IDENTITY),
            ..still
        },
        None,
    );
    let actual = rocket.frame.to_inertial(
        &s.ephemeris,
        rocket.time(),
        rocket.part_state(&s.ephemeris, RocketPart::Upper),
    );
    let (expected, _) = plan.trajectory.sample(rocket.time());
    let position = (actual.position - expected).length();
    let fuel = (rocket.mass_kg() - burn.mass_after_kg).abs();
    println!("position {position:.2e} m, mass {fuel:.2e} kg");
    assert!(position < 0.1 && fuel < 1e-6);
}
