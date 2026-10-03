use glam::DVec3;
use std::{
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};
use void_frames::SplitPosition;
use void_multiscale::{default_galaxy, wide_world};
use void_multiscale_lab::Encounter;

fn scene(galaxy: SplitPosition) -> Encounter {
    Encounter::new(Rc::new(RefCell::new(wide_world(galaxy))), "Aster")
}
#[test]
fn collision_is_independent_of_galactic_placement() {
    let mut a = scene(SplitPosition::at(DVec3::ZERO));
    let mut b = scene(default_galaxy());
    a.advance(40.0);
    b.advance(40.0);
    let ra = a.fleet.relative(&a.second, &a.first);
    let rb = b.fleet.relative(&b.second, &b.first);
    assert!((ra.position - rb.position).length() < 1e-4);
    assert!((ra.velocity - rb.velocity).length() < 1e-5);
    assert!(ra.velocity.length() < 0.1);
    let sa = b.fleet.snapshot(&b.first);
    let sb = b.fleet.snapshot(&b.second);
    assert!(sa.scene.is_some());
    assert_eq!(sa.scene, sb.scene);
    let distance = b
        .position(&b.first)
        .relative(&b.position(&b.second))
        .length();
    assert!((distance - rb.position.length()).abs() < 1e-4);
    println!(
        "separation {distance:.6} m; relative speed {:.6} m/s",
        rb.velocity.length()
    );
}
#[test]
fn actual_nodes_merge_and_conserve_mass_and_momentum() {
    let mut scene = scene(default_galaxy());
    assert!(catch_unwind(AssertUnwindSafe(|| scene.join())).is_err());
    scene.advance(40.0);
    let a = scene.fleet.snapshot(&scene.first);
    let b = scene.fleet.snapshot(&scene.second);
    let mass = a.mass_kg + b.mass_kg;
    let momentum = a.velocity * a.mass_kg + b.velocity * b.mass_kg;
    let id = scene.join();
    assert_eq!(scene.fleet.vessel_ids().len(), 1);
    let merged = scene.fleet.snapshot(&id);
    assert!((merged.mass_kg - mass).abs() < 1e-9);
    assert!((merged.velocity * mass - momentum).length() < 1e-5);
    assert_eq!(scene.fleet.part_snapshots(&id).len(), 4);
    scene.advance(10.0);
    assert!(scene.fleet.snapshot(&id).position.is_finite());
}
