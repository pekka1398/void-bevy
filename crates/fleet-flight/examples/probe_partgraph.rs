//! Temporary A/B probe for the part graph refactor: exact bits of every vessel and part.
use glam::{DQuat, DVec3};
use std::fmt::Write;
use void_fleet_flight::FleetFlight;
use void_vessels::{Fleet, Scenario, VesselControl, create_lab_scene, flat_site};

fn bits(v: DVec3) -> String {
    format!(
        "{:x},{:x},{:x}",
        v.x.to_bits(),
        v.y.to_bits(),
        v.z.to_bits()
    )
}
fn qbits(q: DQuat) -> String {
    format!(
        "{:x},{:x},{:x},{:x}",
        q.x.to_bits(),
        q.y.to_bits(),
        q.z.to_bits(),
        q.w.to_bits()
    )
}
fn dump(tag: &str, f: &Fleet, out: &mut String) {
    writeln!(out, "== {tag} t={:x}", f.time().to_bits()).unwrap();
    for id in f.vessel_ids() {
        let s = f.snapshot(&id);
        writeln!(
            out,
            "v {id} {:?} {:?} p={} v={} q={} w={} m={:x} parts={:?} stages={:?}",
            s.mode,
            s.scene,
            bits(s.position),
            bits(s.velocity),
            qbits(s.rotation),
            bits(s.angular_velocity),
            s.mass_kg.to_bits(),
            s.part_ids,
            f.stages_left(&id)
        )
        .unwrap();
        let t = f.thrust(&id);
        writeln!(
            out,
            "  thrust f={} tq={} flow={:x}",
            bits(t.force),
            bits(t.torque),
            t.flow_kg_per_second.to_bits()
        )
        .unwrap();
        for p in f.part_snapshots(&id) {
            writeln!(
                out,
                "  part {} p={} q={} lp={} lq={} fuel={:x} st={:?} staged={} lit={} firing={}",
                p.id,
                bits(p.position),
                qbits(p.rotation),
                bits(p.local_position),
                qbits(p.local_rotation),
                p.fuel_kg.to_bits(),
                p.stage,
                p.staged,
                p.lit,
                p.firing
            )
            .unwrap();
        }
        for n in f.free_nodes(&id) {
            let (a, d) = f.node_frame(&n.part, &n.node);
            writeln!(
                out,
                "  node {} {} {} {} {}",
                n.part,
                n.node,
                n.size,
                bits(a),
                bits(d)
            )
            .unwrap();
        }
    }
    let mut c = f.connection_snapshots();
    c.sort_by(|a, b| (&a.a, &a.node_a).cmp(&(&b.a, &b.node_a)));
    writeln!(
        out,
        "  connections {:?}",
        c.iter()
            .map(|c| format!("{}:{}-{}:{}", c.a, c.node_a, c.b, c.node_b))
            .collect::<Vec<_>>()
    )
    .unwrap();
}

fn main() {
    let mut out = String::new();
    // Main game: ground launch, ignition, ascent through the hand-off, booster separation, coast.
    let planet = void_landing::aurelia();
    let site = flat_site(&planet);
    let mut sim = FleetFlight::new(planet, &void_assembly::flight_rocket(), site, true);
    sim.sas(true);
    sim.control(VesselControl {
        throttle: 1.0,
        turn: DVec3::ZERO,
    });
    sim.stage();
    for i in 0..600 {
        sim.advance(0.25, false).unwrap();
        if i == 300 {
            sim.control(VesselControl {
                throttle: 1.0,
                turn: DVec3::new(0.0, 0.0, 0.3),
            });
        }
        if i == 320 {
            sim.control(VesselControl {
                throttle: 1.0,
                turn: DVec3::ZERO,
            });
        }
        if i == 560 {
            let split = sim.stage();
            writeln!(out, "staged -> {split:?}").unwrap();
        }
        if i % 20 == 0 {
            dump(&format!("ascent {i}"), &sim.fleet, &mut out);
        }
    }
    sim.control(VesselControl {
        throttle: 0.0,
        turn: DVec3::ZERO,
    });
    sim.advance(0.25, false).unwrap();
    let r = sim.advance(600.0, true);
    writeln!(out, "rails {r:?}").unwrap();
    dump("rails", &sim.fleet, &mut out);
    // Main game in the air: a direct world checkpoint restored mid-ascent flies on as the original.
    let planet = void_landing::aurelia();
    let craft = void_assembly::flight_rocket();
    let mut a = FleetFlight::new(planet.clone(), &craft, site, true);
    a.sas(true);
    a.control(VesselControl {
        throttle: 1.0,
        turn: DVec3::ZERO,
    });
    a.stage();
    for _ in 0..200 {
        a.advance(0.25, false).unwrap();
    }
    let saved = void_fleet_flight::checkpoint::FlightCheckpoint::capture(
        &a,
        void_fleet_flight::session::InitialWorld::new(&planet, &craft, site, true),
    );
    let mut b = saved.restore();
    for i in 0..80 {
        a.advance(0.25, false).unwrap();
        b.advance(0.25, false).unwrap();
        if i % 20 == 0 {
            dump(&format!("air original {i}"), &a.fleet, &mut out);
            dump(&format!("air restored {i}"), &b.fleet, &mut out);
        }
    }
    let mut e = create_lab_scene(Scenario::Encounter);
    for _ in 0..3 {
        let r = e.fleet.advance_on_rails(200.0);
        writeln!(out, "encounter rails {r}").unwrap();
        dump("encounter rails", &e.fleet, &mut out);
    }
    for i in 0..120 {
        e.fleet.advance(1.0 / 60.0);
        if i % 60 == 0 {
            dump(&format!("encounter {i}"), &e.fleet, &mut out);
        }
    }
    // Lab scenes: spinning separation, and a join.
    let mut s = create_lab_scene(Scenario::Separate);
    for _ in 0..2 {
        let split = s.fleet.stage("v1");
        writeln!(out, "separate -> {split:?}").unwrap();
    }
    for i in 0..300 {
        s.fleet.advance(1.0 / 60.0);
        if i % 30 == 0 {
            dump(&format!("separate {i}"), &s.fleet, &mut out);
        }
    }
    let mut j = create_lab_scene(Scenario::Join);
    while (j.fleet.node_frame("v1/p2", "bottom").0 - j.fleet.node_frame("v2/p2", "bottom").0)
        .length()
        > 0.08
        && j.fleet.time() < 60.0
    {
        j.fleet.advance(1.0 / 60.0);
    }
    dump("before join", &j.fleet, &mut out);
    let joined = j.fleet.join("v1/p2", "bottom", "v2/p2", "bottom");
    writeln!(out, "joined {joined}").unwrap();
    for i in 0..240 {
        j.fleet.advance(1.0 / 60.0);
        if i % 30 == 0 {
            dump(&format!("joined {i}"), &j.fleet, &mut out);
        }
    }
    let ck = j.fleet.checkpoint();
    let (ephemeris, _) = void_landing::planet_ephemeris(&j.planet);
    let mut restored = Fleet::from_checkpoint(ephemeris, j.fleet.environment().clone(), ck, None);
    for i in 0..60 {
        restored.advance(1.0 / 60.0);
        j.fleet.advance(1.0 / 60.0);
        if i % 30 == 0 {
            dump(&format!("restored {i}"), &restored, &mut out);
            dump(&format!("kept {i}"), &j.fleet, &mut out);
        }
    }
    print!("{out}");
}
