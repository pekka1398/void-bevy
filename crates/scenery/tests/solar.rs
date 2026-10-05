use void_scenery::solar::{RingRecipe, SurfaceRecipe};
#[test]
fn gas_patterns_are_spatial_and_stars_emit() {
    let gas = SurfaceRecipe::GasEnvelope {
        low: [0.1; 3],
        high: [0.8; 3],
        bands: 18.0,
        turbulence: 0.8,
        storm: 1.0,
    };
    gas.validate();
    let colors: Vec<_> = void_terrain::lattice_directions(1024)
        .into_iter()
        .map(|d| gas.color(d))
        .collect();
    assert!(colors.iter().flatten().all(|v| v.is_finite() && *v >= 0.0));
    assert!(colors.iter().any(|c| (c[0] - colors[0][0]).abs() > 0.3));
    let star = SurfaceRecipe::EmissiveStar {
        color: [1.0, 0.6, 0.3],
        radiance: 6.0,
        granulation: 0.6,
    };
    star.validate();
    assert!(star.color(glam::DVec3::X)[0] > 1.0);
    assert_eq!(
        gas,
        serde_json::from_str::<SurfaceRecipe>(&serde_json::to_string(&gas).unwrap()).unwrap()
    );
}
#[test]
fn invalid_recipes_reject_nan_and_inside_rings() {
    let invalid = SurfaceRecipe::GasEnvelope {
        low: [f32::NAN; 3],
        high: [0.5; 3],
        bands: 8.0,
        turbulence: 0.0,
        storm: 0.0,
    };
    assert!(std::panic::catch_unwind(|| invalid.validate()).is_err());
    let ring = RingRecipe {
        inner_radius: 0.9,
        outer_radius: 2.0,
        color: [0.5; 3],
        opacity: 0.8,
    };
    assert!(std::panic::catch_unwind(|| ring.validate()).is_err());
}

#[test]
fn unrepresentable_hdr_star_is_rejected() {
    let star = SurfaceRecipe::EmissiveStar {
        color: [1.0; 3],
        radiance: 65504.0,
        granulation: 1.0,
    };
    assert!(std::panic::catch_unwind(|| star.validate()).is_err());
}
