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
    let planet = void_landing::aurelia();
    let (ephemeris, home) = void_landing::planet_ephemeris(&planet);
    let environment = void_landing::planet_environment(&planet, &ephemeris, home, true);
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
    let airless = void_landing::planet_environment(&planet, &ephemeris, home, false);
    assert!(!has_atmosphere(&airless));
    assert!(vessel_air(&airless, &graph, &ids, DVec3::ZERO, DQuat::IDENTITY).is_none());
    assert!(Conditions::at(&airless, &ephemeris, 0.0, low).air.is_none());
}
