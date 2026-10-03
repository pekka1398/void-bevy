//! The environment query: gravity and surroundings are one physical answer whatever frame asks,
//! gravity is the integrator's field, and air, ground and sea are read in the body's frame.
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use glam::{DQuat, DVec3};
use void_environment::{Atmosphere, BodyEnvironment, Environment};
use void_frames::{FrameId, Motion, State};
use void_orbit::{
    Ephemeris, EphemerisOptions, SystemFrames, SystemSpec, Tolerances, VesselPropagator,
    build_system, suggested_step_seconds,
};
use void_terrain::{DEFAULT_LAYERED, LayeredOptions, SEA_LEVEL, Terrain, TerrainConfig};

const T: f64 = 18_000.0;

fn sol() -> Ephemeris {
    let path = format!("{}/../orbit/systems/sol.json", env!("CARGO_MANIFEST_DIR"));
    let system = build_system(&SystemSpec::from_json(
        &std::fs::read_to_string(&path).expect(&path),
    ));
    let mut e = Ephemeris::new(
        &system,
        EphemerisOptions {
            step_seconds: suggested_step_seconds(&system.bodies, 256.0),
            chunk_steps: 2048,
        },
    );
    e.extend_to(2.0 * T);
    e
}

fn index(e: &Ephemeris, id: &str) -> usize {
    e.bodies().iter().position(|b| b.id == id).expect(id)
}

fn terrain(radius_meters: f64) -> Arc<Terrain> {
    Arc::new(Terrain::from_config(&TerrainConfig::Layered(
        LayeredOptions {
            radius_meters,
            ..DEFAULT_LAYERED
        },
    )))
}

fn aurelia(e: &Ephemeris) -> (Environment, usize, Arc<Terrain>) {
    let b = index(e, "aurelia");
    let ground = terrain(e.bodies()[b].radius_meters);
    let env = Environment::new(e).with(
        b,
        BodyEnvironment {
            atmosphere: Some(Atmosphere::earth()),
            air_datum_meters: 0.0,
            terrain: Some(ground.clone()),
            sea_level_meters: Some(SEA_LEVEL),
        },
    );
    (env, b, ground)
}

/// The layered launch site, body-fixed.
fn site() -> DVec3 {
    let (latitude, longitude) = (0.3_f64, 0.5_f64);
    DVec3::new(
        latitude.cos() * longitude.cos(),
        latitude.cos() * longitude.sin(),
        latitude.sin(),
    )
}

fn panics(f: impl FnOnce()) -> bool {
    catch_unwind(AssertUnwindSafe(f)).is_err()
}

/// A scene-like frame on the surface, as a contact scene or a vessel's parts frame would be.
fn scene(frames: &mut SystemFrames, b: usize, at: DVec3) -> FrameId {
    let surface = frames.surface[b];
    frames.tree.add_fixed(
        surface,
        Motion::fixed(
            at,
            DQuat::from_axis_angle(DVec3::new(0.3, -0.4, 0.8).normalize(), 1.1),
        ),
    )
}

#[test]
fn gravity_is_one_vector_in_every_frame() {
    let e = sol();
    let (env, b, ground) = aurelia(&e);
    let mut frames = SystemFrames::new(&e);
    let pad = site() * (e.bodies()[b].radius_meters + ground.height(site()));
    let local = scene(&mut frames, b, pad);
    let moon = frames.inertial[index(&e, "selene")];
    let at = frames.tree.at(T, &e);
    let surface = frames.surface[b];
    let p = site() * (e.bodies()[b].radius_meters + ground.height(site()) + 2000.0);
    let want = env.gravity(&at, &frames, surface, p);
    assert!((want.length() - 9.8).abs() < 0.05, "{want}");
    // Frames that meet Aurelia's below the system keep the point to a few ulps of 6.4e6 m. The
    // Moon's and the origin meet it only at the barycentre: the point passes through 1 AU
    // coordinates (3e-5 m spacing), times Aurelia's gravity gradient (3e-6 /s²) ~ 1e-11 relative.
    for (name, frame, tolerance) in [
        ("scene", local, 1e-14),
        ("inertial", frames.inertial[b], 1e-14),
        ("moon", moon, 1e-11),
        ("origin", frames.origin, 1e-11),
    ] {
        let q = at.transform(surface, frame).apply_point(p);
        let g = at
            .transform(frame, surface)
            .apply_direction(env.gravity(&at, &frames, frame, q));
        let error = (g - want).length() / want.length();
        println!("{name}: {error:.1e}");
        assert!(error < tolerance, "{name}: {error}");
    }
}

#[test]
fn gravity_is_the_integrators_field() {
    let e = sol();
    let (env, b, _) = aurelia(&e);
    let frames = SystemFrames::new(&e);
    let centre = e.body_position(b, T);
    let mut propagator = VesselPropagator::new(
        &e,
        Tolerances {
            position_meters: 1e-3,
            velocity_meters_per_second: 1e-6,
        },
    );
    for offset in [
        DVec3::new(7.0e6, 0.0, 0.0),
        DVec3::new(-2.0e6, 6.6e6, 1.0e6),
        DVec3::new(1.0e5, 2.0e5, -4.0e7),
    ] {
        let x = centre + offset;
        let field = propagator.gravity_at(&e, T, x) + e.frame_acceleration_at(T);
        let at = frames.tree.at(T, &e);
        let g = env.gravity(&at, &frames, frames.origin, x);
        let error = (g - field).length() / field.length();
        println!("{offset}: {error:.1e}");
        assert!(error < 1e-12, "{offset}: {error}");
    }
}

#[test]
fn surroundings_are_read_in_the_body_frame() {
    let e = sol();
    let (env, b, ground) = aurelia(&e);
    let radius = e.bodies()[b].radius_meters;
    let mut frames = SystemFrames::new(&e);
    let height = ground.height(site());
    let local = scene(&mut frames, b, site() * (radius + height));
    let at = frames.tree.at(T, &e);
    let surface = frames.surface[b];
    let state = State {
        position: site() * (radius + height + 150.0),
        velocity: DVec3::new(30.0, -5.0, 2.0),
    };
    let atmosphere = Atmosphere::earth();
    // A few ulps of the 6.4e6 m radius below the system; 1 AU spacing (3e-5 m) through it.
    let near = 8.0 * radius * f64::EPSILON;
    for (name, frame, meters) in [
        ("surface", surface, near),
        ("scene", local, near),
        ("origin", frames.origin, 1e-4),
    ] {
        let there = at.transform(surface, frame).apply_state(state);
        let s = env.surroundings(&at, &frames, frame, there, b);
        assert_eq!(env.ground(&at, &frames, frame, there.position, b), s.ground);
        let turn = at.transform(frame, surface);
        let air = s.air.expect("air on the pad");
        let ground_sample = s.ground.expect("terrain");
        let sea = s.sea.expect("sea");
        let errors = [
            (s.radius - state.position.length()).abs(),
            (air.altitude - (height + 150.0)).abs(),
            (ground_sample.height - height).abs(),
            (ground_sample.clearance - 150.0).abs(),
            (sea.depth - (SEA_LEVEL - height - 150.0)).abs(),
            (turn.apply_direction(air.airspeed) - state.velocity).length(),
            (turn.apply_direction(s.up) - site()).length() * radius,
        ];
        println!("{name}: {:?}", errors.map(|x| format!("{x:.1e}")));
        for (i, error) in errors.into_iter().enumerate() {
            assert!(error < meters, "{name} [{i}]: {error}");
        }
        assert_eq!(air.air, atmosphere.sample(air.altitude), "{name}");
    }
}

#[test]
fn absent_air_ground_and_sea_are_none() {
    let e = sol();
    let (env, b, _) = aurelia(&e);
    let frames = SystemFrames::new(&e);
    let at = frames.tree.at(T, &e);
    let surface = frames.surface[b];
    let radius = e.bodies()[b].radius_meters;
    let high = State {
        position: DVec3::X * (radius + 120_000.0),
        velocity: DVec3::ZERO,
    };
    let s = env.surroundings(&at, &frames, surface, high, b);
    assert!(s.air.is_none() && s.ground.is_some() && s.sea.is_some());
    let just_below = State {
        position: DVec3::X * (radius + 119_999.0),
        ..high
    };
    assert!(
        env.surroundings(&at, &frames, surface, just_below, b)
            .air
            .is_some()
    );
    let moon = index(&e, "selene");
    let s = env.surroundings(
        &at,
        &frames,
        frames.surface[moon],
        State {
            position: DVec3::X * (e.bodies()[moon].radius_meters + 10.0),
            velocity: DVec3::ZERO,
        },
        moon,
    );
    assert!(s.air.is_none() && s.ground.is_none() && s.sea.is_none());
    assert!(env.body(moon).is_none() && env.body(b).is_some());
}

#[test]
fn invalid_queries_and_descriptions_panic() {
    let e = sol();
    let (env, b, _) = aurelia(&e);
    let frames = SystemFrames::new(&e);
    let at = frames.tree.at(T, &e);
    let (inertial, surface) = (frames.inertial[b], frames.surface[b]);
    let radius = e.bodies()[b].radius_meters;
    let still = |position| State {
        position,
        velocity: DVec3::ZERO,
    };
    assert!(panics(|| {
        env.gravity(&at, &frames, inertial, DVec3::ZERO);
    }));
    assert!(panics(|| {
        env.surroundings(&at, &frames, surface, still(DVec3::ZERO), b);
    }));
    assert!(panics(|| {
        env.surroundings(&at, &frames, surface, still(DVec3::X * radius), 999);
    }));
    assert!(panics(|| {
        env.gravity(&at, &frames, surface, DVec3::NAN);
    }));
    // Below the atmosphere's −5 km domain.
    assert!(panics(|| {
        env.surroundings(
            &at,
            &frames,
            surface,
            still(DVec3::X * (radius - 6000.0)),
            b,
        );
    }));
    let place = || BodyEnvironment {
        atmosphere: None,
        air_datum_meters: 0.0,
        terrain: None,
        sea_level_meters: None,
    };
    assert!(panics(|| {
        env.clone().with(b, place());
    }));
    assert!(panics(|| {
        Environment::new(&e).with(
            b,
            BodyEnvironment {
                terrain: Some(terrain(radius + 1.0)),
                ..place()
            },
        );
    }));
    assert!(panics(|| {
        Environment::new(&e).with(
            b,
            BodyEnvironment {
                air_datum_meters: f64::INFINITY,
                ..place()
            },
        );
    }));
}
