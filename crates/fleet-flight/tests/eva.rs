use glam::{DMat3, DQuat, DVec3};
use void_assembly::{ModuleState, crew_rover};
use void_fleet_flight::{
    checkpoint::FlightCheckpoint,
    session::{Action, FlightSession, InitialWorld, Outcome, world_mark},
};
use void_vessels::flat_site;
fn make() -> FlightSession {
    let p = void_landing::earth_size();
    FlightSession::new(InitialWorld::new(&p, &crew_rover(), flat_site(&p), false)).with_recording()
}
fn momentum(s: &FlightSession) -> (f64, DVec3, DVec3, DVec3) {
    let f = &s.sim().fleet;
    let ids = f.vessel_ids();
    let root = f.snapshot(&ids[0]);
    let total: f64 = ids.iter().map(|id| f.snapshot(id).mass_kg).sum();
    let centre = ids.iter().fold(DVec3::ZERO, |sum, id| {
        sum + f.relative(id, &ids[0]).position * f.snapshot(id).mass_kg
    }) / total;
    let velocity = ids.iter().fold(DVec3::ZERO, |sum, id| {
        sum + f.relative(id, &ids[0]).velocity * f.snapshot(id).mass_kg
    }) / total;
    let angular = ids.iter().fold(DVec3::ZERO, |sum, id| {
        let ship = f.snapshot(id);
        let rel = f.relative(id, &ids[0]);
        let r = DMat3::from_quat(ship.rotation);
        let i = DMat3::from_cols_array(&f.inertia(id)).transpose();
        sum + r * i * r.transpose() * ship.angular_velocity
            + (rel.position - centre).cross(rel.velocity - velocity) * ship.mass_kg
    });
    (
        total,
        root.position + centre,
        (root.velocity + velocity) * total,
        angular,
    )
}
#[test]
fn rover_crew_exit_board_preserves_identity_pack_mass_momentum_and_replays() {
    let mut s = make();
    s.execute(Action::Advance {
        seconds: 5.0,
        rails: false,
    });
    let id = s.sim().selected.clone();
    let seat = s.sim().fleet.crew_seats(&id).remove(0);
    let crew = seat.occupant.clone().unwrap();
    let before = momentum(&s);
    let outcome = s.execute(Action::EvaExit {
        part: seat.part.clone(),
        module: seat.module.clone(),
    });
    let Outcome::Spawned(actor) = outcome else {
        panic!("exit {outcome:?}")
    };
    let after = momentum(&s);
    println!(
        "exit conservation: COM {} P {} L {}",
        (before.1 - after.1).length(),
        (before.2 - after.2).length(),
        (before.3 - after.3).length()
    );
    assert_eq!(before.0, after.0);
    assert!((before.1 - after.1).length() < 1e-7);
    assert!((before.2 - after.2).length() < 1e-3);
    assert!((before.3 - after.3).length() < 1e-3);
    assert_eq!(s.sim().fleet.eva_crew(&actor).unwrap().id, crew.id);
    assert!(s.sim().fleet.crew_seats(&id)[0].occupant.is_none());
    assert_eq!(
        s.sim()
            .fleet
            .parts()
            .part(&seat.part)
            .resource(void_assembly::ResourceId::Monopropellant),
        0.0
    );
    let saved = FlightCheckpoint::capture(s.sim(), s.recording_initial().clone());
    let mut copy = FlightSession::from_checkpoint(
        serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap(),
    )
    .with_recording();
    assert_eq!(world_mark(s.sim()), world_mark(copy.sim()));
    let board = Action::EvaBoard {
        part: seat.part.clone(),
        module: seat.module.clone(),
    };
    assert_eq!(s.execute(board.clone()), copy.execute(board));
    assert_eq!(world_mark(s.sim()), world_mark(copy.sim()));
    assert_eq!(s.sim().fleet.vessel_ids().len(), 1);
    assert_eq!(
        s.sim().fleet.crew_seats(&id)[0]
            .occupant
            .as_ref()
            .unwrap()
            .id,
        crew.id
    );
    let final_m = momentum(&s);
    assert_eq!(before.0, final_m.0);
    assert!((before.1 - final_m.1).length() < 1e-7);
    assert!((before.2 - final_m.2).length() < 1e-3);
    assert!((before.3 - final_m.3).length() < 1e-3);
    let replay = FlightSession::from_recording(s.recording());
    assert_eq!(world_mark(s.sim()), world_mark(replay.sim()));
}
#[test]
fn spinning_orbital_hatch_transfer_conserves_total_momentum() {
    let mut s = make();
    let p = s.sim().planet.clone();
    let pos = s.sim().fleet.snapshot(&s.sim().selected).position;
    // Launch a declared diagnostic orbital carrier state; no special propulsion is claimed.
    let initial = InitialWorld::new(&p, &void_assembly::demo_craft(), flat_site(&p), false);
    s.execute(Action::ResetWorld {
        initial: Box::new(initial),
    });
    let Outcome::Spawned(id) = s.execute(Action::LaunchState {
        craft: crew_rover(),
        position: pos + DVec3::Y * 2e6,
        velocity: DVec3::new(70.0, -30.0, 10.0),
        rotation: DQuat::from_rotation_x(0.4),
        angular_velocity: DVec3::new(0.1, 0.2, -0.15),
    }) else {
        panic!()
    };
    s.execute(Action::Select { vessel: id.clone() });
    let seat = s.sim().fleet.crew_seats(&id).remove(0);
    let before = momentum(&s);
    let exit = s.execute(Action::EvaExit {
        part: seat.part.clone(),
        module: seat.module.clone(),
    });
    assert!(matches!(exit, Outcome::Spawned(_)), "{exit:?}");
    let after = momentum(&s);
    println!(
        "orbit P {} L {}",
        (before.2 - after.2).length(),
        (before.3 - after.3).length()
    );
    assert_eq!(before.0, after.0);
    assert!((before.2 - after.2).length() < 1e-5);
    assert!((before.3 - after.3).length() < 1e-3);
    let _ = ModuleState::Passive;
}
#[test]
fn grounded_eva_walk_and_jump_use_native_contact_and_finite_actuators() {
    let mut s = make();
    s.execute(Action::Advance {
        seconds: 5.0,
        rails: false,
    });
    let seat = s.sim().fleet.crew_seats(&s.sim().selected).remove(0);
    let Outcome::Spawned(actor) = s.execute(Action::EvaExit {
        part: seat.part,
        module: seat.module,
    }) else {
        panic!()
    };
    s.execute(Action::Advance {
        seconds: 3.0,
        rails: false,
    });
    let home = s.sim().home;
    let start = s.sim().fleet.body_fixed_state(&actor, home);
    println!("EVA landed {:?}", s.sim().fleet.snapshot(&actor));
    assert!(s.sim().fleet.parts().parts().any(|p| {
        p.modules
            .values()
            .any(|m| matches!(m, ModuleState::Crew { grounded: true, .. }))
    }));
    s.execute(Action::Eva {
        control: void_assembly::EvaControl {
            forward: 1.0,
            strafe: 0.0,
            yaw: 0.0,
        },
    });
    s.execute(Action::Advance {
        seconds: 5.0,
        rails: false,
    });
    let walked = s.sim().fleet.body_fixed_state(&actor, home);
    println!(
        "walked {} velocity {:?}",
        (walked.position - start.position).length(),
        walked.velocity
    );
    assert!((walked.position - start.position).length() > 2.0);
    let snap = s.sim().fleet.snapshot(&actor);
    let up = s
        .sim()
        .fleet
        .frames()
        .transform(
            s.sim().fleet.body_frames(home).1,
            s.sim().fleet.origin_frame(),
        )
        .apply_direction(walked.position.normalize());
    assert!(
        (snap.rotation * DVec3::Y).dot(up) > 0.8,
        "dynamic astronaut tipped over: {snap:?}"
    );
    s.execute(Action::Eva {
        control: void_assembly::EvaControl::default(),
    });
    s.execute(Action::Advance {
        seconds: 1.0,
        rails: false,
    });
    assert_eq!(s.execute(Action::EvaJump), Outcome::Applied);
    assert!(matches!(s.execute(Action::EvaJump), Outcome::Refused(_)));
    let jumping = s.sim().fleet.body_fixed_state(&actor, home);
    let before = jumping.position;
    s.execute(Action::Advance {
        seconds: 0.2,
        rails: false,
    });
    let airborne = s.sim().fleet.body_fixed_state(&actor, home);
    assert!((airborne.position - before).dot(before.normalize()) > 0.1);
    assert!(matches!(s.execute(Action::EvaJump), Outcome::Refused(_)));
}

#[test]
fn low_gravity_jump_is_a_real_ballistic_impulse() {
    let p = void_landing::pebble();
    let mut s = FlightSession::new(InitialWorld::new(&p, &crew_rover(), flat_site(&p), false));
    s.execute(Action::Advance {
        seconds: 5.0,
        rails: false,
    });
    let seat = s.sim().fleet.crew_seats(&s.sim().selected).remove(0);
    let Outcome::Spawned(actor) = s.execute(Action::EvaExit {
        part: seat.part,
        module: seat.module,
    }) else {
        panic!()
    };
    s.execute(Action::Advance {
        seconds: 5.0,
        rails: false,
    });
    let before = s.sim().fleet.body_fixed_state(&actor, s.sim().home);
    assert_eq!(s.execute(Action::EvaJump), Outcome::Applied);
    s.execute(Action::Advance {
        seconds: 1.0,
        rails: false,
    });
    let after = s.sim().fleet.body_fixed_state(&actor, s.sim().home);
    let height = (after.position - before.position).dot(before.position.normalize());
    assert!(
        height > 1.0 && height < 3.2,
        "low-gravity ballistic rise {height}m"
    );
    assert!(matches!(s.execute(Action::EvaJump), Outcome::Refused(_)));
}

#[test]
fn orbital_backpack_uses_existing_finite_resource_and_checkpoint_journal() {
    let mut s = make();
    let Outcome::Spawned(actor) = s.execute(Action::LaunchOrbit {
        craft: void_assembly::eva_suit(),
        offset: DVec3::ZERO,
    }) else {
        panic!()
    };
    s.execute(Action::Select {
        vessel: actor.clone(),
    });
    let suit = s.sim().fleet.snapshot(&actor).part_ids[0].clone();
    let before = s
        .sim()
        .fleet
        .parts()
        .part(&suit)
        .resource(void_assembly::ResourceId::Monopropellant);
    s.execute(Action::Rcs {
        control: void_vessels::RcsControl {
            enabled: true,
            force: DVec3::Y * 40.0,
            torque: DVec3::Y * 8.0,
        },
    });
    s.execute(Action::Advance {
        seconds: 2.0,
        rails: false,
    });
    let after = s
        .sim()
        .fleet
        .parts()
        .part(&suit)
        .resource(void_assembly::ResourceId::Monopropellant);
    assert!(after < before && after > 0.0);
    assert!((s.sim().fleet.snapshot(&actor).mass_kg - (100.0 + after)).abs() < 1e-10);
    let saved = FlightCheckpoint::capture(s.sim(), s.recording_initial().clone());
    let mut restored = FlightSession::from_checkpoint(
        serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap(),
    )
    .with_recording();
    let step = Action::Advance {
        seconds: 0.7,
        rails: false,
    };
    assert_eq!(s.execute(step.clone()), restored.execute(step));
    assert_eq!(world_mark(s.sim()), world_mark(restored.sim()));
    let replay = FlightSession::from_recording(s.recording());
    assert_eq!(world_mark(s.sim()), world_mark(replay.sim()));
}

#[test]
fn positive_strafe_and_yaw_follow_body_fixed_player_right_not_graphics_x() {
    let p = void_landing::earth_size();
    let mut s = FlightSession::new(InitialWorld::new(
        &p,
        &void_assembly::eva_suit(),
        flat_site(&p),
        false,
    ));
    s.execute(Action::Advance {
        seconds: 3.0,
        rails: false,
    });
    let id = s.sim().selected.clone();
    let home = s.sim().home;
    let before = s.sim().fleet.body_fixed_state(&id, home);
    let up = before.position.normalize();
    let north = (DVec3::Z - up * up.z).normalize();
    let east = DVec3::Z.cross(up).normalize();
    let heading = |sim: &void_fleet_flight::FleetFlight| {
        let forward = sim
            .fleet
            .frames()
            .transform(sim.fleet.vessel_frame(&id), sim.fleet.body_frames(home).1)
            .apply_direction(DVec3::Z);
        forward.dot(east).atan2(forward.dot(north))
    };
    let first = heading(s.sim());
    s.execute(Action::Eva {
        control: void_assembly::EvaControl {
            forward: 0.0,
            strafe: 1.0,
            yaw: 0.0,
        },
    });
    s.execute(Action::Advance {
        seconds: 4.0,
        rails: false,
    });
    let delta = s.sim().fleet.body_fixed_state(&id, home).position - before.position;
    let eastward = delta.dot(east);
    let northward = delta.dot(north);
    assert!(
        eastward * first.cos() - northward * first.sin() > 2.0,
        "right strafe in true body north/east: N{northward} E{eastward} initial heading{first}"
    );
    s.execute(Action::Eva {
        control: void_assembly::EvaControl {
            forward: 0.0,
            strafe: 0.0,
            yaw: 1.0,
        },
    });
    s.execute(Action::Advance {
        seconds: 0.5,
        rails: false,
    });
    let after = heading(s.sim());
    let change = (after - first + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
        - std::f64::consts::PI;
    assert!(
        change > 0.05,
        "right yaw must increase physical heading, delta{change}"
    );
}
