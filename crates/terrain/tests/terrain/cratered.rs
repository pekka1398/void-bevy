use void_terrain::{
    CrateredOptions, Terrain, TerrainConfig, check_terrain_contract, lattice_directions,
};
fn config() -> TerrainConfig {
    TerrainConfig::Cratered(CrateredOptions {
        name: "Selene".into(),
        radius_meters: 1_737_400.0,
        max_height_meters: 8000.0,
        crater_count: 96,
        crater_radius_radians: 0.16,
        roughness: 0.8,
        seed: 19,
        low_color: [0.08; 3],
        high_color: [0.5; 3],
    })
}
#[test]
fn bounded_continuous_serializable_and_shared_surface() {
    let config = config();
    let encoded = serde_json::to_string(&config).unwrap();
    let restored: TerrainConfig = serde_json::from_str(&encoded).unwrap();
    assert_eq!(config, restored);
    let terrain = Terrain::from_config(&restored);
    assert!(check_terrain_contract(&terrain, 2048).is_empty());
    let heights: Vec<_> = lattice_directions(512)
        .into_iter()
        .map(|d| {
            let rendered = void_lod::SurfaceSampler::sample(&terrain, d, 1000.0);
            assert_eq!(rendered.height_meters, terrain.height(d));
            terrain.height(d)
        })
        .collect();
    assert!(
        heights.iter().copied().reduce(f64::max).unwrap()
            - heights.iter().copied().reduce(f64::min).unwrap()
            > 1500.0
    );
}
#[test]
fn invalid_parameters_rejected() {
    let TerrainConfig::Cratered(mut options) = config() else {
        unreachable!()
    };
    options.roughness = f64::NAN;
    assert!(
        std::panic::catch_unwind(|| Terrain::from_config(&TerrainConfig::Cratered(options)))
            .is_err()
    );
}
