//! Bounded-memory raw-state accuracy reference; no dense history export.
use glam::DVec3;
use std::{fs, time::Instant};
use void_orbit::*;
fn main() {
    let output = std::env::args().nth(1).expect("output JSON");
    let refinement: usize = std::env::args()
        .nth(2)
        .unwrap_or("8".into())
        .parse()
        .unwrap();
    assert!([1, 8, 16].contains(&refinement));
    let system = build_system(&expanded_sol());
    let base_h = suggested_step_seconds(&system.bodies, 256.0);
    let base_steps = 24640;
    let h = base_h / refinement as f64;
    let steps = base_steps * refinement;
    let mut ep = Ephemeris::new(
        &system,
        EphemerisOptions {
            step_seconds: h,
            chunk_steps: 256,
        },
    );
    ep.set_acceleration_backend(AccelerationBackend::ScalarReference);
    let start = Instant::now();
    for chunk in (0..steps).step_by(256) {
        ep.extend_to((chunk + 256).min(steps) as f64 * h);
        ep.forget_before(ep.end_time() - h * 2.0);
    }
    let seconds = start.elapsed().as_secs_f64();
    let mut p = vec![DVec3::ZERO; system.bodies.len()];
    let mut v = p.clone();
    ep.current_states(&mut p, &mut v);
    let json = serde_json::json!({"refinement":refinement,"base_h":base_h,"base_steps":base_steps,"end_time":ep.end_time(),"seconds":seconds,"retained_bytes":ep.retained_bytes(),"gm":system.bodies.iter().map(|b|b.gm).collect::<Vec<_>>(),"ids":system.bodies.iter().map(|b|&b.id).collect::<Vec<_>>(),"parents":system.bodies.iter().map(|b|b.parent_index).collect::<Vec<_>>(),"q":system.positions.iter().map(|p|p.to_array()).collect::<Vec<_>>(),"v":system.velocities.iter().map(|v|v.to_array()).collect::<Vec<_>>(),"final_q":p.iter().map(|p|p.to_array()).collect::<Vec<_>>(),"final_v":v.iter().map(|v|v.to_array()).collect::<Vec<_>>()});
    fs::write(output, serde_json::to_vec_pretty(&json).unwrap()).unwrap();
    println!(
        "{}",
        serde_json::json!({"seconds":seconds,"steps":steps,"retained_bytes":ep.retained_bytes(),"refinement":refinement})
    );
}
