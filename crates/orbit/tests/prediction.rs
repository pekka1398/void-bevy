use void_frames::{BodyId, BodyStates};
use void_orbit::*;
fn source() -> Ephemeris {
    let system = build_system(&SystemSpec::from_json(include_str!("../systems/sol.json")));
    let mut e = Ephemeris::new(
        &system,
        EphemerisOptions {
            step_seconds: 100.0,
            chunk_steps: 16,
        },
    );
    e.extend_to(300.0);
    e
}
#[test]
fn snapshot_exact_continuation_is_independent_and_send() {
    let mut live = source();
    let snapshot = live
        .prediction_snapshot(PredictionBudget::default(), CancellationToken::new())
        .unwrap();
    let states = std::thread::spawn(move || {
        let mut copy = snapshot.into_source();
        copy.try_extend_to(2700.0).unwrap();
        [150.0, 350.0, 2650.0].map(|t| {
            (0..copy.bodies().len())
                .map(|b| copy.body_state(BodyId(b), t))
                .collect::<Vec<_>>()
        })
    })
    .join()
    .unwrap();
    assert_eq!(live.end_time(), 300.0);
    live.extend_to(2700.0);
    for (t, expected) in [150.0, 350.0, 2650.0].into_iter().zip(states) {
        for (b, state) in expected.into_iter().enumerate() {
            assert_eq!(live.body_state(BodyId(b), t), state);
        }
    }
}
#[test]
fn snapshot_budget_and_cancellation_precede_copy_or_extension() {
    let e = source();
    let budget = PredictionBudget {
        bytes: 1,
        ..Default::default()
    };
    assert!(matches!(
        e.prediction_snapshot(budget, CancellationToken::new()),
        Err(PredictionError::BudgetExceeded {
            resource: "bytes",
            ..
        })
    ));
    let token = CancellationToken::new();
    let mut copy = e
        .prediction_snapshot(PredictionBudget::default(), token.clone())
        .unwrap()
        .into_source();
    token.cancel();
    assert_eq!(copy.try_extend_to(1e20), Err(PredictionError::Cancelled));
    assert_eq!(copy.end_time(), 300.0);
    let mut copy = e
        .prediction_snapshot(
            PredictionBudget {
                ephemeris_steps: 2,
                ..Default::default()
            },
            CancellationToken::new(),
        )
        .unwrap()
        .into_source();
    assert!(matches!(
        copy.try_extend_to(1e20),
        Err(PredictionError::BudgetExceeded {
            resource: "ephemeris steps",
            ..
        })
    ));
    assert_eq!(copy.end_time(), 300.0);
}

#[test]
fn adoption_rejects_changed_live_integrator_and_transfers_exact_future() {
    let mut live = source();
    let mut prediction = live
        .prediction_snapshot(PredictionBudget::default(), CancellationToken::new())
        .unwrap()
        .into_source();
    prediction.try_extend_to(2700.0).unwrap();
    let expected = prediction.body_state(BodyId(1), 2650.0);
    live.adopt_prediction(prediction.export_prediction().unwrap())
        .unwrap();
    assert_eq!(live.body_state(BodyId(1), 2650.0), expected);
    assert!(live.prediction_context().is_none());
    let prediction = live
        .prediction_snapshot(PredictionBudget::default(), CancellationToken::new())
        .unwrap()
        .into_source();
    live.extend_to(2800.0);
    assert_eq!(
        live.adopt_prediction(prediction.export_prediction().unwrap()),
        Err(PredictionError::IncompatibleSnapshot)
    );
}
#[test]
fn trial_budget_stops_before_vessel_time_advance() {
    let e = source();
    let mut copy = e
        .prediction_snapshot(
            PredictionBudget {
                vessel_trials: 0,
                ..Default::default()
            },
            CancellationToken::new(),
        )
        .unwrap()
        .into_source();
    let mut run = PropagationRun::new(VesselState {
        time: 100.0,
        position: glam::DVec3::new(1e13, 0.0, 0.0),
        velocity: glam::DVec3::ZERO,
        mass_kg: 1000.0,
    });
    let mut propagator = VesselPropagator::new(
        &copy,
        Tolerances {
            position_meters: 0.02,
            velocity_meters_per_second: 0.001,
        },
    );
    assert_eq!(
        propagator.advance(&mut copy, &mut run, 200.0, 100, None, None),
        AdvanceOutcome::Budget
    );
    assert_eq!(run.time, 100.0);
    assert!(matches!(
        copy.prediction_context().unwrap().check(),
        Err(PredictionError::BudgetExceeded {
            resource: "vessel trials",
            ..
        })
    ));
}

#[test]
fn future_chunk_budget_is_rejected_before_integrating() {
    let live = source();
    let copy = live
        .prediction_snapshot(PredictionBudget::default(), CancellationToken::new())
        .unwrap()
        .into_source();
    let bytes = copy.prediction_context().unwrap().usage().reserved_bytes as usize;
    let mut copy = live
        .prediction_snapshot(
            PredictionBudget {
                bytes,
                ..Default::default()
            },
            CancellationToken::new(),
        )
        .unwrap()
        .into_source();
    // Sample 16 begins a new chunk; no work or new chunk is allowed by this budget.
    assert!(matches!(
        copy.try_extend_to(1600.0),
        Err(PredictionError::BudgetExceeded {
            resource: "bytes",
            ..
        })
    ));
    assert_eq!(copy.end_time(), 300.0);
    assert_eq!(
        copy.prediction_context().unwrap().usage().ephemeris_steps,
        0
    );
}
