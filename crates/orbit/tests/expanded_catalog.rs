use void_orbit::{GRAVITATIONAL_CONSTANT, build_system, expanded_sol};
#[test]
fn catalog_is_positive_finite_and_preserves_frozen_fixture() {
    let frozen = void_orbit::SystemSpec::from_json(include_str!("../systems/sol.json"));
    assert_eq!(build_system(&frozen).bodies.len(), 15);
    let built = build_system(&expanded_sol());
    assert_eq!(built.bodies.len(), 58);
    for body in &built.bodies {
        assert!(
            body.mass_kg > 0.0 && body.mass_kg.is_finite(),
            "{}",
            body.id
        );
        assert!(body.radius_meters > 0.0 && body.radius_meters.is_finite());
        assert_eq!(body.gm, body.mass_kg * GRAVITATIONAL_CONSTANT);
    }
    for id in [
        "phobos",
        "amalthea",
        "enceladus",
        "miranda",
        "triton",
        "charon",
        "bennu",
        "eris",
        "halley",
        "67p",
    ] {
        assert!(built.bodies.iter().any(|b| b.id == id));
    }
}

#[test]
fn catalog_contract_and_short_ephemeris_are_explicit() {
    use glam::DVec3;
    use void_orbit::{Ephemeris, EphemerisOptions, suggested_step_seconds};
    let spec = expanded_sol();
    let system = build_system(&spec);
    let provenance: serde_json::Value =
        serde_json::from_str(include_str!("../systems/sources/expanded-catalog.json")).unwrap();
    let frozen = build_system(&void_orbit::SystemSpec::from_json(include_str!(
        "../systems/sol.json"
    )));
    let additions = "phobos deimos amalthea mimas enceladus tethys dione rhea hyperion iapetus phoebe miranda ariel umbriel titania oberon proteus triton nereid pluto charon styx nix kerberos hydra ceres vesta pallas hygiea eros bennu ryugu eris haumea makemake quaoar orcus gonggong sedna halley 67p encke halebopp";
    for id in additions.split_whitespace() {
        assert!(system.bodies.iter().any(|b| b.id == id), "missing {id}");
    }
    for body in &system.bodies {
        if !frozen.bodies.iter().any(|b| b.id == body.id) {
            assert!(
                provenance["satellites"].get(&body.id).is_some()
                    || provenance["small_bodies"].get(&body.id).is_some()
                    || provenance["authored"].get(&body.id).is_some(),
                "missing provenance {}",
                body.id
            );
        }
    }
    for (id, parent) in [
        ("phobos", "ares"),
        ("amalthea", "velvet"),
        ("enceladus", "halo"),
        ("miranda", "azure"),
        ("triton", "abyss"),
        ("charon", "pluto"),
    ] {
        let b = system.bodies.iter().find(|b| b.id == id).unwrap();
        assert_eq!(system.bodies[b.parent_index.unwrap()].id, parent);
    }
    let triton = spec
        .root
        .children
        .iter()
        .find(|b| b.id == "abyss")
        .unwrap()
        .children
        .iter()
        .find(|b| b.id == "triton")
        .unwrap();
    let halley = spec
        .root
        .children
        .iter()
        .find(|b| b.id == "halley")
        .unwrap();
    assert!(triton.orbit.unwrap().inclination_radians > std::f64::consts::FRAC_PI_2);
    assert!(halley.orbit.unwrap().inclination_radians > std::f64::consts::FRAC_PI_2);
    assert_eq!(
        system
            .bodies
            .iter()
            .find(|b| b.id == "eris")
            .unwrap()
            .mass_kg,
        1.66e22
    );
    assert_eq!(
        system
            .bodies
            .iter()
            .find(|b| b.id == "haumea")
            .unwrap()
            .mass_kg,
        4.006e21
    );
    let mut ephemeris = Ephemeris::new(
        &system,
        EphemerisOptions {
            step_seconds: suggested_step_seconds(&system.bodies, 256.0),
            chunk_steps: 16,
        },
    );
    let mut p = vec![DVec3::ZERO; system.bodies.len()];
    let mut v = p.clone();
    for time in [1.0, 60.0, 600.0, 3600.0] {
        ephemeris.extend_to(time);
        ephemeris.states_at(time, &mut p, Some(&mut v));
        assert!(p.iter().chain(&v).all(|s| s.is_finite()));
    }
}
