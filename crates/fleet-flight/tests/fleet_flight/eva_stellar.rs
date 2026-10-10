//! Crew transactions retain local precision at real stellar separation.
use crate::common;
use common::{daylight_terrain_site, stellar_neighborhood};
use glam::{DMat3, DQuat, DVec3};
use void_fleet_flight::{
    checkpoint::FlightCheckpoint,
    session::{Action, FlightSession, InitialWorld, Outcome, world_mark},
};
use void_frames::{BodyId, SplitPosition, SystemId};
use void_testkit::crew_rover;

fn make() -> FlightSession {
    let planet = void_testkit::aurelia();
    let world = stellar_neighborhood(&planet);
    let site = daylight_terrain_site(&world, "Sol/aurelia").unwrap();
    let mut initial = InitialWorld::new(&planet, &crew_rover(), site, true);
    initial.world = world;
    initial.launch_body = "Sol/aurelia".into();
    FlightSession::new(initial).with_recording()
}

fn momentum(
    s: &FlightSession,
    carrier: &str,
    members: &[String],
) -> (f64, SplitPosition, DVec3, DVec3) {
    let f = &s.sim().fleet;
    let root = f.precise_snapshot(carrier);
    let mass: f64 = members.iter().map(|id| f.snapshot(id).mass_kg).sum();
    let centre = members.iter().fold(DVec3::ZERO, |sum, id| {
        sum + f.relative(id, carrier).position * f.snapshot(id).mass_kg
    }) / mass;
    let velocity = members.iter().fold(DVec3::ZERO, |sum, id| {
        sum + f.relative(id, carrier).velocity * f.snapshot(id).mass_kg
    }) / mass;
    let angular = members.iter().fold(DVec3::ZERO, |sum, id| {
        let ship = f.precise_snapshot(id).local;
        let rel = f.relative(id, carrier);
        let rotation = DMat3::from_quat(ship.rotation);
        let inertia = f.inertia(id);
        sum + rotation * inertia * rotation.transpose() * ship.angular_velocity
            + (rel.position - centre).cross(rel.velocity - velocity) * ship.mass_kg
    });
    (
        mass,
        // Reduce the centre in the common local vessel frame before traversing the
        // planet's AU-scale ephemeris; adding it to an already rounded global point
        // would measure floating-point reduction order rather than crew conservation.
        f.frames().to_galaxy(
            f.vessel_frame(carrier),
            f.centre_of_mass_local(carrier) + root.local.rotation.conjugate() * centre,
        ),
        (root.velocity + velocity) * mass,
        angular,
    )
}

fn round_trip(s: &mut FlightSession, carrier: String) {
    assert_eq!(s.sim().fleet.vessel_system(&carrier), SystemId(1));
    s.execute(Action::Select {
        vessel: carrier.clone(),
    });
    let seat = s.sim().fleet.crew_seats(&carrier).remove(0);
    let crew_id = seat.occupant.as_ref().unwrap().id.clone();
    let before = momentum(s, &carrier, std::slice::from_ref(&carrier));
    let Outcome::Spawned(actor) = s.execute(Action::EvaExit {
        part: seat.part.clone(),
        module: seat.module.clone(),
    }) else {
        panic!("remote exit failed")
    };
    assert_eq!(s.sim().fleet.vessel_system(&actor), SystemId(1));
    assert_eq!(s.sim().fleet.eva_crew(&actor).unwrap().id, crew_id);
    let split = momentum(s, &carrier, &[carrier.clone(), actor.clone()]);
    assert_eq!(before.0, split.0);
    assert!(
        before.1.relative(&split.1).length() < 1e-6,
        "exit COM error {} m; before {:?}, after {:?}",
        before.1.relative(&split.1).length(),
        before.1,
        split.1
    );
    assert!((before.2 - split.2).length() < 1e-3);
    assert!((before.3 - split.3).length() < 1e-3);

    let home_seat = s.sim().fleet.crew_seats("v1").remove(0);
    let unchanged = world_mark(s.sim());
    assert!(matches!(s.execute(Action::EvaBoard {
        part: home_seat.part, module: home_seat.module,
    }), Outcome::Refused(reason) if reason.contains("different stellar systems")));
    assert_eq!(world_mark(s.sim()), unchanged);

    let saved = FlightCheckpoint::capture(s.sim(), s.recording_initial().clone());
    let mut restored = FlightSession::from_checkpoint(
        serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap(),
    )
    .with_recording();
    let board = Action::EvaBoard {
        part: seat.part,
        module: seat.module,
    };
    assert_eq!(s.execute(board.clone()), Outcome::Spawned(carrier.clone()));
    assert_eq!(restored.execute(board), Outcome::Spawned(carrier.clone()));
    assert_eq!(world_mark(s.sim()), world_mark(restored.sim()));
    assert_eq!(
        s.sim().fleet.crew_seats(&carrier)[0]
            .occupant
            .as_ref()
            .unwrap()
            .id,
        crew_id
    );
    let after = momentum(s, &carrier, std::slice::from_ref(&carrier));
    assert_eq!(before.0, after.0);
    assert!(before.1.relative(&after.1).length() < 1e-6);
    assert!((before.2 - after.2).length() < 1e-3);
    assert!((before.3 - after.3).length() < 1e-3);
    assert_eq!(
        world_mark(s.sim()),
        world_mark(FlightSession::from_recording(s.recording()).sim())
    );
}

#[test]
fn remote_ground_crew_transactions_keep_identity_and_split_momentum() {
    let mut s = make();
    let site = daylight_terrain_site(&s.sim().world, "Beryl/aurelia").unwrap();
    let Outcome::Spawned(carrier) = s.execute(Action::LaunchGroundAt {
        body: "Beryl/aurelia".into(),
        craft: crew_rover(),
        site,
    }) else {
        panic!()
    };
    s.execute(Action::Advance {
        seconds: 5.0,
        rails: false,
    });
    round_trip(&mut s, carrier);
}

#[test]
fn spinning_remote_orbit_crew_transactions_keep_identity_and_split_momentum() {
    let mut s = make();
    let body = s.sim().world.body_index("Beryl/aurelia");
    let (position, velocity) = s.sim().fleet.ephemeris.body_in_system(BodyId(body), 0.0);
    let radius = s.sim().fleet.ephemeris.bodies()[body].radius_meters;
    // Declared orbital initial state, not a claim about ordinary rocket travel.
    let Outcome::Spawned(carrier) = s.execute(Action::LaunchSplitState {
        craft: void_testkit::crewed_flight_rocket(),
        system: SystemId(1),
        position: SplitPosition::ORIGIN.translate(position + DVec3::X * (radius + 2e6)),
        velocity: velocity + DVec3::new(70.0, -30.0, 10.0),
        rotation: DQuat::from_rotation_x(0.4),
        angular_velocity: DVec3::new(0.1, 0.2, -0.15),
    }) else {
        panic!()
    };
    round_trip(&mut s, carrier);
}
