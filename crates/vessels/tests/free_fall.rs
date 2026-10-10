mod common;
use common::{Setup, scene};
use glam::DVec3;
use std::panic::{AssertUnwindSafe, catch_unwind};
use void_frames::State;
use void_landing::{ContactFrame, ContactWorld, ContactWorldOptions, pebble};
use void_orbit::{AdvanceOutcome, PropagationRun, VesselPropagator, VesselState};
use void_vessels::FreeFallFrame;

#[test]
#[should_panic(expected = "contact world: terrain is body-fixed; it needs a planet frame")]
fn free_fall_frame_rejects_terrain() {
    let mut scene = scene(Setup::Coast);
    let initial = scene.fleet.snapshot("v1");
    let frame = FreeFallFrame::new(
        &scene.fleet.ephemeris,
        scene.fleet.options.tolerances,
        0.0,
        State {
            position: initial.position,
            velocity: initial.velocity,
        },
    );
    ContactWorld::new(
        frame,
        Some(pebble().terrain),
        ContactWorldOptions {
            step_seconds: 1.0 / 60.0,
            tile_level: 0,
            tile_resolution: 2,
            tile_reach_meters: 1.0,
            tile_keep_meters: 2.0,
            recenter_meters: 5000.0,
            sleeping: false,
        },
        0.0,
        DVec3::ZERO,
        &mut scene.fleet.ephemeris,
    );
}

#[test]
fn lazy_origin_and_conversions_follow_independent_coast() {
    let mut scene = scene(Setup::Coast);
    let initial = scene.fleet.snapshot("v1");
    let anchor = State {
        position: initial.position,
        velocity: initial.velocity,
    };
    let tolerances = scene.fleet.options.tolerances;
    let mut frame = FreeFallFrame::new(&scene.fleet.ephemeris, tolerances, 0.0, anchor);
    let mut reference = VesselPropagator::new(&scene.fleet.ephemeris, tolerances);
    let mut run = PropagationRun::new(VesselState {
        time: 0.0,
        position: anchor.position,
        velocity: anchor.velocity,
        mass_kg: 1.0,
    });
    assert_eq!(frame.origin_time(), 0.0);
    for time in [1.0, 10.0, 100.0] {
        let local = State {
            position: DVec3::new(100.0, 20.0, -5.0),
            velocity: DVec3::new(1.0, -2.0, 3.0),
        };
        let inertial = frame.to_inertial_at(&mut scene.fleet.ephemeris, time, local);
        assert_eq!(frame.origin_time(), time);
        assert_eq!(
            reference.advance(
                &mut scene.fleet.ephemeris,
                &mut run,
                time,
                100_000,
                None,
                None
            ),
            AdvanceOutcome::Reached
        );
        let origin = frame.origin_at(&mut scene.fleet.ephemeris, time);
        assert!((origin.position - run.state().position).length() < 1e-6);
        assert!((origin.velocity - run.state().velocity).length() < 1e-9);
        let recovered = frame.from_inertial_at(&mut scene.fleet.ephemeris, time, inertial);
        assert!((recovered.position - local.position).length() < 1e-6);
        assert!((recovered.velocity - local.velocity).length() < 1e-9);
        assert_eq!(
            frame.acceleration(&scene.fleet.ephemeris, time, DVec3::ZERO, DVec3::ZERO),
            DVec3::ZERO
        );
    }
    assert!(
        catch_unwind(AssertUnwindSafe(
            || frame.origin_at(&mut scene.fleet.ephemeris, 10.0)
        ))
        .is_err()
    );
}
