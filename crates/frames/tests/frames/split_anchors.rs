use glam::{DQuat, DVec3};
use void_frames::{BodyId, FrameSource, FrameTree, Motion, SplitPosition, State, SystemId};
struct Source;
impl FrameSource for Source {
    fn system_state(&self, _: SystemId, _: f64) -> (SplitPosition, DVec3) {
        (
            SplitPosition::new(DVec3::ZERO, [1_i128 << 100, 0, 0]),
            DVec3::new(220_000.0, 0.0, 0.0),
        )
    }
    fn body_in_system(&self, _: BodyId, _: f64) -> (DVec3, DVec3) {
        panic!("no bodies")
    }
}
#[test]
fn neighboring_cruise_anchors_keep_centimeters_within_one_system_and_galaxy() {
    let mut tree = FrameTree::new();
    let system = tree.add_system(SystemId(0));
    let far = SplitPosition::new(DVec3::new(0.125, -0.25, 0.0), [1_i128 << 45, 0, 0]);
    let a = tree.add_split_fixed(system, far);
    let b = tree.add_split_fixed(system, far.translate(DVec3::new(0.01, 0.03, 0.0)));
    let ship_a = tree.add_fixed(a, Motion::fixed(DVec3::Y, DQuat::IDENTITY));
    let ship_b = tree.add_fixed(b, Motion::fixed(DVec3::Y, DQuat::IDENTITY));
    let at = tree.at(0.0, &Source);
    let relative = at.transform(ship_b, ship_a).apply_state(State {
        position: DVec3::ZERO,
        velocity: DVec3::ZERO,
    });
    assert!((relative.position - DVec3::new(0.01, 0.03, 0.0)).length() < 1e-14);
    assert_eq!(relative.velocity, DVec3::ZERO);
    let absolute = at.to_galaxy(ship_b, DVec3::ZERO);
    assert!((at.from_galaxy(&absolute, ship_a) - relative.position).length() < 1e-14);
}
