use crate::{Input, maximum};
use glam::{DMat3, DQuat, DVec3};
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc};
use void_assembly::demo_craft;
use void_frames::SplitPosition;
use void_frames::{BodyId, BodyStates};
use void_landing::{FrameState, aurelia, planet_ephemeris};
use void_multiscale::{FrameEphemeris, wide_world};
use void_orbit::{AdvanceOutcome, EphemerisSource, PropagationRun, VesselPropagator, VesselState};
use void_vessels::{Fleet, FleetOptions, VesselMode, pod_tank};
fn initial(f: &Fleet, body: usize, altitude: f64) -> FrameState {
    let (p, v) = f.ephemeris.body_state(BodyId(body), 0.0);
    let b = &f.ephemeris.bodies()[body];
    let r = b.radius_meters + altitude;
    FrameState {
        position: p + DVec3::X * r,
        velocity: v + DVec3::Y * (b.gm / r).sqrt(),
    }
}
fn normal() -> (Fleet, usize) {
    let (e, body) = planet_ephemeris(&aurelia());
    (Fleet::new(e, 0.0, vec![], FleetOptions::default()), body)
}
fn distant(cells: [i128; 3]) -> (Fleet, usize) {
    let world = Rc::new(RefCell::new(wide_world(SplitPosition::new(
        DVec3::new(123.125, -9.25, 7.0),
        cells,
    ))));
    let e = FrameEphemeris::new(world, "Beryl");
    let body = e
        .bodies()
        .iter()
        .position(|b| b.id == "Beryl/planet")
        .unwrap();
    (Fleet::new(e, 0.0, vec![], FleetOptions::default()), body)
}
fn momentum(f: &Fleet) -> (DVec3, DVec3) {
    let ids = f.vessel_ids();
    let ss: Vec<_> = ids.iter().map(|id| f.snapshot(id)).collect();
    let rr: Vec<_> = ids.iter().map(|id| f.relative(id, &ids[0])).collect();
    let m = ss.iter().map(|s| s.mass_kg).sum::<f64>();
    let c = ss
        .iter()
        .zip(&rr)
        .fold(DVec3::ZERO, |sum, (s, r)| sum + r.position * s.mass_kg)
        / m;
    let v = ss
        .iter()
        .zip(&rr)
        .fold(DVec3::ZERO, |sum, (s, r)| sum + r.velocity * s.mass_kg)
        / m;
    let linear = ss
        .iter()
        .fold(DVec3::ZERO, |sum, s| sum + s.velocity * s.mass_kg);
    let angular = ss.iter().zip(&rr).fold(DVec3::ZERO, |sum, (s, r)| {
        let rot = DMat3::from_quat(s.rotation);
        let inertia = DMat3::from_cols_array(&f.inertia(&s.id)).transpose();
        sum + rot * inertia * rot.transpose() * s.angular_velocity
            + (r.position - c).cross(r.velocity - v) * s.mass_kg
    });
    (linear, angular)
}
fn instantaneous(
    f: &mut Fleet,
    operation: impl FnOnce(&mut Fleet),
    linear_limit: f64,
    angular_limit: f64,
) -> Value {
    let before: Vec<_> = f
        .vessel_ids()
        .iter()
        .flat_map(|id| f.part_snapshots(id))
        .collect();
    let (p, l) = momentum(f);
    let fuel: Vec<_> = before
        .iter()
        .map(|part| (part.id.clone(), f.fuel(&part.id)))
        .collect();
    let mass: f64 = f.vessel_ids().iter().map(|id| f.snapshot(id).mass_kg).sum();
    operation(f);
    let (pa, la) = momentum(f);
    let after: Vec<_> = f
        .vessel_ids()
        .iter()
        .flat_map(|id| f.part_snapshots(id))
        .collect();
    assert_eq!(before.len(), after.len(), "part count changed");
    assert_eq!(
        mass,
        f.vessel_ids()
            .iter()
            .map(|id| f.snapshot(id).mass_kg)
            .sum::<f64>(),
        "mass changed"
    );
    let (mut position, mut rotation) = (0.0_f64, 0.0_f64);
    for part in before {
        let a = after
            .iter()
            .find(|a| a.id == part.id)
            .expect("part disappeared");
        position = maximum(position, (a.position - part.position).length());
        rotation = maximum(rotation, a.rotation.angle_between(part.rotation));
    }
    for (id, value) in fuel {
        assert_eq!(
            f.fuel(&id),
            value,
            "fuel changed during instant topology operation"
        );
    }
    let dp = (pa - p).length();
    let dl = (la - l).length();
    // Same absolute gates as the owning Fleet tests. Only the instant operation is compared:
    // gravity, propulsion, contact impulses and fuel consumption are not conservation claims.
    assert!(
        dp < linear_limit,
        "linear momentum error {dp} >= {linear_limit}"
    );
    assert!(
        dl < angular_limit,
        "angular momentum error {dl} >= {angular_limit}"
    );
    assert!(position < 5e-5, "part pose position error {position}");
    assert!(rotation < 1e-6, "part pose rotation error {rotation}");
    json!({"linear_momentum_error":dp,"angular_momentum_error":dl,"position_m":position,"rotation_rad":rotation})
}
pub fn check(input: &Input) -> Value {
    match input {
        Input::Separate {
            altitude,
            rotation,
            velocity,
            spin,
        } => {
            let (mut f, body) = normal();
            let mut s = initial(&f, body, *altitude);
            s.velocity += *velocity;
            f.launch(&demo_craft(), s, *rotation, *spin);
            let metrics = instantaneous(
                &mut f,
                |f| {
                    f.decouple("v1/p4");
                },
                0.1,
                0.01,
            );
            assert_eq!(f.vessel_ids().len(), 2, "separation did not create a child");
            metrics
        }
        Input::Join {
            rotation,
            velocity,
            spin,
            gap,
            distant: far,
            cells,
        } => {
            assert!((0.0..=0.25).contains(gap), "debug join capture range");
            let (mut f, body) = if *far { distant(*cells) } else { normal() };
            let s = initial(&f, body, 400_000.0);
            let craft = pod_tank("seam join");
            let a = f.launch(&craft, s, *rotation, *spin);
            let node = f.node_frame("v1/p2", "bottom").0;
            let local = rotation.conjugate() * (node - s.position);
            let rb = *rotation * DQuat::from_rotation_x(std::f64::consts::PI);
            let sb = FrameState {
                position: node - rb * local + *rotation * DVec3::X * *gap,
                velocity: s.velocity + *velocity,
            };
            let b = f.launch(&craft, sb, rb, -*spin);
            f.advance(0.0);
            assert_eq!(f.snapshot(&a).mode, VesselMode::Bubble);
            assert_eq!(f.snapshot(&b).mode, VesselMode::Bubble);
            let distance =
                (f.node_frame("v1/p2", "bottom").0 - f.node_frame("v2/p2", "bottom").0).length();
            assert!(distance <= 0.25, "join node distance {distance}");
            let metrics = instantaneous(
                &mut f,
                |f| {
                    f.join("v1/p2", "bottom", "v2/p2", "bottom");
                },
                0.1,
                0.005,
            );
            assert_eq!(f.vessel_ids(), ["v1"]);
            assert!(!f.free_nodes("v1").iter().any(|n| n.node == "bottom"));
            metrics
        }
        Input::Encounter {
            distance,
            speed,
            miss,
        } => {
            let (mut f, body) = normal();
            let a = initial(&f, body, 400_000.0);
            let b = FrameState {
                position: a.position + DVec3::new(*miss, 0.0, -*distance),
                velocity: a.velocity + DVec3::Z * *speed,
            };
            let mut refs = vec![];
            for s in [a, b] {
                let id = f.launch(&pod_tank("coast"), s, DQuat::IDENTITY, DVec3::ZERO);
                refs.push((
                    id,
                    PropagationRun::new(VesselState {
                        time: 0.0,
                        position: s.position,
                        velocity: s.velocity,
                        mass_kg: 1120.0,
                    }),
                ));
            }
            let mut prop = VesselPropagator::new(&f.ephemeris, f.options.tolerances);
            let (mut p, mut v) = (0.0_f64, 0.0_f64);
            // Measure intermediate samples as well as final owner. A final-only comparison can miss
            // a discontinuity that is later cancelled by another handoff.
            for _ in 0..70 {
                f.advance(10.0);
                let time = f.time();
                for (id, run) in &mut refs {
                    assert_eq!(
                        prop.advance(&mut f.ephemeris, run, time, 100_000, None, None),
                        AdvanceOutcome::Reached
                    );
                    let actual = f.snapshot(id);
                    p = maximum(p, (actual.position - run.state().position).length());
                    v = maximum(v, (actual.velocity - run.state().velocity).length());
                }
            }
            let transitions: Vec<_> = f
                .events
                .iter()
                .filter(|e| e.vessel == "v1" && e.from.is_some())
                .collect();
            assert!(
                transitions.iter().any(|e| e.to == Some(VesselMode::Bubble)),
                "no encounter entry"
            );
            assert_eq!(
                f.snapshot("v1").mode,
                VesselMode::Orbit,
                "no encounter exit"
            );
            assert!(p < 0.03, "encounter position error {p}");
            assert!(v < 3e-5, "encounter velocity error {v}");
            json!({"position_m":p,"velocity_m_s":v,"owner_transitions":transitions.len()})
        }
        _ => unreachable!(),
    }
}
