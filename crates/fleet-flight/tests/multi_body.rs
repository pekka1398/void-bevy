use glam::DVec3;
use void_fleet_flight::{
    session::{Action, FlightSession, InitialWorld, world_mark},
    world::aurelia_selene,
};
use void_vessels::{VesselMode, flat_site, pod_tank};
fn initial() -> InitialWorld {
    let planet = void_landing::aurelia();
    let site = flat_site(&planet);
    InitialWorld {
        air_dynamics: void_vessels::AirDynamics::ForceOnly,
        world: aurelia_selene(&planet),
        launch_body: "aurelia".into(),
        craft: pod_tank("Two bodies"),
        launch_site: site,
    }
}
#[test]
fn two_ground_owners_restore_and_replay_without_observation_dependency() {
    let initial = initial();
    let moon = initial.world.landing_planet("selene");
    let mut session = FlightSession::new(initial.clone()).with_recording();
    let outcome = session.execute(Action::LaunchGroundAt {
        body: "selene".into(),
        craft: initial.craft.clone(),
        site: flat_site(&moon),
    });
    let void_fleet_flight::session::Outcome::Spawned(id) = outcome else {
        panic!("spawn")
    };
    for _ in 0..60 {
        session.execute(Action::Advance {
            seconds: 0.5,
            rails: false,
        });
    }
    for vessel in session.sim().fleet.vessel_ids() {
        assert_eq!(
            session.sim().fleet.snapshot(&vessel).mode,
            VesselMode::Ground
        );
    }
    assert_eq!(session.sim().nearby_body("v1"), session.sim().home);
    assert_eq!(
        session.sim().nearby_body(&id),
        initial.world.body_index("selene")
    );
    assert_eq!(session.sim().fleet.ground_count(), 2);
    assert!(
        session.sim().fleet.rails_blocker().is_none(),
        "both ground owners should sleep: {:?}; scenes {:?}; Earth local {:?}; Moon local {:?}",
        session.sim().fleet.rails_blocker(),
        session.sim().fleet.scene_snapshots(),
        session
            .sim()
            .fleet
            .body_fixed_state("v1", session.sim().home),
        session
            .sim()
            .fleet
            .body_fixed_state(&id, initial.world.body_index("selene"))
    );
    session.execute(Action::Advance {
        seconds: 86_400.0,
        rails: true,
    });
    assert_eq!(session.sim().fleet.ground_count(), 2);

    let checkpoint =
        void_fleet_flight::checkpoint::FlightCheckpoint::capture(session.sim(), initial);
    let mut restored = checkpoint.restore();
    assert_eq!(world_mark(&restored), world_mark(session.sim()));
    restored.select(&id);
    restored.advance(0.5, false).unwrap();
    let replayed = FlightSession::from_recording(session.recording());
    assert_eq!(world_mark(replayed.sim()), world_mark(session.sim()));
}
#[test]
fn terrain_identity_and_airless_moon_are_explicit() {
    let initial = initial();
    let built = initial.world.build();
    let moon = initial.world.body_index("selene");
    assert!(built.environment.body(moon).unwrap().atmosphere.is_none());
    assert!(
        built
            .environment
            .body(moon)
            .unwrap()
            .sea_level_meters
            .is_none()
    );
    assert_ne!(
        built.terrains[&moon].radius_meters,
        built.terrains[&initial.world.body_index("aurelia")].radius_meters
    );
    let again = initial.world.build();
    for (&body, terrain) in &built.terrains {
        for d in [DVec3::X, DVec3::Y, DVec3::Z] {
            assert_eq!(
                terrain.sample(d, None),
                again.terrains[&body].sample(d, None)
            );
        }
    }
}
#[test]
#[should_panic(expected = "terrain radius mismatch")]
fn mismatched_terrain_is_rejected() {
    let mut initial = initial();
    initial.world.bodies.get_mut("selene").unwrap().terrain =
        initial.world.bodies["aurelia"].terrain.clone();
    initial.world.build();
}
#[test]
fn lunar_approach_crosses_contact_band_and_restores_midflight() {
    let initial = initial();
    let mut session = FlightSession::new(initial.clone());
    let moon = initial.world.body_index("selene");
    let terrain = session.sim().terrains[&moon].clone();
    let position = DVec3::X * (terrain.radius_meters + terrain.height(DVec3::X) + 2000.0);
    let void_fleet_flight::session::Outcome::Spawned(id) =
        session.execute(Action::LaunchFlightAt {
            body: "selene".into(),
            craft: initial.craft.clone(),
            position,
            velocity: -DVec3::X * 20.0,
        })
    else {
        panic!("spawn")
    };
    session.execute(Action::Select { vessel: id.clone() });
    assert_eq!(session.sim().mode(), VesselMode::Orbit);
    let checkpoint =
        void_fleet_flight::checkpoint::FlightCheckpoint::capture(session.sim(), initial);
    let mut restored = FlightSession::from_checkpoint(checkpoint);
    let mut entered = false;
    for _ in 0..100 {
        let action = Action::Advance {
            seconds: 1.0,
            rails: false,
        };
        assert_eq!(session.execute(action.clone()), restored.execute(action));
        assert_eq!(world_mark(session.sim()), world_mark(restored.sim()));
        if session.sim().mode() == VesselMode::Ground {
            entered = true;
            break;
        }
    }
    assert!(
        entered,
        "actual time integration must enter lunar contact band"
    );
    assert_eq!(session.sim().nearby_body(&id), moon);
    assert_eq!(
        session.sim().fleet.snapshot("v1").mode,
        VesselMode::Ground,
        "unobserved mother-planet vessel persists"
    );
}
#[test]
fn flight_traverses_between_two_bodies_by_time_integration() {
    let mut initial = initial();
    // Small binary accelerates the acceptance run without teleporting after launch. Both bodies
    // and their motion remain the same production N-body ephemeris and Fleet owner machinery.
    let template = void_landing::pebble();
    initial.world = void_fleet_flight::world::WorldDescription::single(&template, false);
    initial.world.system.root.mass_kg = 1e10;
    initial.world.system.root.radius_meters = 100.0;
    initial.world.system.root.rotation = void_orbit::RotationSpec::Spin(void_orbit::SpinSpec {
        period_seconds: 1e6,
        obliquity_radians: 0.0,
        pole_longitude_radians: 0.0,
        angle_at_epoch_radians: 0.0,
    });
    initial.world.system.root.children.clear();
    let mut moon = initial.world.system.root.clone();
    moon.id = "companion".into();
    moon.mass_kg = 1e8;
    moon.orbit = Some(void_orbit::EllipticElements {
        semi_major_axis_meters: 10_000.0,
        eccentricity: 0.0,
        inclination_radians: 0.0,
        longitude_of_ascending_node_radians: 0.0,
        argument_of_periapsis_radians: 0.0,
        mean_anomaly_radians: 0.0,
    });
    moon.orbit_plane = Some(void_orbit::OrbitPlane::Ecliptic);
    initial.world.system.root.children.push(moon);
    let mut descriptor = initial.world.bodies["pebble"].clone();
    descriptor.terrain = Some(void_terrain::TerrainConfig::Hills(
        void_terrain::HillsOptions {
            name: "Transfer terrain".into(),
            radius_meters: 100.0,
            max_height_meters: 1.0,
            wavelength_meters: 100.0,
            octaves: 1,
        },
    ));
    initial
        .world
        .bodies
        .insert("pebble".into(), descriptor.clone());
    initial.world.bodies.insert("companion".into(), descriptor);
    initial.launch_body = "pebble".into();
    initial.launch_site = DVec3::X;
    let mut sim = initial.build();
    let target = sim.world.body_index("companion");
    let to_moon = sim
        .fleet
        .frames()
        .transform(
            sim.fleet.body_frames(target).0,
            sim.fleet.body_frames(sim.home).1,
        )
        .apply_point(DVec3::ZERO)
        .normalize();
    let id = sim.launch_flight_at(
        "pebble",
        &initial.craft,
        void_landing::FrameState {
            position: to_moon * 600.0,
            velocity: to_moon * 10.0,
        },
    );
    sim.select(&id);
    assert_eq!(sim.nearby_body(&id), sim.home);
    let mut crossed = false;
    for _ in 0..1100 {
        sim.advance(1.0, false).unwrap();
        if sim.mode() == VesselMode::Ground && sim.nearby_body(&id) == target {
            crossed = true;
            break;
        }
    }
    assert!(
        crossed,
        "time integration must carry departure vessel into companion contact band"
    );
}
#[test]
fn drawn_sampler_matches_each_bodys_actual_collision_vertices() {
    let initial = initial();
    let mut sim = initial.build();
    let moon = initial.world.landing_planet("selene");
    let moon_id = sim.launch_ground_at("selene", &initial.craft, flat_site(&moon));
    sim.advance(0.1, false).unwrap();
    let mut measured = std::collections::BTreeSet::new();
    for tile in sim.fleet.terrain_tiles() {
        let member = sim
            .fleet
            .scene_snapshots()
            .iter()
            .find(|s| s.id == tile.scene)
            .unwrap()
            .members[0]
            .clone();
        let body = sim.nearby_body(&member);
        measured.insert(body);
        let (vertices, _) = sim.fleet.terrain_geometry(tile.scene, &tile.tile);
        let into = sim
            .fleet
            .frames()
            .transform(tile.frame, sim.fleet.body_frames(body).1);
        for v in vertices.iter().step_by(64) {
            let p =
                into.apply_point(tile.local_position + glam::DVec3::from_array(v.map(f64::from)));
            let terrain = &sim.terrains[&body];
            let difference =
                (p.length() - terrain.radius_meters - terrain.height(p.normalize())).abs();
            assert!(
                difference < 1e-3,
                "collision/display terrain mismatch {difference} m on body {body}"
            );
        }
    }
    assert_eq!(measured.len(), 2);
    assert_eq!(
        sim.nearby_body(&moon_id),
        initial.world.body_index("selene")
    );
}
#[test]
fn duplicate_world_body_ids_are_rejected_by_parser() {
    let i = initial();
    let world = serde_json::to_value(i.world).unwrap();
    let descriptor = serde_json::to_string(&world["bodies"]["aurelia"]).unwrap();
    let json = format!(
        "{{\"schema\":1,\"system\":{},\"bodies\":{{\"aurelia\":{descriptor},\"aurelia\":{descriptor}}}}}",
        world["system"]
    );
    assert!(
        serde_json::from_str::<void_fleet_flight::world::WorldDescription>(&json)
            .unwrap_err()
            .to_string()
            .contains("duplicate body ID")
    );
}

#[test]
fn air_switch_does_not_manufacture_air_on_an_airless_preset() {
    for planet in [void_landing::pebble(), void_landing::moon_size()] {
        let craft = pod_tank("Airless compatibility");
        let initial = InitialWorld::new(&planet, &craft, flat_site(&planet), true);
        let sim = initial.build();
        assert!(
            sim.fleet
                .environment()
                .body(sim.home)
                .unwrap()
                .atmosphere
                .is_none()
        );
        assert!(!initial.world.bodies[&planet.body_id].visual.atmosphere);
    }
}

#[test]
fn descriptor_datum_and_sea_survive_legacy_projection_and_checkpoint() {
    let mut initial = initial();
    let description = initial.world.bodies.get_mut("aurelia").unwrap();
    description.air_datum_meters = 1234.0;
    description.sea_level_meters = Some(567.0);
    let projected = initial.planet();
    assert_eq!(projected.air_datum_meters(), 1234.0);
    assert_eq!(projected.sea_level, Some(567.0));
    let resingle = void_fleet_flight::world::WorldDescription::single(&projected, true);
    assert_eq!(resingle.bodies["aurelia"].air_datum_meters, 1234.0);
    assert_eq!(resingle.bodies["aurelia"].sea_level_meters, Some(567.0));
    let sim = initial.build();
    let saved = void_fleet_flight::checkpoint::FlightCheckpoint::capture(&sim, initial);
    let restored = saved.restore();
    let physical = restored.fleet.environment().body(restored.home).unwrap();
    assert_eq!(
        physical.air_datum_meters,
        restored.planet.air_datum_meters()
    );
    assert_eq!(physical.sea_level_meters, restored.planet.sea_level);
    assert_eq!(world_mark(&sim), world_mark(&restored));
}

#[test]
fn refocusing_a_ship_on_another_body_places_the_camera_above_its_local_horizon() {
    use void_fleet_flight::presentation::ViewCommand;
    let mut session = FlightSession::new(initial()).with_recording();
    let moon = session.sim().world.body_index("selene");
    session.execute(Action::LaunchGroundAt {
        body: "selene".into(),
        craft: session.recording_initial().craft.clone(),
        site: void_vessels::flat_site(&session.sim().world.landing_planet("selene")),
    });
    session.execute(Action::Select {
        vessel: "v2".into(),
    });
    session.execute(Action::View {
        command: ViewCommand::Focus { body: None },
    });
    let sim = session.sim();
    let fleet = &sim.fleet;
    let surface = fleet.body_frames(moon).1;
    let frames = fleet.frames();
    let up = frames
        .transform(fleet.vessel_frame(&sim.selected), surface)
        .apply_point(fleet.root_position_local(&sim.selected))
        .normalize();
    let camera_direction = frames
        .transform(fleet.origin_frame(), surface)
        .apply_direction(sim.presentation.direction);
    assert!(camera_direction.dot(up) > 0.25);
    session.mark();
    let replay = FlightSession::from_recording(session.recording());
    assert_eq!(world_mark(session.sim()), world_mark(replay.sim()));
}

#[test]
fn datums_cannot_collapse_the_atmosphere_or_ocean_sphere() {
    for sea in [false, true] {
        let mut config = initial();
        let radius = config.planet().terrain.radius_meters;
        let body = config.world.bodies.get_mut("aurelia").unwrap();
        if sea {
            body.sea_level_meters = Some(-radius);
            body.visual.ocean = false;
        } else {
            body.air_datum_meters = -radius;
        }
        assert!(std::panic::catch_unwind(|| config.build()).is_err());
    }
}
