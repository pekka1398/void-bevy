use glam::DVec3;
use void_terrain::{
    AresOptions, Terrain, TerrainConfig, check_terrain_contract, lattice_directions,
};
fn terrain() -> Terrain {
    Terrain::from_config(&TerrainConfig::Ares(AresOptions::ares(3_389_500.0)))
}
#[test]
fn serialized_contract_and_global_envelope() {
    let t = terrain();
    let json = serde_json::to_string(t.config()).unwrap();
    let restored: TerrainConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(&restored, t.config());
    assert!(check_terrain_contract(&t, 4096).is_empty());
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for d in lattice_directions(100_000) {
        let (h, c) = t.sample(d, t.finest_cell_meters());
        lo = lo.min(h);
        hi = hi.max(h);
        assert!(
            c.into_iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(&v))
        );
        for cell in [1000.0, 60_000.0] {
            let h = t.sample(d, cell).0;
            assert!((0.0..=t.max_height_meters).contains(&h));
        }
    }
    assert!(hi - lo > 18_000.0, "global relief {lo}..{hi}");
}
#[test]
fn provinces_have_real_relief_and_polar_material() {
    let t = terrain();
    let o = AresOptions::ares(t.radius_meters);
    let basin = &o.impact.basins[0];
    let center = DVec3::from_array(basin.direction);
    let rim = (center + center.cross(DVec3::Z).normalize() * basin.radius_meters / t.radius_meters)
        .normalize();
    assert!(t.height(center) < 8000.0 && t.height(rim) - t.height(center) > 4000.0);
    let v = &o.volcanoes[0];
    let d = DVec3::from_array(v.direction);
    let tangent = d.cross(DVec3::Z).normalize();
    let flank = (d + tangent * v.radius_meters / t.radius_meters).normalize();
    assert!(t.height(d) - t.height(flank) > 12_000.0);
    let center = DVec3::from_array(o.canyon_direction);
    let across = center.cross(center.cross(DVec3::Z).normalize());
    let wall = (center + across * 100_000.0 / t.radius_meters).normalize();
    assert!(t.height(wall) - t.height(center) > 3000.0);
    let ice = t.sample(DVec3::Z, t.finest_cell_meters()).1;
    assert!(ice[0] > 0.65 && (ice[0] - ice[2]).abs() < 0.1);
    let dust = t
        .sample(
            DVec3::new(-0.3, 0.7, 0.65).normalize(),
            t.finest_cell_meters(),
        )
        .1;
    assert!(dust[0] > dust[1] && dust[1] > dust[2]);
}
#[test]
fn malformed_configuration_rejected() {
    let mut o = AresOptions::ares(3_389_500.0);
    o.volcanoes[0].caldera_depth_meters = f64::NAN;
    assert!(std::panic::catch_unwind(|| Terrain::from_config(&TerrainConfig::Ares(o))).is_err());
}

#[test]
fn canyon_chart_is_finite_at_both_spin_poles() {
    for pole in [DVec3::Z, DVec3::NEG_Z] {
        let mut o = AresOptions::ares(3_389_500.0);
        o.canyon_direction = pole.to_array();
        let (center, along, across) = o.canyon_frame();
        assert!(along.is_finite() && across.is_finite());
        assert!(center.dot(along).abs() < 1e-12 && along.dot(across).abs() < 1e-12);
        let t = Terrain::from_config(&TerrainConfig::Ares(o));
        for d in [
            pole,
            (pole + along * 0.01).normalize(),
            (pole + across * 0.01).normalize(),
        ] {
            let (h, c) = t.sample(d, 10.0);
            assert!(h.is_finite() && c.into_iter().all(f64::is_finite));
        }
    }
}
