//! Pure parachute state transitions. Trial integrator stages only evaluate area/force;
//! the Fleet commits a returned state after accepted simulation time.
use glam::DVec3;
use void_assembly::{
    ParachuteDefinition as Definition, ParachutePhase as Phase, ParachuteState as State,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Deploy,
    Cut,
}
#[derive(Clone, Copy, Debug)]
pub struct Conditions {
    pub pressure_pa: f64,
    pub dynamic_pressure_pa: f64,
    pub altitude_meters: f64,
}
pub fn command(mut state: State, command: Command) -> State {
    match command {
        Command::Deploy => match state.phase {
            Phase::Stowed => state.phase = Phase::Armed,
            Phase::Cut => panic!("cut parachute cannot redeploy"),
            _ => {}
        },
        Command::Cut => match state.phase {
            Phase::Stowed => panic!("stowed parachute cannot be cut"),
            Phase::Cut => {}
            _ => {
                state.phase = Phase::Cut;
                state.elapsed_seconds = 0.0;
            }
        },
    }
    state
}
/// Start environment-triggered transitions at a fixed simulation boundary.
pub fn prepare(mut state: State, p: &Definition, c: Option<Conditions>) -> State {
    if let Some(c) = c {
        assert!(
            c.pressure_pa.is_finite()
                && c.pressure_pa >= 0.0
                && c.dynamic_pressure_pa.is_finite()
                && c.dynamic_pressure_pa >= 0.0
                && c.altitude_meters.is_finite(),
            "invalid parachute surroundings"
        );
        if state.phase == Phase::Armed
            && c.pressure_pa >= p.min_pressure_pa
            && c.dynamic_pressure_pa <= p.max_dynamic_pressure_pa
        {
            state.phase = Phase::SemiDeploying;
        }
        if state.phase == Phase::Semi && c.altitude_meters <= p.full_deploy_altitude_meters {
            state.phase = Phase::FullDeploying;
        }
    }
    state
}
/// Advance an already-started deployment. No resources or states are touched in force evaluation.
pub fn commit(mut s: State, p: &Definition, seconds: f64) -> State {
    assert!(
        seconds.is_finite() && seconds >= 0.0,
        "invalid parachute duration"
    );
    let (limit, after) = match s.phase {
        Phase::SemiDeploying => (p.semi_seconds, Phase::Semi),
        Phase::FullDeploying => (p.full_seconds, Phase::Full),
        _ => return s,
    };
    s.elapsed_seconds += seconds;
    if s.elapsed_seconds + 1e-12 >= limit {
        s.phase = after;
        s.elapsed_seconds = 0.0;
    }
    s
}
pub fn area(s: State, p: &Definition, trial_seconds: f64) -> f64 {
    assert!(
        trial_seconds.is_finite() && trial_seconds >= 0.0,
        "invalid parachute trial duration"
    );
    match s.phase {
        Phase::Stowed | Phase::Armed | Phase::Cut => 0.0,
        Phase::SemiDeploying => {
            p.semi_area_m2 * ((s.elapsed_seconds + trial_seconds) / p.semi_seconds).min(1.0)
        }
        Phase::Semi => p.semi_area_m2,
        Phase::FullDeploying => {
            p.semi_area_m2
                + (p.full_area_m2 - p.semi_area_m2)
                    * ((s.elapsed_seconds + trial_seconds) / p.full_seconds).min(1.0)
        }
        Phase::Full => p.full_area_m2,
    }
}
pub fn force(s: State, p: &Definition, trial_seconds: f64, density: f64, airspeed: DVec3) -> DVec3 {
    assert!(
        density.is_finite() && density >= 0.0 && airspeed.is_finite(),
        "invalid parachute air"
    );
    -airspeed * (0.5 * density * p.drag_coefficient * area(s, p, trial_seconds) * airspeed.length())
}
pub fn active(s: State) -> bool {
    !matches!(s.phase, Phase::Stowed | Phase::Cut)
}
