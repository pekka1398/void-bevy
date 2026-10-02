use glam::DVec3;
use std::panic::{AssertUnwindSafe, catch_unwind};
use void_landing::{ContactFrame, FrameState};
use void_orbit::{AdvanceOutcome, PropagationRun, VesselPropagator, VesselState};
use void_vessels::{FreeFallFrame, Scenario, create_lab_scene};

#[test]
fn lazy_origin_and_conversions_follow_independent_coast() {
    let mut scene = create_lab_scene(Scenario::Coast);
    let initial = scene.fleet.snapshot("v1");
    let anchor = FrameState {
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
        let local = FrameState {
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
