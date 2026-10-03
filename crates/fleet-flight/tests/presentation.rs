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
    let mut loaded = FlightSession::from_checkpoint(saved);
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
        world_mark(FlightSession::from_recording(recording).sim()),
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
    assert!(std::panic::catch_unwind(|| FlightSession::from_recording(recording)).is_err());
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
        world_mark(FlightSession::from_recording(recording).sim()),
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
