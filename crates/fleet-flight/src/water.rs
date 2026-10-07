//! Main-game splashdown fixture, using the world's real sea and terrain sampler.
use crate::session::{Action, FlightSession, Outcome};
use glam::{DQuat, DVec3};
pub fn splashdown(session: &mut FlightSession) -> String {
    splashdown_with(session, &void_assembly::reentry_capsule(), 2., 0.)
}
/// Explicit acceptance variant; speeds are fixtures, never a flight capability claim.
pub fn splashdown_with(
    session: &mut FlightSession,
    craft: &void_assembly::Craft,
    descent: f64,
    tilt: f64,
) -> String {
    assert!(descent.is_finite() && descent >= 0. && tilt.is_finite());
    let sim = session.sim();
    let fleet = &sim.fleet;
    let body = sim.home;
    let level = fleet
        .environment()
        .body(body)
        .and_then(|p| p.sea_level_meters)
        .expect("splashdown requires sea");
    let radius = fleet.environment().bodies()[body].radius_meters;
    let direction = (0..4096)
        .find_map(|i| {
            let z = 1. - 2. * (i as f64 + 0.5) / 4096.;
            let t = i as f64 * 2.399963229728653;
            let r = (1. - z * z).sqrt();
            let d = DVec3::new(r * t.cos(), r * t.sin(), z);
            let s = fleet.environment().surroundings_local(
                body,
                void_frames::State {
                    position: d * (radius + level + 8.),
                    velocity: DVec3::ZERO,
                },
            );
            s.ground.filter(|g| g.height < level - 50.).map(|_| d)
        })
        .expect("splashdown requires a deep ocean site");
    let transform = fleet
        .frames()
        .transform(fleet.body_frames(body).1, fleet.origin_frame());
    let state = transform.apply_state(void_frames::State {
        position: direction * (radius + level + 8.),
        velocity: -direction * descent,
    });
    let rotation = transform.rotation()
        * DQuat::from_rotation_arc(DVec3::Y, direction)
        * DQuat::from_rotation_z(tilt);
    let outcome = session.execute(Action::LaunchState {
        craft: craft.clone(),
        position: state.position,
        velocity: state.velocity,
        rotation,
        angular_velocity: DVec3::ZERO,
    });
    let Outcome::Spawned(id) = outcome else {
        panic!("splashdown launch refused")
    };
    session.execute(Action::Select { vessel: id.clone() });
    id
}
