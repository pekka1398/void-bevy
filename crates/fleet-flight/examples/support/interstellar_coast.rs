//! Explicit acceptance starting state; no launch or completed transfer is implied.
use glam::{DQuat, DVec3};
use void_fleet_flight::{FleetFlight, session::InitialWorld, world::stellar_neighborhood};
use void_frames::{SplitPosition, SystemId};
use void_vessels::{Fleet, FleetOptions};

pub fn fixture() -> (FleetFlight, InitialWorld) {
    let planet = void_landing::aurelia();
    let craft = void_assembly::rcs_flight_rocket();
    let mut initial = InitialWorld::new(&planet, &craft, void_vessels::flat_site(&planet), true)
        .with_air_dynamics(void_vessels::AirDynamics::ForceAndTorque);
    initial.world = stellar_neighborhood(&planet);
    initial.launch_body = "Sol/aurelia".into();
    let mut sim = initial.build();
    let built = sim.world.build();
    // Replace the initial fleet with the declared cruise-only fleet. The world,
    // environment and authoritative part representation remain the usual ones.
    sim.fleet = Fleet::new(
        built.ephemeris,
        built.environment,
        0.0,
        built.grounds,
        FleetOptions {
            air_dynamics: initial.air_dynamics,
            ..Default::default()
        },
    );
    sim.coupled_world = built.coupled_world;
    sim.selected = sim.fleet.launch_at_split(
        &craft,
        SystemId(0),
        SplitPosition::at(DVec3::new(2.0 * void_multiscale::LIGHT_YEAR, 0.0, 0.0)),
        DVec3::new(1_000_000.0, 0.0, 0.0),
        DQuat::IDENTITY,
        DVec3::ZERO,
    );
    sim.presentation.paused = true;
    sim.presentation.speed_surface = false;
    sim.presentation.altitude_agl = false;
    (sim, initial)
}
