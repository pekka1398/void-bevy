use glam::{DQuat, DVec3};
use void_frames::{Motion, State};
#[test]
fn water_trial_frames_purity_and_spin_dissipation() {
    let mut planet = void_testkit::earth_size();
    planet.sea_level = Some(9000.);
    let (e, home) = void_testkit::planet_ephemeris(&planet);
    let env = void_testkit::planet_environment(&planet, &e, home, true);
    let mut graph = void_assembly::PartGraph::new();
    let ids = graph.add(
        &void_assembly::compile(&void_assembly::fresh_craft()).unwrap(),
        "water",
    );
    let before = graph
        .parts()
        .map(|p| format!("{:?}", p))
        .collect::<Vec<_>>();
    let source = void_modules::water::VesselWater::new(&env, &graph, &ids, DVec3::ZERO);
    let mut frames = env.frames().clone();
    let surface = frames.surface[home];
    let scene = frames.tree.add_fixed(
        surface,
        Motion::fixed(
            DVec3::X * (planet.terrain.radius_meters + 9000.),
            DQuat::from_rotation_z(0.4),
        ),
    );
    let at = frames.tree.at(0., &e);
    let state = State {
        position: DVec3::X * (planet.terrain.radius_meters + 9000. - 2.),
        velocity: DVec3::new(0., 3., 4.),
    };
    let q = DQuat::from_rotation_z(0.2);
    let spin = DVec3::X * 2.;
    let to_origin = at.transform(surface, frames.origin);
    assert!(source.near_surface(&e, 0., to_origin.apply_state(state), 1., 0.));
    let far = State {
        position: DVec3::X * (planet.terrain.radius_meters + 100_000.),
        velocity: DVec3::ZERO,
    };
    assert!(!source.near_surface(&e, 0., to_origin.apply_state(far), 1., 0.));
    let incoming = State {
        position: DVec3::X * (planet.terrain.radius_meters + 9050.),
        velocity: -DVec3::X * 100.,
    };
    assert!(source.near_surface(&e, 0., to_origin.apply_state(incoming), 1., 0.));
    let load = source.wrench_in(&at, surface, state, q, spin);
    let still = source.wrench_in(
        &at,
        surface,
        State {
            velocity: DVec3::ZERO,
            ..state
        },
        q,
        DVec3::ZERO,
    );
    assert!((load.force - still.force).dot(state.velocity) + load.torque.dot(spin) < 0.);
    let to = at.transform(surface, scene);
    let other = source
        .wrench_in(
            &at,
            scene,
            to.apply_state(state),
            to.rotation() * q,
            to.rotation() * spin,
        )
        .in_frame(&at, surface);
    assert!((other.force - load.force).length() < 1e-5);
    assert!((other.torque - load.torque).length() < 1e-5);
    assert_eq!(
        before,
        graph
            .parts()
            .map(|p| format!("{:?}", p))
            .collect::<Vec<_>>()
    );
}
#[test]
fn hydrostatic_projection_uses_shared_j2_body_gravity() {
    let mut planet = void_testkit::earth_size();
    planet.sea_level = Some(9000.);
    planet.system.root.gravity_field = Some(void_orbit::GravityField {
        j2: 0.03,
        reference_radius_meters: planet.terrain.radius_meters,
    });
    let (e, home) = void_testkit::planet_ephemeris(&planet);
    let env = void_testkit::planet_environment(&planet, &e, home, false);
    let mut graph = void_assembly::PartGraph::new();
    let ids = graph.add(
        &void_assembly::compile(&void_assembly::fresh_craft()).unwrap(),
        "j2",
    );
    let source = void_modules::water::VesselWater::new(&env, &graph, &ids, DVec3::ZERO);
    let frames = env.frames();
    let at = frames.tree.at(0., &e);
    let state = State {
        position: DVec3::X * (planet.terrain.radius_meters + 8995.),
        velocity: DVec3::ZERO,
    };
    let load = source.wrench_in(
        &at,
        frames.surface[home],
        state,
        DQuat::IDENTITY,
        DVec3::ZERO,
    );
    let body = &e.bodies()[home];
    let g = -void_orbit::gravity::pull(
        body.gm,
        void_orbit::gravity::oblateness(body),
        DVec3::Z,
        state.position,
    )
    .dot(DVec3::X)
        - body.rotation.rate().powi(2) * state.position.length();
    let definition = graph.part(&ids[0]).definition;
    let volume = void_modules::water::displacement(definition, DVec3::X, 5.).volume;
    assert!((load.force.x - 1000. * volume * g).abs() < 1e-8);
    assert!(
        (g - (body.gm / state.position.length_squared()
            - body.rotation.rate().powi(2) * state.position.length()))
        .abs()
            > 0.1
    );
}
