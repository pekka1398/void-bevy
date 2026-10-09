//! Binary input/oracle for the optional native CUDA probe, independent of the game backend.
use glam::DVec3;
use std::{fs::File, io::Write};
use void_orbit::*;
fn main() {
    let path = std::env::args().nth(1).expect("output path");
    let system = build_system(&expanded_sol());
    let n = system.bodies.len();
    let refinement: usize = std::env::args()
        .nth(2)
        .unwrap_or("1".into())
        .parse()
        .unwrap();
    assert!((1..=16).contains(&refinement));
    let base_h = suggested_step_seconds(&system.bodies, 256.0);
    let h = base_h / refinement as f64;
    let base_steps: usize = std::env::args()
        .nth(3)
        .map(|x| x.parse().unwrap())
        .unwrap_or((86400.0 / base_h).ceil() as usize);
    assert!(base_steps <= 1000);
    let steps = base_steps * refinement;
    let mut f = File::create(path).unwrap();
    for x in [n as u32, steps as u32] {
        f.write_all(&x.to_le_bytes()).unwrap();
    }
    let mut write = |x: f64| f.write_all(&x.to_le_bytes()).unwrap();
    write(h);
    for w in yoshida8_sequence() {
        write(w);
    }
    for b in &system.bodies {
        write(b.gm);
    }
    for values in [&system.positions, &system.velocities] {
        for p in values {
            for x in p.to_array() {
                write(x);
            }
        }
    }
    let mut e = Ephemeris::new(
        &system,
        EphemerisOptions {
            step_seconds: h,
            chunk_steps: 1024,
        },
    );
    e.set_acceleration_backend(AccelerationBackend::ScalarReference);

    let mut p = vec![DVec3::ZERO; n];
    let mut v = p.clone();
    for k in 1..=steps {
        e.extend_to(k as f64 * h);
        e.current_states(&mut p, &mut v);
        for i in 0..n {
            for x in p[i].to_array().into_iter().chain(v[i].to_array()) {
                write(x);
            }
        }
    }
}
