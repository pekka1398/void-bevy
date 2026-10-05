use glam::DVec3;
use void_fleet_flight::{
    presentation::ViewCommand,
    session::{Action, FlightSession, InitialWorld, world_mark},
    world::{WorldDescription, solar_scenery},
};
fn initial() -> InitialWorld {
    let planet = void_landing::aurelia();
    InitialWorld {
        world: solar_scenery(&planet),
        air_dynamics: void_vessels::AirDynamics::ForceAndTorque,
        launch_body: "aurelia".into(),
        craft: void_vessels::pod_tank("Scenery witness"),
        launch_site: void_vessels::flat_site(&planet),
    }
}
#[test]
fn recipes_roundtrip_and_never_create_gas_ground_or_physical_optics() {
    let initial = initial();
    let json = serde_json::to_string(&initial).unwrap();
    let restored: InitialWorld = serde_json::from_str(&json).unwrap();
    assert_eq!(
        serde_json::to_value(initial).unwrap(),
        serde_json::to_value(&restored).unwrap()
    );
    let built = restored.world.build();
    assert_eq!(restored.world.bodies.len(), 10);
    assert_eq!(built.grounds.len(), 5);
    for id in ["sol", "velvet", "halo", "azure", "abyss"] {
        let body = restored.world.body_index(id);
        assert!(!built.terrains.contains_key(&body));
        assert!(built.environment.body(body).unwrap().terrain.is_none());
    }
    for id in ["vesper", "ares"] {
        assert!(restored.world.bodies[id].visual.scattering.is_some());
        assert!(
            built
                .environment
                .body(restored.world.body_index(id))
                .unwrap()
                .atmosphere
                .is_none()
        );
    }
    let mut old: WorldDescription = restored.world;
    old.schema = 2;
    assert!(std::panic::catch_unwind(|| old.build()).is_err());
}
#[test]
fn all_body_presets_preserve_physics_and_checkpoint() {
    let initial = initial();
    let mut session = FlightSession::new(initial.clone()).with_recording();
    let ids: Vec<_> = initial.world.bodies.keys().cloned().collect();
    let before = session.sim().fleet.snapshot(&session.sim().selected);
    for id in ids {
        let body = initial.world.body_index(&id);
        let radius = session.sim().fleet.ephemeris.bodies()[body].radius_meters;
        for scale in [1.08, 3.5, 12.0] {
            session.execute(Action::View {
                command: ViewCommand::BodyPreset {
                    body,
                    direction: DVec3::new(1.0, 0.2, 0.3).normalize(),
                    distance: radius * scale,
                },
            });
            session.execute(Action::View {
                command: ViewCommand::Exposure {
                    value: (scale as f32) / 10.0,
                },
            });
            let sample = session.sim().presentation.sample(session.sim());
            assert!(sample.eye.is_finite() && sample.offset.is_finite());
            let after = session.sim().fleet.snapshot(&session.sim().selected);
            assert_eq!(before.position, after.position);
            assert_eq!(before.velocity, after.velocity);
        }
    }
    let restored = FlightSession::from_recording(session.recording());
    assert_eq!(world_mark(session.sim()), world_mark(restored.sim()));
    let checkpoint =
        void_fleet_flight::checkpoint::FlightCheckpoint::capture(session.sim(), initial);
    let restored = checkpoint.restore();
    assert_eq!(world_mark(session.sim()), world_mark(&restored));
}
#[test]
fn cratered_collision_vertices_match_renderer_sampler() {
    let initial = initial();
    let mut sim = initial.build();
    for id in ["selene", "cinder", "ares", "vesper"] {
        sim.launch_ground_at(id, &initial.craft, DVec3::X);
    }
    sim.advance(0.02, false).unwrap();
    let mut measured = std::collections::BTreeSet::new();
    for tile in sim.fleet.terrain_tiles() {
        let scenes = sim.fleet.scene_snapshots();
        let member = &scenes.iter().find(|s| s.id == tile.scene).unwrap().members[0];
        let body = sim.nearby_body(member);
        measured.insert(body);
        let (vertices, _) = sim.fleet.terrain_geometry(tile.scene, &tile.tile);
        let into = sim
            .fleet
            .frames()
            .transform(tile.frame, sim.fleet.body_frames(body).1);
        let terrain = &sim.terrains[&body];
        for v in vertices.iter().step_by(64) {
            let p = into.apply_point(tile.local_position + DVec3::from_array(v.map(f64::from)));
            assert!(
                (p.length() - terrain.radius_meters - terrain.height(p.normalize())).abs() < 1e-3
            );
        }
    }
    assert_eq!(measured.len(), 5);
}

#[test]
fn authored_luts_and_vacuum_extreme_optics_are_finite() {
    use void_scenery::{
        atmosphere::build_transmittance_table, atmosphere_scene::AtmosphereProfile, tables::*,
    };
    let initial = initial();
    let system = void_orbit::build_system(&initial.world.system);
    for (id, description) in &initial.world.bodies {
        if let Some(profile) = &description.visual.scattering {
            let radius = system
                .bodies
                .iter()
                .find(|b| &b.id == id)
                .unwrap()
                .radius_meters;
            let params = profile.parameters(radius + description.air_datum_meters);
            let trans = build_transmittance_table(&params);
            let multiple = build_multiple_scattering_table(&params, &trans, 16, 8);
            let irradiance = build_irradiance_table(&params, &trans, &multiple, 16, 8);
            assert!(
                trans
                    .iter()
                    .chain(&multiple)
                    .chain(&irradiance)
                    .all(|v| v.is_finite()),
                "nonfinite {id}"
            );
        }
    }
    for coefficient in [0.0, 1.0] {
        let profile = AtmosphereProfile::Custom {
            height_meters: 120000.0,
            rayleigh_scattering: [coefficient; 3],
            rayleigh_scale_height: 16000.0,
            mie_scattering: coefficient,
            mie_extinction: coefficient,
            mie_scale_height: 18000.0,
            mie_anisotropy: 0.99,
            ozone_absorption: [0.0; 3],
            ozone_center_height: 0.0,
            ozone_width: 1.0,
        };
        let params = profile.parameters(6e6);
        let trans = build_transmittance_table(&params);
        assert!(trans.iter().all(|v| v.is_finite()));
        if coefficient == 0.0 {
            assert!(trans.as_chunks::<4>().0.iter().all(|v| v[..3] == [1.0; 3]));
        }
    }
}
