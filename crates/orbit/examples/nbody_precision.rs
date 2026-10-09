//! Initial-force precision probe only. These f32 kernels are NOT game integration backends.
use glam::DVec3;
use void_orbit::{SystemSpec, build_system};
fn main() {
    let system = build_system(&SystemSpec::from_json(include_str!(
        "../systems/sol-expanded.json"
    )));
    let n = system.bodies.len();
    let q = &system.positions;
    let mut reference = vec![DVec3::ZERO; n];
    for i in 0..n {
        for j in i + 1..n {
            let d = q[j] - q[i];
            let r2 = d.x * d.x + d.y * d.y + d.z * d.z;
            let inv = 1.0 / (r2 * r2.sqrt());
            reference[i] += d * (system.bodies[j].gm * inv);
            reference[j] -= d * (system.bodies[i].gm * inv);
        }
    }
    for stable in [false, true] {
        for mode in [
            "absolute_f32",
            "star_relative_f32",
            "f64_pair_delta_then_f32",
        ] {
            let positions: Vec<_> = q
                .iter()
                .map(|p| {
                    if mode == "star_relative_f32" {
                        (*p - q[0]).as_vec3()
                    } else {
                        p.as_vec3()
                    }
                })
                .collect();
            let mut out = vec![glam::Vec3::ZERO; n];
            for i in 0..n {
                for j in i + 1..n {
                    let d = if mode == "f64_pair_delta_then_f32" {
                        (q[j] - q[i]).as_vec3()
                    } else {
                        positions[j] - positions[i]
                    };
                    let r2 = d.x * d.x + d.y * d.y + d.z * d.z;
                    assert!(r2 > 0.0 && r2.is_finite());
                    let inv = 1.0 / (r2 * r2.sqrt());
                    if stable {
                        let direction = d / r2.sqrt();
                        out[i] += direction * (system.bodies[j].gm as f32 / r2);
                        out[j] -= direction * (system.bodies[i].gm as f32 / r2);
                    } else {
                        out[i] += d * (system.bodies[j].gm as f32 * inv);
                        out[j] -= d * (system.bodies[i].gm as f32 * inv);
                    }
                }
            }
            let errors:Vec<_>=out.iter().zip(&reference).enumerate().map(|(i,(got,want))| {
            let absolute=(got.as_dvec3()-want).length();serde_json::json!({"body":system.bodies[i].id,"absolute_m_s2":absolute,"relative":absolute/want.length()})
        }).collect();
            println!(
                "{}",
                serde_json::json!({"mode":mode,"range_safe_formula":stable,"errors":errors})
            );
        }
    }
}
