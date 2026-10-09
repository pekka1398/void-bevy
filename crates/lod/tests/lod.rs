//! The LOD core's geometry, meshes and selection against what they must satisfy.

use std::sync::Arc;

use glam::DVec3;
use void_lod::{
    DemoTerrain, FACE_EDGES, LodView, PlanetLod, PlanetLodOptions, TileKey, TileMeshData,
    TileMeshOptions, build_tile_mesh, cube_to_sphere, neighbor_key, sphere_to_cube, stitch_edges,
    tile_containing, tiles_around,
};

fn directions() -> Vec<DVec3> {
    // A spiral over the sphere, plus the awkward cases: cube corners, edges and face centres.
    let mut out: Vec<DVec3> = (0..500)
        .map(|i| {
            let z = 1.0 - (i as f64 + 0.5) / 250.0;
            let a = i as f64 * 2.399_963;
            let r = (1.0 - z * z).sqrt();
            DVec3::new(r * a.cos(), r * a.sin(), z)
        })
        .collect();
    out.extend([
        DVec3::ONE.normalize(),
        DVec3::new(1.0, -1.0, 0.0).normalize(),
        DVec3::X,
        -DVec3::Z,
    ]);
    out
}

#[test]
fn cube_and_sphere_round_trip_and_tiles_nest() {
    for d in directions() {
        let (face, u, v) = sphere_to_cube(d);
        assert!(u.abs() <= 1.0 && v.abs() <= 1.0);
        assert!((cube_to_sphere(face, u, v) - d).length() < 1e-15, "{d}");
        for level in 1..12 {
            assert_eq!(
                tile_containing(d, level).parent(),
                tile_containing(d, level - 1),
                "{d} at level {level}"
            );
        }
    }
}

#[test]
fn every_neighbour_leads_back() {
    for level in 0..4 {
        for face in 0..6 {
            let side = 1 << level;
            for x in 0..side {
                for y in 0..side {
                    let key = TileKey { face, level, x, y };
                    for e in FACE_EDGES {
                        let n = neighbor_key(key, e);
                        assert_eq!(n.level, level);
                        assert!(
                            FACE_EDGES.iter().any(|&b| neighbor_key(n, b) == key),
                            "{key} across {e:?} to {n} has no way back"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn tiles_around_a_point_include_its_own() {
    let radius = 6_371_000.0;
    for d in directions().into_iter().step_by(25) {
        let point = d * (radius + 120.0);
        let around = tiles_around(point, 600.0, 14, radius);
        assert!(around.contains(&tile_containing(d, 14)), "{d}");
        assert!(around.iter().all(|k| k.level == 14));
    }
}

#[test]
fn a_mesh_has_unit_normals_and_heights_within_its_bounds() {
    let terrain = DemoTerrain::preset("normal");
    let n = 33;
    let options = TileMeshOptions {
        radius_meters: terrain.radius_meters,
        resolution: n,
    };
    let key = tile_containing(DVec3::new(0.3, -0.6, 0.74).normalize(), 5);
    let mesh = build_tile_mesh(key, &terrain, options);
    assert_eq!(mesh.positions.len(), n * n + 4 * n);
    for normal in &mesh.normals {
        let l = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        assert!((l - 1.0).abs() < 1e-5, "normal length {l}");
    }
    for h in &mesh.heights {
        let h = f64::from(*h);
        assert!(h >= mesh.min_height_meters - 1e-3 && h <= mesh.max_height_meters + 1e-3);
    }
    // Stitching to a coarser neighbour moves only that edge's vertices.
    let e = FACE_EDGES[1];
    let coarse = build_tile_mesh(neighbor_key(key, e).parent(), &terrain, options);
    let mut seams = [None; 4];
    seams[e.index()] = Some(&coarse);
    let (positions, _, _) = stitch_edges(&mesh, seams, n);
    let moved = positions
        .iter()
        .zip(&mesh.positions)
        .filter(|(a, b)| a != b)
        .count();
    assert!(moved > 0 && moved <= 2 * n, "{moved} vertices moved");
}

#[test]
fn a_settled_selection_covers_the_sphere_once_and_is_finest_under_the_observer() {
    let radius = 100e3;
    let max_level = 7;
    let mut lod = PlanetLod::new(PlanetLodOptions {
        radius_meters: radius,
        min_surface_height_meters: 0.0,
        max_surface_height_meters: 2000.0,
        occluder_radius_meters: radius,
        lod_surface_band_meters: 2000.0,
        resolution: 33,
        max_level,
        split_distance_ratios: (0..max_level)
            .map(|l| {
                if l < 2 {
                    f64::INFINITY
                } else {
                    4.0 / f64::from(1 << l)
                }
            })
            .collect(),
        retain_frames: 90,
        max_cached_tiles: 50_000,
    });
    let observer = DVec3::new(0.2, 0.9, -0.3).normalize() * (radius + 50.0);
    let view = LodView {
        observer_positions: vec![observer],
        camera: None,
        distance_scale: 1.0,
        horizon_culling: false,
    };
    let mut selection = lod.select(&view);
    for _ in 0..4 * max_level {
        if selection.requests.is_empty() {
            break;
        }
        for request in &selection.requests {
            lod.accept_tile(Arc::new(stub(request.key)));
        }
        selection = lod.select(&view);
    }
    assert!(selection.requests.is_empty(), "selection did not settle");
    let keys: Vec<TileKey> = selection
        .render
        .iter()
        .map(|&code| lod.node(code).expect("rendered tile has a node").key)
        .collect();
    // Each tile covers 4^-level of its face: a gapless, overlap-free cover adds up to six faces.
    let covered: f64 = keys.iter().map(|k| 0.25_f64.powi(k.level as i32)).sum();
    assert!((covered - 6.0).abs() < 1e-12, "covers {covered} faces");
    assert!(keys.contains(&tile_containing(observer.normalize(), max_level)));
}

/// An empty tile: selection reads nothing but its presence.
fn stub(key: TileKey) -> TileMeshData {
    TileMeshData {
        key,
        origin: DVec3::ZERO,
        positions: Vec::new(),
        normals: Vec::new(),
        colors: Vec::new(),
        heights: Vec::new(),
        grid: Vec::new(),
        min_height_meters: 0.0,
        max_height_meters: 0.0,
        error_meters: 0.0,
        skirt_depth_meters: 0.0,
        build_seconds: 0.0,
    }
}
