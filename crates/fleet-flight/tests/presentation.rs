use glam::DVec3;
use void_assembly::demo_craft;
use void_fleet_flight::{
    checkpoint::FlightCheckpoint,
    presentation::{Toggle, ViewCommand},
    session::{Action, FlightSession, InitialWorld, Playback, world_mark},
};
use void_vessels::flat_site;
fn session() -> FlightSession {
    let planet = void_landing::earth_size();
    FlightSession::new(InitialWorld::new(
        &planet,
        &demo_craft(),
        flat_site(&planet),
        false,
    ))
    .with_recording()
}
fn view(s: &mut FlightSession, command: ViewCommand) {
    s.execute(Action::View { command });
}
#[test]
fn paused_visual_frames_replay_one_at_a_time_and_resume_from_checkpoint() {
    let mut original = session();
    let before = original.sim().presentation.clone();
    view(&mut original, ViewCommand::Drag { x: 21.0, y: -13.0 });
    original.execute(Action::EndFrame {
        paused: true,
        rate: 0,
    });
    original.mark();
    let first = world_mark(original.sim());
    assert_ne!(original.sim().presentation.direction, before.direction);
    assert_eq!(original.sim().fleet.time(), 0.0);
    view(&mut original, ViewCommand::Zoom { pixels: -1500.0 });
    view(
        &mut original,
        ViewCommand::Toggle {
            setting: Toggle::Colliders,
        },
    );
    view(
        &mut original,
        ViewCommand::Toggle {
            setting: Toggle::Terrain,
        },
    );
    original.execute(Action::EndFrame {
        paused: true,
        rate: 0,
    });
    original.mark();
    let second = world_mark(original.sim());
    let saved = FlightCheckpoint::capture(original.sim(), original.recording_initial().clone());
    let mut loaded = FlightSession::from_checkpoint(saved).with_recording();
    assert_eq!(world_mark(loaded.sim()), second);
    for s in [&mut original, &mut loaded] {
        view(s, ViewCommand::Focus { body: Some(0) });
        view(
            s,
            ViewCommand::Toggle {
                setting: Toggle::PathFrame,
            },
        );
        s.execute(Action::Advance {
            seconds: 0.137,
            rails: false,
        });
        s.execute(Action::EndFrame {
            paused: false,
            rate: 1,
        });
        s.mark();
    }
    assert_eq!(world_mark(original.sim()), world_mark(loaded.sim()));
    let recording = original.recording();
    let (mut replay, mut restored) = Playback::new(recording.clone());
    assert!(replay.next_frame(&mut restored));
    assert_eq!(world_mark(restored.sim()), first);
    assert!(replay.next_frame(&mut restored));
    assert_eq!(world_mark(restored.sim()), second);
    assert!(replay.next_frame(&mut restored));
    assert!(!replay.next_frame(&mut restored));
    assert_eq!(world_mark(restored.sim()), world_mark(original.sim()));
    assert_eq!(
        world_mark(
            FlightSession::from_recording(recording)
                .with_recording()
                .sim()
        ),
        world_mark(original.sim())
    );
}
#[test]
fn changing_one_camera_input_fails_visual_marks_without_changing_physics() {
    let mut original = session();
    view(&mut original, ViewCommand::Drag { x: 30.0, y: 2.0 });
    original.execute(Action::EndFrame {
        paused: true,
        rate: 0,
    });
    let mut recording = original.recording();
    recording.entries[0].action = Action::View {
        command: ViewCommand::Drag { x: 29.0, y: 2.0 },
    };
    assert!(
        std::panic::catch_unwind(|| FlightSession::from_recording(recording).with_recording())
            .is_err()
    );
}
#[test]
fn camera_sampling_is_read_only_at_any_render_cadence() {
    let mut a = session();
    let mut b = session();
    for i in 0..30 {
        for s in [&mut a, &mut b] {
            if i == 10 {
                view(
                    s,
                    ViewCommand::Toggle {
                        setting: Toggle::PathFrame,
                    },
                );
            }
            s.execute(Action::Advance {
                seconds: 0.113,
                rails: false,
            });
        }
        let before = world_mark(a.sim());
        for _ in 0..i {
            let sample = a.sim().presentation.sample(a.sim());
            assert!(sample.eye.is_finite());
        }
        assert_eq!(before, world_mark(a.sim()));
    }
    assert_eq!(world_mark(a.sim()), world_mark(b.sim()));
}
#[test]
fn switching_vessel_resets_body_focus_and_plain_camera_replays() {
    let mut s = session();
    view(&mut s, ViewCommand::Configure { main_camera: false });
    view(&mut s, ViewCommand::Drag { x: 20.0, y: -4.0 });
    view(&mut s, ViewCommand::Zoom { pixels: 40.0 });
    assert_eq!(s.sim().presentation.yaw, 0.4 - 20.0 * 0.006);
    assert_eq!(s.sim().presentation.pitch, 0.25 - 4.0 * 0.006);
    let ship = s.execute(Action::LaunchOrbit {
        craft: demo_craft(),
        offset: DVec3::ZERO,
    });
    let void_fleet_flight::session::Outcome::Spawned(vessel) = ship else {
        panic!("spawn")
    };
    view(&mut s, ViewCommand::Focus { body: Some(0) });
    s.execute(Action::Select { vessel });
    assert_eq!(s.sim().presentation.focus_body, None);
    assert_eq!(s.sim().presentation.distance, 40.0);
    s.execute(Action::ResetWorld {
        initial: Box::new(s.recording_initial().clone()),
    });
    assert!(
        !s.sim().presentation.main_camera,
        "reset keeps the lab camera mode"
    );
    let recording = s.recording();
    assert_eq!(
        world_mark(
            FlightSession::from_recording(recording)
                .with_recording()
                .sim()
        ),
        world_mark(s.sim())
    );
}
#[test]
fn non_finite_view_input_is_rejected() {
    let mut s = session();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| view(
            &mut s,
            ViewCommand::Drag {
                x: f64::NAN,
                y: 0.0
            }
        )))
        .is_err()
    );
}
#[test]
fn the_camera_frame_agrees_with_the_inertial_eye() {
    let mut s = session();
    for main_camera in [true, false] {
        view(&mut s, ViewCommand::Configure { main_camera });
        let sim = s.sim();
        let f = &sim.fleet;
        let sample = sim.presentation.sample(sim);
        let axes = f
            .frames()
            .transform(f.body_frames(sim.home).1, f.origin_frame())
            .rotation();
        // Through the focus frame, the focus sits exactly one camera distance away.
        let focus = sample
            .to_camera(f, sample.focus_frame, axes)
            .apply_point(sample.focus_local);
        assert!((focus + axes.inverse() * sample.offset).length() < 1e-9);
        // The long way, through the system's large coordinates, lands on the same eye.
        let eye = sample
            .to_camera(f, f.origin_frame(), axes)
            .apply_point(sample.eye);
        assert!(eye.length() < 1e-3, "{eye}");
        for part in f.part_snapshots(&sim.selected) {
            let near = sample
                .to_camera(f, part.frame, axes)
                .apply_point(part.local_position);
            let far = sample
                .to_camera(f, f.origin_frame(), axes)
                .apply_point(part.position);
            assert!((near - far).length() < 1e-3, "{}: {near} {far}", part.id);
        }
    }
}

#[test]
fn plain_camera_can_focus_every_body_and_return_to_the_ship() {
    let mut s = session();
    view(&mut s, ViewCommand::Configure { main_camera: false });
    let initial_offset = s.sim().presentation.sample(s.sim()).offset;
    let count = s.sim().fleet.ephemeris.bodies().len();
    for body in (0..count).map(Some).chain(std::iter::once(None)) {
        view(&mut s, ViewCommand::Focus { body });
        let sim = s.sim();
        let sample = sim.presentation.sample(sim);
        assert!(sample.eye.is_finite() && sample.offset.is_finite());
        assert!((sample.offset.normalize() - initial_offset.normalize()).length() < 1e-12);
        let axes = sim
            .fleet
            .frames()
            .transform(sim.fleet.body_frames(sim.home).1, sim.fleet.origin_frame())
            .rotation();
        let focus = sample
            .to_camera(&sim.fleet, sample.focus_frame, axes)
            .apply_point(sample.focus_local);
        assert!(
            (focus.length() - sim.presentation.distance).abs()
                < 1e-12 * sim.presentation.distance.max(1.0)
        );
    }
}

#[test]
fn camera_tracks_the_upper_command_part_before_and_after_staging() {
    for main_camera in [true, false] {
        let planet = void_landing::earth_size();
        let craft = void_assembly::flight_rocket();
        let mut s = FlightSession::new(InitialWorld::new(
            &planet,
            &craft,
            flat_site(&planet),
            false,
        ));
        s.execute(Action::View {
            command: ViewCommand::Configure { main_camera },
        });
        let root = s
            .sim()
            .fleet
            .part_snapshots(&s.sim().selected)
            .into_iter()
            .find(|p| p.definition.id == "flight-pod")
            .unwrap();
        let before = s.sim().presentation.sample(s.sim());
        assert!((before.focus - root.position).length() < 1e-8);
        assert_eq!(before.focus_local, root.local_position);
        // Ignite, then separate without advancing physics: neither the part nor camera may jump.
        s.execute(Action::Stage);
        s.execute(Action::Stage);
        let after = s.sim().presentation.sample(s.sim());
        let root_after = s
            .sim()
            .fleet
            .part_snapshots(&s.sim().selected)
            .into_iter()
            .find(|p| p.id == root.id)
            .unwrap();
        assert!((after.focus - root_after.position).length() < 1e-8);
        assert!((after.focus - before.focus).length() < 1e-5);
        assert!((after.eye - before.eye).length() < 1e-5);
        let in_camera = after
            .to_camera(&s.sim().fleet, root_after.frame, glam::DQuat::IDENTITY)
            .apply_point(root_after.local_position);
        assert!((in_camera + after.offset).length() < 1e-8);
    }
}
