//! The modules' answers: engines lose thrust to back pressure, a part's body exposes only what no
//! neighbour covers, and a vessel feels air only inside an atmosphere.
use glam::{DQuat, DVec3};
use std::f64::consts::PI;
use void_aero::{AeroElement, AeroShape};
use void_assembly::{PartGraph, compile, flight_rocket};
use void_environment::{Air, AirSample, Atmosphere};
use void_frames::{BodyId, BodyStates, State};
use void_modules::{Conditions, body, engine, has_atmosphere, vessel_air};
use void_orbit::AirSource;

fn rocket() -> (PartGraph, Vec<String>) {
    let mut graph = PartGraph::new();
    let ids = graph.add(&compile(&flight_rocket()).unwrap(), "v1");
    (graph, ids)
}

fn in_air(air: Air) -> Conditions {
    Conditions {
        air: Some(AirSample {
            altitude: 0.0,
            air,
            airspeed: DVec3::ZERO,
        }),
    }
}

#[test]
fn engines_lose_thrust_to_back_pressure() {
    let (graph, ids) = rocket();
    let sea = Atmosphere::earth().sample(0.0);
    let mut kept = vec![];
    for id in ids.iter().filter(|id| graph.part(id).engine().is_some()) {
        let part = graph.part(id);
        let rating = part.engine().unwrap();
        let vacuum = engine::thrust(part, 1.0, &Conditions::VACUUM);
        assert_eq!(
            vacuum.force,
            part.pose.rotation * rating.direction * rating.thrust_newtons
        );
        assert_eq!(vacuum.point, part.pose.position);
        let half = engine::thrust(part, 0.5, &Conditions::VACUUM);
        assert_eq!(half.force * 2.0, vacuum.force);
        assert_eq!(half.flow_kg_per_second * 2.0, vacuum.flow_kg_per_second);
        let low = engine::thrust(part, 1.0, &in_air(sea));
        let fraction = low.force.length() / vacuum.force.length();
        let expected = 1.0 - rating.nozzle_exit_area_m2 * sea.pressure_pa / rating.thrust_newtons;
        assert!((fraction - expected).abs() < 1e-12, "{id}: {fraction}");
        assert!(low.force.normalize().dot(vacuum.force.normalize()) > 1.0 - 1e-15);
        assert_eq!(
            low.flow_kg_per_second, vacuum.flow_kg_per_second,
            "{id}: the flow stays the vacuum rating's"
        );
        // Ten atmospheres overexpand every nozzle: the thrust stops at zero, the flow does not.
        let crushed = engine::thrust(
            part,
            1.0,
            &in_air(Air {
                pressure_pa: 10.0 * sea.pressure_pa,
                ..sea
            }),
        );
        assert_eq!(crushed.force, DVec3::ZERO);
        assert_eq!(crushed.flow_kg_per_second, vacuum.flow_kg_per_second);
        kept.push((part.definition.id.as_str(), fraction));
    }
    // The booster keeps 89.9 % at sea level; the upper stage's vacuum nozzle only 24 %.
    kept.sort_by(|a, b| a.0.cmp(b.0));
    assert_eq!(kept.len(), 2);
    assert_eq!(kept[0].0, "flight-booster-engine");
    assert!((kept[0].1 - 0.899).abs() < 5e-4, "{kept:?}");
    assert_eq!(kept[1].0, "flight-upper-engine");
    assert!((kept[1].1 - 0.240).abs() < 5e-4, "{kept:?}");
}

fn ends(e: &AeroElement) -> (f64, f64) {
    let AeroShape::Body(b) = &e.shape else {
        panic!("{} is not a body", e.id)
    };
    (b.front_area, b.rear_area)
}

#[test]
fn a_part_body_exposes_what_no_neighbour_covers() {
    let (graph, ids) = rocket();
    let centre = DVec3::new(0.25, -3.0, 0.5);
    for id in &ids {
        let part = graph.part(id);
        let e = body::element(&graph, &ids, id, centre);
        assert_eq!(e.point, part.pose.position - centre);
        let AeroShape::Body(b) = &e.shape else {
            unreachable!()
        };
        assert_eq!(b.axis, part.pose.rotation * DVec3::Y);
        // Alone, both ends are open.
        let alone = body::element(&graph, std::slice::from_ref(id), id, centre);
        let full = PI * part.definition.radius.powi(2);
        assert_eq!(ends(&alone), (full, full), "{id}");
    }
    // In the stack, an end is open only beyond its neighbour's radius.
    let mut covered = 0;
    for c in graph.connections() {
        for (part, node, other) in [(&c.a, &c.node_a, &c.b), (&c.b, &c.node_b, &c.a)] {
            let (front, rear) = ends(&body::element(&graph, &ids, part, DVec3::ZERO));
            let area = match node.as_str() {
                "top" => front,
                "bottom" => rear,
                _ => continue,
            };
            let (r, o) = (
                graph.part(part).definition.radius,
                graph.part(other).definition.radius,
            );
            assert_eq!(area, PI * (r * r - o * o).max(0.0), "{part} {node}");
            covered += 1;
        }
    }
    assert!(covered >= 6, "{covered}");
    // A separated stage covers nothing: the pod alone with its neighbour outside `members`.
    let pod = &ids[0];
    let (_, rear) = ends(&body::element(
        &graph,
        std::slice::from_ref(pod),
        pod,
        DVec3::ZERO,
    ));
    assert_eq!(rear, PI * graph.part(pod).definition.radius.powi(2));
}

#[test]
fn a_vessel_feels_air_only_inside_an_atmosphere() {
    let planet = void_testkit::aurelia();
    let (ephemeris, home) = void_testkit::planet_ephemeris(&planet);
    let environment = void_testkit::planet_environment(&planet, &ephemeris, home, true);
    assert!(has_atmosphere(&environment));
    let (graph, ids) = rocket();
    let air = vessel_air(&environment, &graph, &ids, DVec3::ZERO, DQuat::IDENTITY)
        .expect("Aurelia has air");
    assert_eq!(air.elements().len(), ids.len());
    let datum = environment.body(home).unwrap().air_datum_meters;
    let (centre, velocity) = ephemeris.body_state(BodyId(home), 0.0);
    let sea = planet.terrain.radius_meters + datum;
    let mass = graph.mass(&ids);
    // A kilometre above the sea, climbing at 300 m/s through air that turns with the planet: the
    // drag takes energy out of the flow. It is not antiparallel to the airspeed, since the body
    // flies at an angle of attack and its side and end coefficients differ.
    let low = State {
        position: centre + DVec3::X * (sea + 1000.0),
        velocity: velocity + DVec3::X * 300.0,
    };
    let sample = Conditions::at(&environment, &ephemeris, 0.0, low)
        .air
        .expect("air a kilometre above the sea");
    assert!(
        (sample.altitude - 1000.0).abs() < 1e-3,
        "{}",
        sample.altitude
    );
    let drag = air.acceleration(&ephemeris, 0.0, low.position, low.velocity, mass);
    assert!(drag.length() > 1.0, "{drag}");
    assert!(
        drag.dot(sample.airspeed) < 0.0,
        "{drag} {}",
        sample.airspeed
    );
    // Above the ceiling there is no air and no drag.
    let high = State {
        position: centre + DVec3::X * (sea + 200e3),
        ..low
    };
    assert!(
        Conditions::at(&environment, &ephemeris, 0.0, high)
            .air
            .is_none()
    );
    assert_eq!(
        Conditions::at(&environment, &ephemeris, 0.0, high).ambient_pressure_pa(),
        0.0
    );
    assert_eq!(
        air.acceleration(&ephemeris, 0.0, high.position, high.velocity, mass),
        DVec3::ZERO
    );
    // A world without air: no vessel air, vacuum everywhere.
    let airless = void_testkit::planet_environment(&planet, &ephemeris, home, false);
    assert!(!has_atmosphere(&airless));
    assert!(vessel_air(&airless, &graph, &ids, DVec3::ZERO, DQuat::IDENTITY).is_none());
    assert!(Conditions::at(&airless, &ephemeris, 0.0, low).air.is_none());
}

#[test]
fn engines_reject_invalid_pressure_before_clamping_thrust() {
    let (graph, ids) = rocket();
    let part = ids
        .iter()
        .map(|id| graph.part(id))
        .find(|p| p.engine().is_some())
        .unwrap();
    let sea = Atmosphere::earth().sample(0.0);
    for pressure_pa in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
        let conditions = in_air(Air { pressure_pa, ..sea });
        let failure = std::panic::catch_unwind(|| engine::thrust(part, 1.0, &conditions))
            .expect_err("invalid pressure must not turn into an operating engine");
        let message = failure.downcast_ref::<String>().unwrap();
        assert!(message.contains("ambient pressure"), "{message}");
    }
}

#[test]
fn vessel_air_samples_the_current_body_in_a_multi_atmosphere_world() {
    use std::sync::Arc;
    use void_environment::BodyEnvironment;
    let planet = void_testkit::aurelia();
    let (ephemeris, home) = void_testkit::planet_ephemeris(&planet);
    let base = void_testkit::planet_environment(&planet, &ephemeris, home, true);
    let other = (0..base.bodies().len()).find(|&b| b != home).unwrap();
    let environment = Arc::new(base.as_ref().clone().with(
        other,
        BodyEnvironment {
            atmosphere: Some(Atmosphere::earth()),
            air_datum_meters: 250.0,
            terrain: None,
            sea_level_meters: None,
        },
    ));
    let (graph, ids) = rocket();
    let source = vessel_air(&environment, &graph, &ids, DVec3::ZERO, DQuat::IDENTITY).unwrap();
    let frames = environment.frames();
    let at = frames.tree.at(0.0, &ephemeris);
    for body in [home, other] {
        let (centre, velocity) = ephemeris.body_state(BodyId(body), 0.0);
        let radius = environment.bodies()[body].radius_meters;
        let datum = environment.body(body).unwrap().air_datum_meters;
        let state = State {
            position: centre + DVec3::X * (radius + datum + 3000.0),
            velocity: velocity + DVec3::X * 300.0,
        };
        let expected = environment
            .surroundings(&at, frames, frames.origin, state, body)
            .air
            .unwrap();
        let actual = Conditions::at(&environment, &ephemeris, 0.0, state)
            .air
            .unwrap();
        assert_eq!(actual, expected, "body {body}");
        let drag = source.acceleration(
            &ephemeris,
            0.0,
            state.position,
            state.velocity,
            graph.mass(&ids),
        );
        assert!(drag.dot(actual.airspeed) < 0.0, "body {body}: {drag}");
    }
}

#[test]
fn a_local_chute_feels_air_when_its_com_is_above_the_ceiling() {
    use void_assembly::{ModuleState, ParachutePhase, ParachuteState, PartPose, fresh_craft};
    let p = void_testkit::earth_size();
    let (e, home) = void_testkit::planet_ephemeris(&p);
    let env = void_testkit::planet_environment(&p, &e, home, true);
    let mut c = fresh_craft();
    c.parts[0].definition_id = "parachute-pod".into();
    let mut g = PartGraph::new();
    let ids = g.add(&compile(&c).unwrap(), "v1");
    g.set_pose(
        &ids[0],
        PartPose {
            position: -DVec3::X * 1000.0,
            rotation: DQuat::IDENTITY,
        },
    );
    // Restore a valid full state directly; live transitions cannot skip deployment.
    let mut part = g.part(&ids[0]).clone();
    part.id = "full".into();
    part.modules.insert(
        "parachute1".into(),
        ModuleState::Parachute {
            state: ParachuteState {
                phase: ParachutePhase::Full,
                elapsed_seconds: 0.0,
            },
        },
    );
    g.insert(part);
    let ids = vec!["full".into()];
    let frames = env.frames();
    let at = frames.tree.at(10.0, &e);
    let rotation = at.transform(frames.surface[home], frames.origin).rotation();
    let air = void_modules::vessel_air_at(&env, &g, &ids, DVec3::ZERO, rotation, 10.0).unwrap();
    let frame = void_landing::PlanetFrame::new(&e, home);
    let state = frame.to_inertial(
        &e,
        10.0,
        void_frames::State {
            position: DVec3::X * (p.terrain.radius_meters + 120500.0),
            velocity: DVec3::Y * 1000.0,
        },
    );
    assert!(
        Conditions::at(
            &env,
            &e,
            10.0,
            State {
                position: state.position,
                velocity: state.velocity
            }
        )
        .air
        .is_none()
    );
    let before = g.part("full").modules.clone();
    let force = air.acceleration(&e, 10.0, state.position, state.velocity, 300.0);
    assert!(force.length() > 0.0);
    let frames = env.frames();
    let at = frames.tree.at(10.0, &e);
    let framed = air.acceleration_in(
        &at,
        frames.origin,
        State {
            position: state.position,
            velocity: state.velocity,
        },
        300.0,
    );
    assert_eq!(
        framed, force,
        "scene sampling must retain the off-centre chute force"
    );
    assert_eq!(g.part("full").modules, before);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| air.acceleration(
            &e,
            9.0,
            state.position,
            state.velocity,
            300.0
        )))
        .is_err()
    );
}

#[test]
fn scene_air_at_rest_is_exactly_zero_at_a_distant_planet() {
    let planet = void_testkit::aurelia();
    let (mut ephemeris, home) = void_testkit::planet_ephemeris(&planet);
    let environment = void_testkit::planet_environment(&planet, &ephemeris, home, true);
    let (graph, ids) = rocket();
    let frames = environment.frames();
    let surface = frames.surface[home];
    let mass = graph.mass(&ids);
    let local = State {
        position: DVec3::X * (planet.terrain.radius_meters + 1000.0),
        velocity: DVec3::ZERO,
    };
    for t in [0.0, 1.0, 100.0] {
        ephemeris.extend_to(t);
        let at = frames.tree.at(t, &ephemeris);
        let rotation = at.transform(surface, frames.origin).rotation();
        let source = vessel_air(&environment, &graph, &ids, DVec3::ZERO, rotation).unwrap();
        assert_eq!(
            source.acceleration_in(&at, surface, local, mass),
            DVec3::ZERO
        );
        let moving = State {
            velocity: DVec3::X * 100.0,
            ..local
        };
        let drag = source.acceleration_in(&at, surface, moving, mass);
        assert!(drag.dot(moving.velocity) < 0.0);
    }
}
