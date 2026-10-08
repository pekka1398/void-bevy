use glam::DVec3;
use void_terrain::{
    ImpactOptions, Terrain, TerrainConfig, check_terrain_contract, lattice_directions,
};
fn terrain() -> Terrain {
    Terrain::from_config(&TerrainConfig::Impact(ImpactOptions::cinder(2_439_700.0)))
}
#[test]
fn contract_roundtrip_and_angular_seams() {
    let t = terrain();
    let encoded = serde_json::to_string(t.config()).unwrap();
    let restored: TerrainConfig = serde_json::from_str(&encoded).unwrap();
    assert_eq!(&restored, t.config());
    let failures = check_terrain_contract(&t, 4096);
    assert!(failures.is_empty(), "{failures:?}");
    // Both cube boundaries and hash-cell boundaries are traversed by these dense great circles.
    // Include the outer ownership margin, not just the canonical cube edges. The broadest
    // octave's ejecta can extend past dn=0.45; the face visibility cutoff must not truncate it.
    for i in 0..2000 {
        let a = i as f64 * std::f64::consts::TAU / 2000.0;
        let d = DVec3::new(
            0.45,
            (1.0_f64 - 0.45 * 0.45).sqrt() * a.cos(),
            (1.0_f64 - 0.45 * 0.45).sqrt() * a.sin(),
        );
        let n = (d + DVec3::X * 1e-9).normalize();
        assert!((t.height(d) - t.height(n)).abs() < 0.1);
    }
    for axis in [DVec3::X, DVec3::Y, DVec3::Z] {
        for i in 0..2000 {
            let d = (DVec3::ONE + axis * (i as f64 / 1000.0 - 1.0)).normalize();
            let n = (d + axis * 1e-9).normalize();
            assert!((t.height(d) - t.height(n)).abs() < 0.1, "seam at {d}");
        }
    }
}
#[test]
fn basins_young_impacts_and_scale_are_distinct() {
    let o = ImpactOptions::cinder(2_439_700.0);
    let t = terrain();
    let basin = &o.basins[0];
    let center = DVec3::from_array(basin.direction);
    let tangent = center.cross(DVec3::Z).normalize();
    let floor = t.height(center);
    let rim = t.height((center + tangent * (basin.radius_meters / o.radius_meters)).normalize());
    assert!(rim - floor > 1300.0, "basin floor {floor}, rim {rim}");
    let crater = &o.rayed_impacts[0];
    let center = DVec3::from_array(crater.direction);
    let tangent = center.cross(DVec3::Z).normalize();
    let floor =
        t.height((center + tangent * crater.radius_meters / o.radius_meters * 0.4).normalize());
    let rim = t.height((center + tangent * crater.radius_meters / o.radius_meters).normalize());
    assert!(rim - floor > 1000.0);
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut differences = 0;
    for d in lattice_directions(1024) {
        let a = t.sample(d, None);
        let b = t.sample(d, Some(20_000.0));
        min = min.min(a.0);
        max = max.max(a.0);
        if (a.0 - b.0).abs() > 10.0 {
            differences += 1;
        }
        assert!(a.1[0] / a.1[2] < 1.3, "natural colour must stay subtle");
    }
    assert!(max - min > 3500.0);
    assert!(
        differences > 500,
        "coarse geometry must actually filter finer relief"
    );
}
#[test]
fn invalid_data_is_rejected() {
    let mut o = ImpactOptions::cinder(2_439_700.0);
    o.basins[0].direction = [0.0; 3];
    assert!(std::panic::catch_unwind(|| Terrain::from_config(&TerrainConfig::Impact(o))).is_err());
    assert!(std::panic::catch_unwind(|| terrain().sample(DVec3::X, Some(f64::NAN))).is_err());
}

#[test]
fn dense_global_envelope_includes_full_detail() {
    let t = terrain();
    // Full detail, not only the coarse shell: repeated octave overlap must stay in the envelope.
    for d in lattice_directions(100_000) {
        let (height, color) = t.sample(d, None);
        assert!((0.0..=t.max_height_meters).contains(&height));
        assert!(color.iter().all(|v| (0.0..=1.0).contains(v)));
    }
}
