//! void-aero against lab/aerodynamics on the lab's own outputs (`golden/aero.ts`). Pure arithmetic
//! matches bit for bit; values through V8's `sin`, `cos` and `pow` agree to a few ulps, and the
//! reentry carries those ulps through 400 s of adaptive integration. Each comparison prints its
//! largest relative difference.

use std::cell::RefCell;

use glam::{DQuat, DVec3};
use serde_json::Value;
use void_aero::*;

fn golden() -> Value {
    let path = format!("{}/tests/golden/aero.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(&path).expect(&path)).expect(&path)
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn v3(v: &Value) -> DVec3 {
    DVec3::new(f(&v[0]), f(&v[1]), f(&v[2]))
}

fn quat(v: &Value) -> DQuat {
    DQuat::from_xyzw(f(&v[0]), f(&v[1]), f(&v[2]), f(&v[3]))
}

/// Compares numbers at a relative tolerance (against the larger magnitude, with `floor` as the
/// smallest scale) and remembers the worst relative difference seen.
struct Compare {
    name: &'static str,
    rel: f64,
    floor: f64,
    worst: RefCell<(f64, String)>,
    count: RefCell<usize>,
}

impl Compare {
    fn new(name: &'static str, rel: f64, floor: f64) -> Self {
        Self {
            name,
            rel,
            floor,
            worst: RefCell::new((0.0, String::new())),
            count: RefCell::new(0),
        }
    }

    fn num(&self, label: &str, got: f64, want: f64) {
        *self.count.borrow_mut() += 1;
        if got == want {
            return;
        }
        let scale = got.abs().max(want.abs()).max(self.floor);
        let d = (got - want).abs() / scale;
        assert!(
            d <= self.rel,
            "{}: {label}: {got} vs lab {want} (relative {d:e})",
            self.name
        );
        let mut worst = self.worst.borrow_mut();
        if d > worst.0 {
            *worst = (d, label.to_string());
        }
    }

    fn vec(&self, label: &str, got: DVec3, want: &Value) {
        let w = v3(want);
        // Components share the vector's scale.
        let scale = got.abs().max_element().max(w.abs().max_element());
        for (k, (g, w)) in got.to_array().into_iter().zip(w.to_array()).enumerate() {
            self.num_scaled(&format!("{label}[{k}]"), g, w, scale);
        }
    }

    fn num_scaled(&self, label: &str, got: f64, want: f64, scale: f64) {
        *self.count.borrow_mut() += 1;
        if got == want {
            return;
        }
        let d = (got - want).abs() / scale.max(self.floor);
        assert!(
            d <= self.rel,
            "{}: {label}: {got} vs lab {want} (relative {d:e})",
            self.name
        );
        let mut worst = self.worst.borrow_mut();
        if d > worst.0 {
            *worst = (d, label.to_string());
        }
    }

    fn report(&self) {
        let (d, label) = &*self.worst.borrow();
        if *d == 0.0 {
            eprintln!(
                "{}: {} values, all bit for bit",
                self.name,
                self.count.borrow()
            );
        } else {
            eprintln!(
                "{}: {} values, worst relative {d:.1e} at {label}",
                self.name,
                self.count.borrow()
            );
        }
    }
}

fn vehicle(id: &str) -> Vehicle {
    match id {
        "aircraft" => aircraft(),
        "rocket" => demo_rocket(),
        "capsule" => capsule(true, DEFAULT_ABLATOR_KG),
        "bare" => capsule(false, DEFAULT_ABLATOR_KG),
        _ => panic!("vehicle {id}"),
    }
}

#[test]
fn atmosphere() {
    let g = golden();
    let earth = Atmosphere::earth();
    let c = Compare::new("atmosphere", 4e-16, 0.0);
    for s in g["atmosphere"].as_array().unwrap() {
        let h = f(&s["altitude"]);
        let a = earth.sample(h);
        let got = [
            a.density,
            a.pressure_pa,
            a.temperature_k,
            a.sound_speed,
            a.viscosity,
        ];
        for (k, (got, want)) in got.iter().zip(s["air"].as_array().unwrap()).enumerate() {
            c.num(&format!("{h} m [{k}]"), *got, f(want));
        }
    }
    c.report();
}

#[test]
fn wing_polar_matches() {
    let g = golden();
    let plane = aircraft();
    let wings: Vec<WingAero> = ["wing-left", "fin"]
        .iter()
        .map(|id| {
            match &plane.parts[plane.part_index(id)]
                .aero
                .as_ref()
                .unwrap()
                .shape
            {
                AeroShape::Wing(w) => w.clone(),
                AeroShape::Body(_) => panic!("{id} is a wing"),
            }
        })
        .collect();
    let c = Compare::new("wing polar", 1e-14, 1e-3);
    for s in g["polar"].as_array().unwrap() {
        let wing = &wings[s["wing"].as_u64().unwrap() as usize];
        let (alpha, mach) = (f(&s["alpha"]), f(&s["mach"]));
        let p = wing_polar(wing, alpha, mach);
        let label = format!("alpha {alpha} mach {mach}");
        for (k, got) in [p.cl, p.cd, p.stall].into_iter().enumerate() {
            c.num(&format!("{label} [{k}]"), got, f(&s["out"][k]));
        }
    }
    c.report();
}

#[test]
fn mass_properties_match() {
    let g = golden();
    let c = Compare::new("mass properties", 0.0, 0.0);
    for s in g["mass"].as_array().unwrap() {
        let id = s["vehicle"]
            .as_str()
            .unwrap_or_else(|| s["id"].as_str().unwrap());
        let v = vehicle(id);
        let full = resources(&v);
        let mut empty = full.clone();
        empty.fuel.iter_mut().for_each(|x| *x = 0.0);
        for (state, key) in [(&full, "full"), (&empty, "empty")] {
            let m = mass_properties(&v, state);
            let got = [
                m.mass,
                m.center.x,
                m.center.y,
                m.center.z,
                m.inertia.x,
                m.inertia.y,
                m.inertia.z,
            ];
            for (k, got) in got.into_iter().enumerate() {
                c.num(&format!("{id} {key} [{k}]"), got, f(&s[key][k]));
            }
        }
    }
    c.report();
}

#[test]
fn forces_and_heat_loads_match() {
    let g = golden();
    let earth = Atmosphere::earth();
    let forces = Compare::new("forces", 1e-13, 1e-9);
    let flow = Compare::new("element flow", 1e-13, 1e-9);
    let heat = Compare::new("heat loads", 1e-13, 1e-9);
    for (n, s) in g["loads"].as_array().unwrap().iter().enumerate() {
        let id = s["vehicle"].as_str().unwrap();
        let v = vehicle(id);
        let data = resources(&v);
        let center = mass_properties(&v, &data).center;
        let state = AeroState {
            center: v3(&s["center"]),
            velocity: v3(&s["velocity"]),
            rotation: quat(&s["rotation"]),
            angular_velocity: v3(&s["angularVelocity"]),
        };
        let wind = v3(&s["wind"]);
        let controls = Controls {
            elevator: f(&s["controls"][0]),
            aileron: f(&s["controls"][1]),
            rudder: f(&s["controls"][2]),
        };
        let air = earth.sample(f(&s["altitude"]));
        let out = aerodynamic_forces(&aero_elements(&v, center), &state, &air, wind, &controls);
        let at = format!("{id} case {n}");
        forces.vec(&format!("{at} force"), out.force, &s["force"]);
        forces.vec(&format!("{at} torque"), out.torque, &s["torque"]);
        for (k, got) in [out.q_pa, out.speed, out.mach].into_iter().enumerate() {
            flow.num(&format!("{at} flow[{k}]"), got, f(&s["flow"][k]));
        }
        let want = s["elements"].as_array().unwrap();
        assert_eq!(out.elements.len(), want.len(), "{at} elements");
        for (e, w) in out.elements.iter().zip(want) {
            assert_eq!(e.id, w["id"].as_str().unwrap());
            let at = format!("{at} {}", e.id);
            forces.vec(&format!("{at} point"), e.point, &w["point"]);
            for (name, got) in [("force", e.force), ("moment", e.moment)] {
                forces.vec(&format!("{at} {name}"), got, &w[name]);
            }
            // Lift is the force less its drag part; compare both on the force's scale.
            let scale = e.force.abs().max_element();
            for (name, got) in [("drag", e.drag), ("lift", e.lift)] {
                for (k, (g, w)) in got
                    .to_array()
                    .into_iter()
                    .zip(v3(&w[name]).to_array())
                    .enumerate()
                {
                    forces.num_scaled(&format!("{at} {name}[{k}]"), g, w, scale);
                }
            }
            let got = [
                e.speed,
                e.q_pa,
                e.mach,
                e.alpha_radians,
                e.stall,
                e.cl,
                e.cd,
            ];
            for (k, got) in got.into_iter().enumerate() {
                flow.num(&format!("{at} flow[{k}]"), got, f(&w["flow"][k]));
            }
        }
        let loads = evaluate_vehicle(&v, &data, &state, &air, wind, &controls, 200.0);
        for (k, (h, w)) in loads
            .heat
            .iter()
            .zip(s["heat"].as_array().unwrap())
            .enumerate()
        {
            let at = format!("{at} part {k}");
            assert_eq!(
                h.env.exposed,
                w["exposed"].as_bool().unwrap(),
                "{at} exposed"
            );
            heat.num(&format!("{at} exposure"), h.env.exposure, f(&w["exposure"]));
            heat.num(&format!("{at} speed"), h.env.speed, f(&w["speed"]));
            let l = &h.load;
            let got = [
                l.aerodynamic_w,
                l.convection_w,
                l.radiation_w,
                l.conduction_w,
                l.flux_wm2,
            ];
            for (j, got) in got.into_iter().enumerate() {
                heat.num(&format!("{at} load[{j}]"), got, f(&w["load"][j]));
            }
        }
    }
    forces.report();
    flow.report();
    heat.report();
}

#[test]
fn thermal_steps_match() {
    let g = golden();
    let earth = Atmosphere::earth();
    let shield = capsule(true, DEFAULT_ABLATOR_KG).parts[0].thermal;
    let mut small = shield;
    small.ablator = Some(Ablator {
        mass_kg: 0.001,
        activation_k: 1100.0,
        latent_j_kg: 12e6,
    });
    let hot = |air: Air, speed: f64, background_k: f64| HeatEnvironment {
        air,
        speed,
        exposed: true,
        exposure: 1.0,
        background_k,
    };
    let runs = [
        (
            shield,
            300.0,
            hot(earth.sample(30000.0), 7000.0, 180.0),
            0.2,
            0.0,
        ),
        (
            small,
            1100.0,
            hot(earth.sample(30000.0), 7000.0, 180.0),
            0.2,
            0.0,
        ),
        (
            aircraft().parts[0].thermal,
            288.15,
            hot(earth.sample(700.0), 70.0, 250.0),
            1.0 / 120.0,
            0.0,
        ),
        (
            capsule(true, DEFAULT_ABLATOR_KG).parts[1].thermal,
            500.0,
            HeatEnvironment {
                air: Atmosphere::Vacuum.sample(0.0),
                speed: 0.0,
                exposed: false,
                exposure: 0.0,
                background_k: 3.0,
            },
            1.7,
            300.0,
        ),
    ];
    let c = Compare::new("thermal steps", 1e-13, 1e-6);
    for ((spec, start, env, dt, core), want) in runs.iter().zip(g["thermal"].as_array().unwrap()) {
        let name = want["name"].as_str().unwrap();
        let mut t = thermal_state(spec, *start);
        for (k, step) in want["steps"].as_array().unwrap().iter().enumerate() {
            let b = advance_thermal(spec, &mut t, env, *dt, *core);
            let at = format!("{name} step {k}");
            let state = [
                t.skin_k,
                t.core_k,
                t.ablator_kg,
                if t.failed { 1.0 } else { 0.0 },
            ];
            for (j, got) in state.into_iter().enumerate() {
                c.num(&format!("{at} state[{j}]"), got, f(&step["state"][j]));
            }
            let budget = [
                b.incoming_j,
                b.radiation_j,
                b.ablation_j,
                b.core_external_j,
                b.stored_j,
            ];
            // Stored energy is a difference of large totals: compare it on their scale.
            let scale = t.skin_k * spec.skin_capacity_jk + t.core_k * spec.core_capacity_jk;
            for (j, got) in budget.into_iter().enumerate() {
                let want = f(&step["budget"][j]);
                if j == 4 {
                    c.num_scaled(&format!("{at} budget[{j}]"), got, want, scale);
                } else {
                    c.num(&format!("{at} budget[{j}]"), got, want);
                }
            }
        }
    }
    c.report();
}

fn compare_entry(c: &Compare, label: &str, e: &EntryFlight, want: &Value) {
    c.num(&format!("{label} time"), e.time, f(&want["time"]));
    let y = want["y"].as_array().unwrap();
    // Positions on the planet's scale, velocities on the speed's, attitude and spin on 1.
    let r = DVec3::new(f(&y[0]), f(&y[1]), f(&y[2])).length();
    let v = DVec3::new(f(&y[3]), f(&y[4]), f(&y[5])).length();
    for (k, got) in e.y.iter().enumerate() {
        let scale = if k < 3 {
            r
        } else if k < 6 {
            v
        } else {
            1.0
        };
        c.num_scaled(&format!("{label} y[{k}]"), *got, f(&y[k]), scale);
    }
    for (p, (t, w)) in e
        .resources
        .thermal
        .iter()
        .zip(want["thermal"].as_array().unwrap())
        .enumerate()
    {
        for (j, got) in [t.skin_k, t.core_k, t.ablator_kg].into_iter().enumerate() {
            c.num(&format!("{label} part {p} thermal[{j}]"), got, f(&w[j]));
        }
        assert_eq!(t.failed, f(&w[3]) == 1.0, "{label} part {p} failed");
    }
    for (name, got) in [
        ("maxQPa", e.max_q_pa),
        ("maxFluxWm2", e.max_flux_wm2),
        ("maxG", e.max_g),
        ("heatJ", e.heat_j),
    ] {
        c.num(&format!("{label} {name}"), got, f(&want[name]));
    }
    assert_eq!(
        e.accepted_steps,
        want["acceptedSteps"].as_u64().unwrap(),
        "{label} accepted steps"
    );
    let terminal = want["terminal"].as_str();
    assert_eq!(
        e.terminal.is_some(),
        terminal.is_some(),
        "{label} terminal {terminal:?}"
    );
}

#[test]
fn reentry_matches() {
    let g = golden();
    let entry = &g["entry"];
    let c = Compare::new("reentry", 1e-9, 1e-6);
    for run in entry["chunks"].as_array().unwrap() {
        let shield = run["shield"].as_bool().unwrap();
        let mut e = EntryFlight::new(
            capsule(shield, DEFAULT_ABLATOR_KG),
            DEFAULT_ENTRY,
            Atmosphere::earth(),
        );
        for (k, want) in run["samples"].as_array().unwrap().iter().enumerate() {
            if k > 0 {
                e.advance(10.0, 200_000);
            }
            compare_entry(&c, &format!("shield {shield} chunk {k}"), &e, want);
        }
    }
    for run in entry["whole"].as_array().unwrap() {
        let shield = run["shield"].as_bool().unwrap();
        let mut e = EntryFlight::new(
            capsule(shield, DEFAULT_ABLATOR_KG),
            DEFAULT_ENTRY,
            Atmosphere::earth(),
        );
        e.advance(400.0, 200_000);
        compare_entry(&c, &format!("shield {shield} whole"), &e, &run["end"]);
    }
    let mut steep = EntryFlight::new(
        capsule(true, DEFAULT_ABLATOR_KG),
        EntryOptions {
            altitude_meters: 100_000.0,
            speed: 6500.0,
            flight_path_degrees: -12.0,
            angle_of_attack_degrees: 160.0,
            bank_degrees: 25.0,
        },
        Atmosphere::earth(),
    );
    // Starting nose-back, the capsule flips round in thick air near 45 s; the flip magnifies the
    // trigonometry's ulps, so from then on the path is compared physically: it stays within a
    // decimetre and a few milliradians of the lab's.
    let flip = 42.0;
    let mut gaps = (0.0_f64, 0.0_f64, 0.0_f64);
    for (k, want) in entry["steep"].as_array().unwrap().iter().enumerate() {
        if k > 0 {
            steep.advance(5.0, 200_000);
        }
        if steep.time < flip {
            compare_entry(&c, &format!("steep {k}"), &steep, want);
            continue;
        }
        let y = want["y"].as_array().unwrap();
        let p = DVec3::new(f(&y[0]), f(&y[1]), f(&y[2]));
        let v = DVec3::new(f(&y[3]), f(&y[4]), f(&y[5]));
        let q = DQuat::from_xyzw(f(&y[6]), f(&y[7]), f(&y[8]), f(&y[9]));
        gaps.0 = gaps.0.max((steep.position() - p).length());
        gaps.1 = gaps.1.max((steep.velocity() - v).length());
        gaps.2 = gaps.2.max(steep.rotation().angle_between(q));
        let ablator = steep.resources.thermal[0].ablator_kg - f(&want["thermal"][0][2]);
        assert!(
            gaps.0 < 1.0 && gaps.1 < 0.1 && gaps.2 < 0.02 && ablator.abs() < 0.01,
            "steep entry left the lab's path at {} s: {gaps:?}, ablator {ablator}",
            steep.time
        );
        assert_eq!(steep.terminal.is_some(), !want["terminal"].is_null());
    }
    eprintln!(
        "steep entry after the flip: within {:.3} m, {:.4} m/s, {:.4} rad of the lab to {:.0} s",
        gaps.0, gaps.1, gaps.2, steep.time
    );
    c.report();
}

/// Native Rapier is not the lab's WebAssembly build, so the aircraft only follows the lab's
/// path approximately; this prints how far apart they end up and checks they stay close.
#[test]
fn aircraft_follows_the_lab() {
    let g = golden();
    for run in g["flights"].as_array().unwrap() {
        let mode = run["mode"].as_str().unwrap();
        let start = if mode == "cruise" {
            FlightStart::Cruise
        } else {
            FlightStart::Runway
        };
        let mut flight = AircraftFlight::new(aircraft(), start, Atmosphere::earth());
        let (mut worst_p, mut worst_v) = (0.0_f64, 0.0_f64);
        let samples = run["samples"].as_array().unwrap();
        for i in 0..=120 * 30 {
            if i % 60 == 0 {
                let want = &samples[i / 60];
                worst_p = worst_p.max((flight.position() - v3(&want["position"])).length());
                worst_v = worst_v.max((flight.velocity() - v3(&want["velocity"])).length());
                assert_eq!(
                    flight.failure.is_some(),
                    !want["failure"].is_null(),
                    "{mode} at {} s: {:?}",
                    flight.time,
                    flight.failure
                );
            }
            let elevator = if start == FlightStart::Runway && flight.loads.aero.speed > 40.0 {
                0.15
            } else {
                0.0
            };
            flight.step(&FlightCommand {
                controls: Controls {
                    elevator,
                    ..NEUTRAL
                },
                throttle: if start == FlightStart::Runway {
                    1.0
                } else {
                    0.28
                },
                brakes: false,
            });
        }
        let end = samples.last().unwrap();
        eprintln!(
            "{mode}: 30 s, worst position gap {worst_p:.3} m, velocity {worst_v:.4} m/s; end \
             altitude {:.2} m (lab {:.2}), fuel {:.4} kg (lab {:.4})",
            flight.altitude(),
            f(&end["position"][1]),
            flight.fuel_kg(),
            f(&end["fuel"])
        );
        assert!(
            worst_p < 5.0 && worst_v < 1.0,
            "{mode} drifted from the lab"
        );
    }
}
