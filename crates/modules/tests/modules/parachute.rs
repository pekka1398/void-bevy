use glam::DVec3;
use void_assembly::{Module, ParachutePhase, ParachuteState, definition};
use void_modules::parachute::*;
fn config() -> void_assembly::ParachuteDefinition {
    let Module::Parachute { parameters, .. } = &definition("parachute-pod").unwrap().modules[1]
    else {
        panic!()
    };
    *parameters
}
#[test]
fn accepted_time_and_deployment_conditions_control_area_and_drag() {
    let p = config();
    let s = command(ParachuteState::STOWED, Command::Deploy);
    assert_eq!(prepare(s, &p, None), s);
    assert_eq!(command(s, Command::Deploy), s);
    let unsafe_air = Conditions {
        pressure_pa: 1000.0,
        dynamic_pressure_pa: 30000.0,
        altitude_meters: 1000.0,
    };
    assert_eq!(prepare(s, &p, Some(unsafe_air)), s);
    let safe = Conditions {
        dynamic_pressure_pa: 1000.0,
        ..unsafe_air
    };
    let half = prepare(s, &p, Some(safe));
    assert_eq!(half.phase, ParachutePhase::SemiDeploying);
    // Rejected integrator trials cannot advance graph state.
    let before = half;
    for dt in [0.1, 0.7, 0.2] {
        let f = force(half, &p, dt, 1.0, DVec3::Y * 100.0);
        assert!(f.dot(DVec3::Y * 100.0) < 0.0);
    }
    assert_eq!(half, before);
    assert_eq!(area(half, &p, 0.5), p.semi_area_m2 * 0.5);
    let half = commit(half, &p, 1.0);
    assert_eq!(half.phase, ParachutePhase::Semi);
    let opening = prepare(half, &p, Some(safe));
    assert_eq!(opening.phase, ParachutePhase::FullDeploying);
    let one = commit(opening, &p, 2.0);
    let two = commit(commit(opening, &p, 0.7), &p, 1.3);
    assert_eq!(one, two);
    assert_eq!(one.phase, ParachutePhase::Full);
    assert_eq!(force(one, &p, 0.0, 0.0, DVec3::Y * 100.0), DVec3::ZERO);
    let cut = command(one, Command::Cut);
    assert_eq!(command(cut, Command::Cut), cut);
    assert_eq!(area(cut, &p, 0.0), 0.0);
    assert!(std::panic::catch_unwind(|| command(cut, Command::Deploy)).is_err());
}
