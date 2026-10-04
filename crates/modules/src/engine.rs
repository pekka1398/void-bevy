//! The engine module: thrust along the part's axis, less the back pressure on its nozzle.
use crate::Conditions;
use glam::DVec3;
use void_assembly::{G0, Part};

/// One engine's push for a leg, in its vessel's parts frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thrust {
    pub force: DVec3,
    /// The part's origin.
    pub point: DVec3,
    /// Propellant drawn from its crossfeed group.
    pub flow_kg_per_second: f64,
}

/// `throttle` of the vacuum rating, less nozzle exit area × ambient pressure of the vacuum
/// thrust; an overexpanded nozzle stops at zero (the flow separates) rather than pulling back.
/// The mass flow stays the vacuum rating's.
pub fn thrust(part: &Part, throttle: f64, conditions: &Conditions) -> Thrust {
    let engine = part
        .engine()
        .unwrap_or_else(|| panic!("engine: {} has no engine module", part.id));
    assert!(
        (0.0..=1.0).contains(&throttle),
        "engine: {} throttle {throttle}",
        part.id
    );
    let area = engine.nozzle_exit_area_m2;
    assert!(
        area.is_finite() && area >= 0.0,
        "engine: {} nozzle area {area}",
        part.id
    );
    let thrust = engine.thrust_newtons * throttle;
    let scale = (1.0 - area * conditions.ambient_pressure_pa() / engine.thrust_newtons).max(0.0);
    assert!(
        scale.is_finite() && (0.0..=1.0).contains(&scale),
        "engine: {} back pressure scale {scale}",
        part.id
    );
    Thrust {
        force: part.pose.rotation * engine.direction * thrust * scale,
        point: part.pose.position,
        flow_kg_per_second: thrust / (engine.isp_seconds * G0),
    }
}
