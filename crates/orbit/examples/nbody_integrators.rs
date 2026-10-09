//! Accuracy/time experiment only. Does not replace the game's integration method.
use glam::DVec3;
use std::time::Instant;
use void_orbit::*;
fn force(q: &[DVec3], gm: &[f64], a: &mut [DVec3]) {
    a.fill(DVec3::ZERO);
    for i in 0..q.len() {
        for j in i + 1..q.len() {
            let d = q[j] - q[i];
            let r2 = d.x * d.x + d.y * d.y + d.z * d.z;
            assert!(r2 > 0.0);
            let inv = 1.0 / (r2 * r2.sqrt());
            a[i] += d * (gm[j] * inv);
            a[j] -= d * (gm[i] * inv);
        }
    }
}
fn run(
    system: &BuiltSystem,
    h: f64,
    steps: usize,
    weights: &[f64],
) -> (Vec<DVec3>, Vec<DVec3>, f64) {
    let mut q = system.positions.clone();
    let mut v = system.velocities.clone();
    let gm: Vec<_> = system.bodies.iter().map(|b| b.gm).collect();
    let mut a = vec![DVec3::ZERO; q.len()];
    let mut c = a.clone();
    force(&q, &gm, &mut a);
    let start = Instant::now();
    for _ in 0..steps {
        for w in weights {
            let half = 0.5 * w * h;
            let drift = w * h;
            for i in 0..q.len() {
                v[i] += a[i] * half;
                for k in 0..3 {
                    let y = v[i][k] * drift - c[i][k];
                    let t = q[i][k] + y;
                    c[i][k] = (t - q[i][k]) - y;
                    q[i][k] = t;
                }
            }
            force(&q, &gm, &mut a);
            for i in 0..q.len() {
                v[i] += a[i] * half;
            }
        }
    }
    assert!(q.iter().chain(&v).all(|p| p.is_finite()));
    (q, v, start.elapsed().as_secs_f64())
}
fn energy(q: &[DVec3], v: &[DVec3], b: &[CelestialBody]) -> f64 {
    let mut e = 0.0;
    for i in 0..q.len() {
        e += 0.5 * b[i].gm * v[i].length_squared();
        for j in i + 1..q.len() {
            e -= b[i].gm * b[j].gm / (q[j] - q[i]).length();
        }
    }
    e
}
fn main() {
    let system = build_system(&expanded_sol());
    let h = suggested_step_seconds(&system.bodies, 256.0);
    let eighth = yoshida8_sequence();
    let root = 2f64.cbrt();
    let fourth = [1.0 / (2.0 - root), -root / (2.0 - root), 1.0 / (2.0 - root)];
    let verlet = [1.0];
    let total = 832;
    let (reference, rv, ref_time) = run(&system, h / 8.0, total * 8, &eighth);
    let initial = energy(&system.positions, &system.velocities, &system.bodies);
    println!(
        "{}",
        serde_json::json!({"reference":"Yoshida8 h/8","seconds":ref_time,"duration_seconds":total as f64*h})
    );
    for (name, sequence) in [
        ("Yoshida8", eighth.as_slice()),
        ("Yoshida4", fourth.as_slice()),
        ("Verlet2", verlet.as_slice()),
    ] {
        for factor in [0.0625, 0.25, 1.0, 2.0, 4.0, 8.0, 16.0] {
            let steps = (total as f64 / factor) as usize;
            let (q, v, seconds) = run(&system, h * factor, steps, sequence);
            let mut maxp: f64 = 0.0;
            let mut maxv: f64 = 0.0;
            let mut relative: f64 = 0.0;
            let mut worst = "";
            for i in 0..q.len() {
                maxp = maxp.max((q[i] - reference[i]).length());
                maxv = maxv.max((v[i] - rv[i]).length());
                if let Some(parent) = system.bodies[i].parent_index {
                    let err = ((q[i] - q[parent]) - (reference[i] - reference[parent])).length();
                    if err > relative {
                        relative = err;
                        worst = &system.bodies[i].id;
                    }
                }
            }
            println!(
                "{}",
                serde_json::json!({"method":name,"step_factor":factor,"step_seconds":h*factor,"force_evaluations":steps*sequence.len(),"seconds":seconds,"max_position_m":maxp,"max_velocity_m_s":maxv,"max_parent_relative_position_m":relative,"worst_parent_relative_body":worst,"relative_energy_drift":(energy(&q,&v,&system.bodies)-initial)/initial})
            );
        }
    }
}
