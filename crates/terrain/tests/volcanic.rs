use void_terrain::{
    Terrain, TerrainConfig, VolcanicOptions, check_terrain_contract, lattice_directions,
};

#[test]
fn volcanic_contract_and_cell_filtering() {
    let config = TerrainConfig::Volcanic(VolcanicOptions::vesper(6_051_800.0));
    let terrain = Terrain::from_config(&config);
    assert!(check_terrain_contract(&terrain, 10000).is_empty());
    assert_eq!(
        serde_json::from_str::<TerrainConfig>(&serde_json::to_string(&config).unwrap()).unwrap(),
        config
    );
    assert_eq!(terrain.sea_level_meters(), None);
    let mut detail = 0.0;
    for d in lattice_directions(1000) {
        let fine = terrain.sample(d, Some(1.0));
        let coarse = terrain.sample(d, Some(10000.0));
        assert!((0.0..=terrain.max_height_meters).contains(&coarse.0));
        assert!(fine.1.iter().all(|v| *v > 0.05 && *v < 0.25));
        detail += (fine.0 - coarse.0).abs();
    }
    assert!(detail > 1000.0, "cell filtering must affect true geometry");
}

#[test]
#[should_panic(expected = "invalid volcanic cell")]
fn invalid_cell_is_rejected() {
    Terrain::from_config(&TerrainConfig::Volcanic(VolcanicOptions::vesper(
        6_051_800.0,
    )))
    .sample(glam::DVec3::Z, Some(f64::NAN));
}

#[test]
fn authored_volcanic_landmarks_are_sunlit_distinct_and_real_relief() {
    let options = VolcanicOptions::vesper(6_051_800.0);
    let field = void_terrain::Volcanic::new(&options);
    let sun = glam::DVec3::X;
    let shield = field.sunlit_shield_rim(sun);
    let upland = field.sunlit_upland(sun);
    for d in [shield, upland] {
        assert!(d.dot(sun) > 0.45);
        assert!((d.length() - 1.0).abs() < 1e-12);
        let h = field.sample(d, 10.0).0;
        assert!(h > 4000.0);
        let tangent = glam::DVec3::Z.cross(d).normalize();
        let nearby = (d + tangent * 0.003).normalize();
        assert!((field.sample(nearby, 10.0).0 - h).abs() > 10.0);
    }
    assert!((shield - upland).length() > 0.01);
}
