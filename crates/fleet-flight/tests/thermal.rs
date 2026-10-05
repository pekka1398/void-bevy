use glam::{DQuat, DVec3};
use void_fleet_flight::session::{Action, FlightSession, InitialWorld, Outcome, world_mark};
#[test]
fn loss_of_command_from_heating_refuses_controls_and_replays_without_panicking() {
    // A deliberately extreme 20 km/s thermal stress, not a nominal low-orbit reentry.
    let planet = void_landing::earth_size();
    let mut craft = void_assembly::reentry_capsule();
    craft.parts.pop();
    let initial = InitialWorld::new(&planet, &craft, void_vessels::flat_site(&planet), true)
        .with_air_dynamics(void_vessels::AirDynamics::ForceAndTorque);
    let mut s = FlightSession::new(initial).with_recording();
    let sim = s.sim();
    let fleet = &sim.fleet;
    let transform = fleet
        .frames()
        .transform(fleet.body_frames(sim.home).1, fleet.origin_frame());
    let velocity = DVec3::new(-500., 20000., 0.);
    let state = transform.apply_state(void_frames::State {
        position: DVec3::X * (planet.terrain.radius_meters + 55000.),
        velocity,
    });
    let rotation = transform.rotation() * DQuat::from_rotation_arc(-DVec3::Y, velocity.normalize());
    let Outcome::Spawned(id) = s.execute(Action::LaunchState {
        craft,
        position: state.position,
        velocity: state.velocity,
        rotation,
        angular_velocity: DVec3::ZERO,
    }) else {
        unreachable!()
    };
    s.execute(Action::Select { vessel: id.clone() });
    s.execute(Action::Advance {
        seconds: 60.,
        rails: false,
    });
    println!(
        "thermal part {:?}",
        s.sim().fleet.parts().part(&format!("{id}/pod")).modules["thermal"]
    );
    assert!(
        s.sim().fleet.command_failed(&id),
        "unshielded command pod must fail under sustained heating"
    );
    for action in [
        Action::Sas { enabled: true },
        Action::Control {
            throttle: 1.,
            turn: DVec3::X,
        },
        Action::Stage,
        Action::Rcs {
            control: void_vessels::RcsControl {
                enabled: true,
                force: DVec3::X * 80.,
                torque: DVec3::ZERO,
            },
        },
    ] {
        assert!(
            matches!(s.execute(action), Outcome::Refused(_)),
            "ordinary failed-part input must be an explicit refusal"
        );
    }
    s.execute(Action::EndFrame {
        paused: true,
        rate: 0,
    });
    s.mark();
    let restored = FlightSession::from_recording(s.recording());
    assert_eq!(world_mark(restored.sim()), world_mark(s.sim()));
}
