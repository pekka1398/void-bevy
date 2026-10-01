//! The LOD core against the LOD lab (golden data from `golden/lod.ts`).

use std::sync::Arc;

use glam::DVec3;
use serde_json::Value;
use void_lod::{
    FACE_EDGES, FaceEdge, LodCamera, LodView, PlanetLod, PlanetLodOptions, SurfaceSample, TileKey,
    TileMeshData, TileMeshOptions, build_tile_mesh, cube_to_sphere, face_neighbor, neighbor_key,
    sphere_to_cube, stitch_edges, tile_containing, tiles_around,
};

fn golden(name: &str) -> Value {
    let path = format!("{}/tests/golden/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(&path).expect(&path)).expect(&path)
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn u(v: &Value) -> u64 {
    v.as_u64().unwrap_or_else(|| panic!("not an integer: {v}"))
}

fn v3(v: &Value) -> DVec3 {
    let a = v.as_array().unwrap();
    DVec3::new(f(&a[0]), f(&a[1]), f(&a[2]))
}

fn key(v: &Value) -> TileKey {
    let a = v.as_array().unwrap();
    TileKey {
        face: u(&a[0]) as u8,
        level: u(&a[1]) as u32,
        x: u(&a[2]) as u32,
        y: u(&a[3]) as u32,
    }
}

fn edge(name: &str) -> FaceEdge {
    match name {
        "u-" => FaceEdge::UMinus,
        "u+" => FaceEdge::UPlus,
        "v-" => FaceEdge::VMinus,
        "v+" => FaceEdge::VPlus,
        other => panic!("edge {other}"),
    }
}

#[test]
fn geometry_matches_the_lod_lab() {
    let g = golden("geometry");
    for (face, row) in g["adjacency"].as_array().unwrap().iter().enumerate() {
        for (e, lab) in FACE_EDGES.iter().zip(row.as_array().unwrap()) {
            let ours = face_neighbor(face as u8, *e);
            assert_eq!(
                (ours.face as u64, ours.edge, ours.reversed),
                (
                    u(&lab["face"]),
                    edge(lab["edge"].as_str().unwrap()),
                    lab["reversed"].as_bool().unwrap()
                ),
                "face {face} edge {e:?}"
            );
        }
    }
    let mut direction_error = 0.0_f64;
    for c in g["cubeToSphere"].as_array().unwrap() {
        let d = cube_to_sphere(u(&c["face"]) as u8, f(&c["u"]), f(&c["v"]));
        direction_error = direction_error.max((d - v3(&c["d"])).length());
    }
    let mut uv_error = 0.0_f64;
    for c in g["sphereToCube"].as_array().unwrap() {
        let (face, cu, cv) = sphere_to_cube(v3(&c["d"]));
        assert_eq!(
            face as u64,
            u(&c["face"]),
            "sphere to cube face for {}",
            c["d"]
        );
        uv_error = uv_error
            .max((cu - f(&c["u"])).abs())
            .max((cv - f(&c["v"])).abs());
    }
    for c in g["tileContaining"].as_array().unwrap() {
        assert_eq!(
            tile_containing(v3(&c["d"]), u(&c["level"]) as u32),
            key(&c["key"]),
            "tile containing {}",
            c["d"]
        );
    }
    for c in g["tilesAround"].as_array().unwrap() {
        let mut ours = tiles_around(
            v3(&c["point"]),
            f(&c["reach"]),
            u(&c["level"]) as u32,
            6_371_000.0,
        );
        let mut lab: Vec<TileKey> = c["keys"].as_array().unwrap().iter().map(key).collect();
        ours.sort();
        lab.sort();
        assert_eq!(ours, lab, "tiles around {}", c["point"]);
    }
    println!(
        "geometry: adjacency equal; cube to sphere {direction_error:.1e}; sphere to cube {uv_error:.1e}"
    );
    // Bit-identical: `hypot` and fdlibm's tan and atan are V8's.
    assert!(direction_error == 0.0 && uv_error == 0.0);
}

/// The LOD lab's demo terrain (`lab/lod/src/app/DemoSurface.ts`), a test fixture.
struct DemoSurface {
    max_height: f64,
    t: Value,
}

const GRADIENTS: [[f64; 3]; 12] = [
    [1.0, 1.0, 0.0],
    [-1.0, 1.0, 0.0],
    [1.0, -1.0, 0.0],
    [-1.0, -1.0, 0.0],
    [1.0, 0.0, 1.0],
    [-1.0, 0.0, 1.0],
    [1.0, 0.0, -1.0],
    [-1.0, 0.0, -1.0],
    [0.0, 1.0, 1.0],
    [0.0, -1.0, 1.0],
    [0.0, 1.0, -1.0],
    [0.0, -1.0, -1.0],
];

/// `Math.imul` hash on int32, as the lab's.
fn hash(x: i32, y: i32, z: i32) -> u32 {
    let h =
        x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263) ^ z.wrapping_mul(1_442_695_041);
    let h = (h ^ ((h as u32) >> 13) as i32).wrapping_mul(1_274_126_177);
    (h ^ ((h as u32) >> 16) as i32) as u32
}

fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn perlin(x: f64, y: f64, z: f64) -> f64 {
    let (ix, iy, iz) = (x.floor(), y.floor(), z.floor());
    let (fx, fy, fz) = (x - ix, y - iy, z - iz);
    let (wx, wy, wz) = (fade(fx), fade(fy), fade(fz));
    let corner = |dx: f64, dy: f64, dz: f64| {
        let g = GRADIENTS
            [hash((ix + dx) as i32, (iy + dy) as i32, (iz + dz) as i32) as usize % GRADIENTS.len()];
        (g[0] * (fx - dx) + g[1] * (fy - dy) + g[2] * (fz - dz)) * std::f64::consts::FRAC_1_SQRT_2
    };
    let bottom = lerp(
        lerp(corner(0.0, 0.0, 0.0), corner(1.0, 0.0, 0.0), wx),
        lerp(corner(0.0, 1.0, 0.0), corner(1.0, 1.0, 0.0), wx),
        wy,
    );
    let top = lerp(
        lerp(corner(0.0, 0.0, 1.0), corner(1.0, 0.0, 1.0), wx),
        lerp(corner(0.0, 1.0, 1.0), corner(1.0, 1.0, 1.0), wx),
        wy,
    );
    lerp(bottom, top, wz)
}

fn fractal(x: f64, y: f64, z: f64, octaves: u64) -> f64 {
    let (mut sum, mut frequency, mut amplitude, mut weight) = (0.0, 1.0, 1.0, 0.0);
    for _ in 0..octaves {
        sum += perlin(x * frequency, y * frequency, z * frequency) * amplitude;
        weight += amplitude;
        frequency *= 2.0;
        amplitude *= 0.5;
    }
    sum / weight
}

impl DemoSurface {
    fn sample(&self, d: DVec3) -> SurfaceSample {
        let t = &self.t;
        let n = |k: &str| f(&t[k]);
        let i = |k: &str, j: usize| f(&t[k][j]);
        let (x, y, z) = (d.x, d.y, d.z);
        let (wf, wo, ws) = (n("warpFrequency"), u(&t["warpOctaves"]), n("warpStrength"));
        let warp_x = fractal(x * wf + i("warpOffsets", 0), y * wf, z * wf, wo) * ws;
        let warp_y = fractal(x * wf, y * wf + i("warpOffsets", 1), z * wf, wo) * ws;
        let warp_z = fractal(x * wf, y * wf, z * wf + i("warpOffsets", 2), wo) * ws;
        let (px, py, pz) = (x + warp_x, y + warp_y, z + warp_z);
        let cf = n("continentFrequency");
        let continent = fractal(px * cf, py * cf, pz * cf, u(&t["continentOctaves"]));
        let land = smoothstep(n("coastStart"), n("coastEnd"), continent);
        let ridge = |j: usize| {
            let rf = i("ridgeFrequencies", j);
            1.0 - perlin(px * rf, py * rf, pz * rf).abs().min(1.0)
        };
        let mountains = i("ridgeWeights", 0) * ridge(0).powf(i("ridgePowers", 0))
            + i("ridgeWeights", 1) * ridge(1).powf(i("ridgePowers", 1))
            + i("ridgeWeights", 2) * ridge(2).powf(i("ridgePowers", 2));
        let height_meters = self.max_height
            * land
            * (n("landBaseHeightFraction") + n("landMountainHeightFraction") * mountains);
        let color3 = |k: &str| [i(k, 0), i(k, 1), i(k, 2)];
        let color = if land < 0.02 {
            let ocean = color3("oceanColor");
            [ocean[0], ocean[1] + 0.07 * land, ocean[2]]
        } else if height_meters > n("snowHeightMeters") {
            color3("snowColor")
        } else if height_meters > n("rockHeightMeters") {
            color3("rockColor")
        } else {
            [0.13 + 0.12 * land, 0.25 + 0.1 * land, 0.12]
        };
        SurfaceSample {
            height_meters,
            color: color.map(|c| c as f32),
        }
    }
}

struct MeshError {
    positions: f64,
    normals: f64,
    others: f64,
}

/// Largest differences from a lab mesh: positions in meters, normals, and the rest (colours,
/// heights, grid, bounds) relative to their size.
fn mesh_error(
    ours: (&[[f32; 3]], &[[f32; 3]], &[f32]),
    lab: &Value,
    prefix: &str,
) -> (f64, f64, f64) {
    let flat = |name: &str| -> Vec<f64> { lab[name].as_array().unwrap().iter().map(f).collect() };
    let worst3 = |ours: &[[f32; 3]], lab: &[f64]| {
        assert_eq!(ours.len() * 3, lab.len(), "{prefix}: vertex count");
        ours.iter()
            .flatten()
            .zip(lab)
            .map(|(a, b)| (f64::from(*a) - b).abs())
            .fold(0.0, f64::max)
    };
    let heights = flat("heights");
    let h = ours
        .2
        .iter()
        .zip(&heights)
        .map(|(a, b)| (f64::from(*a) - b).abs() / b.abs().max(1.0))
        .fold(0.0, f64::max);
    (
        worst3(ours.0, &flat("positions")),
        worst3(ours.1, &flat("normals")),
        h,
    )
}

fn check_mesh(ours: &TileMeshData, lab: &Value, label: &str) -> MeshError {
    assert_eq!(ours.key, key(&lab["key"]), "{label}: key");
    let origin = (ours.origin - v3(&lab["origin"])).length();
    let (positions, normals, heights) =
        mesh_error((&ours.positions, &ours.normals, &ours.heights), lab, label);
    let flat = |name: &str| -> Vec<f64> { lab[name].as_array().unwrap().iter().map(f).collect() };
    let colors = ours
        .colors
        .iter()
        .flatten()
        .zip(flat("colors"))
        .map(|(a, b)| (f64::from(*a) - b).abs())
        .fold(0.0, f64::max);
    let grid: Vec<f64> = ours.grid.iter().flatten().map(|v| f64::from(*v)).collect();
    assert_eq!(grid, flat("grid"), "{label}: grid");
    let scalars = [
        (ours.min_height_meters, "minHeightMeters"),
        (ours.max_height_meters, "maxHeightMeters"),
        (ours.error_meters, "errorMeters"),
        (ours.skirt_depth_meters, "skirtDepthMeters"),
    ]
    .iter()
    .map(|(a, k)| (a - f(&lab[*k])).abs() / f(&lab[*k]).abs().max(1.0))
    .fold(0.0, f64::max);
    println!(
        "{label}: origin {origin:.1e} m, positions {positions:.1e} m, normals {normals:.1e}, heights {heights:.1e}, colours {colors:.1e}, bounds {scalars:.1e}"
    );
    MeshError {
        positions: positions.max(origin),
        normals,
        others: heights.max(colors).max(scalars),
    }
}

#[test]
fn meshes_match_the_lod_lab() {
    let g = golden("meshes");
    let presets = &g["presets"];
    let surface = |preset: &str| DemoSurface {
        max_height: f(&presets[preset]["maxSurfaceHeightMeters"]),
        t: presets[preset]["terrain"].clone(),
    };
    let options = |preset: &str| TileMeshOptions {
        radius_meters: f(&presets[preset]["radiusMeters"]),
        resolution: u(&presets[preset]["tileResolution"]) as usize,
    };
    let build = |preset: &str, k: TileKey| {
        let s = surface(preset);
        build_tile_mesh(k, &|d: DVec3, _cell: f64| s.sample(d), options(preset))
    };
    let mut worst = MeshError {
        positions: 0.0,
        normals: 0.0,
        others: 0.0,
    };
    for lab in g["meshes"].as_array().unwrap() {
        let preset = lab["preset"].as_str().unwrap();
        let k = key(&lab["key"]);
        let e = check_mesh(&build(preset, k), lab, &format!("{preset} {k}"));
        worst = MeshError {
            positions: worst.positions.max(e.positions),
            normals: worst.normals.max(e.normals),
            others: worst.others.max(e.others),
        };
    }

    let stitch = &g["stitch"];
    let fine = build("seam", key(&stitch["fine"]["key"]));
    let coarse = build("seam", key(&stitch["coarse"]["key"]));
    let e = edge(stitch["edge"].as_str().unwrap());
    assert_eq!(
        coarse.key,
        neighbor_key(fine.key, e).parent(),
        "stitch: the coarse tile is the neighbour's parent"
    );
    let mut seams = [None; 4];
    seams[e.index()] = Some(&coarse);
    let n = u(&presets["seam"]["tileResolution"]) as usize;
    let (positions, normals, heights) = stitch_edges(&fine, seams, n);
    let (dp, dn, dh) = mesh_error((&positions, &normals, &heights), stitch, "stitch");
    println!(
        "stitched {} against {}: positions {dp:.1e} m, normals {dn:.1e}, heights {dh:.1e}",
        fine.key, coarse.key
    );

    // Bit-identical: `hypot` and fdlibm's tan are V8's, and the f32 roundings happen where the
    // lab's typed arrays round.
    assert!(
        worst.positions == 0.0 && dp == 0.0,
        "positions differ by {} m",
        worst.positions.max(dp)
    );
    assert!(
        worst.normals == 0.0 && dn == 0.0,
        "normals differ by {}",
        worst.normals.max(dn)
    );
    assert!(
        worst.others == 0.0 && dh == 0.0,
        "heights, colours or bounds differ by {}",
        worst.others.max(dh)
    );
}

#[test]
fn selection_matches_the_lod_lab() {
    let g = golden("selection");
    let o = &g["options"];
    let options = PlanetLodOptions {
        radius_meters: f(&o["radiusMeters"]),
        min_surface_height_meters: f(&o["minSurfaceHeightMeters"]),
        max_surface_height_meters: f(&o["maxSurfaceHeightMeters"]),
        occluder_radius_meters: f(&o["occluderRadiusMeters"]),
        lod_surface_band_meters: f(&o["lodSurfaceBandMeters"]),
        resolution: u(&o["resolution"]) as usize,
        max_level: u(&o["maxLevel"]) as u32,
        split_distance_ratios: o["splitDistanceRatios"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r.as_f64().unwrap_or(f64::INFINITY))
            .collect(),
        retain_frames: 90,
        max_cached_tiles: u(&o["maxCachedTiles"]) as usize,
    };
    let c = &g["camera"];
    let every = u(&g["sampleEvery"]) as usize;
    let mut total_frames = 0;
    let mut worst_priority = 0.0_f64;
    for run in g["runs"].as_array().unwrap() {
        let name = format!(
            "{} ({}/frame)",
            run["scenario"].as_str().unwrap(),
            run["perFrame"]
                .as_u64()
                .map_or("all".into(), |p| p.to_string())
        );
        let per_frame = run["perFrame"].as_u64().map(|p| p as usize);
        let mut lod = PlanetLod::new(options.clone());
        for (i, lab) in run["frames"].as_array().unwrap().iter().enumerate() {
            let camera = LodCamera {
                position: v3(&lab["camera"]),
                distance_scale: f(&c["distanceScale"]),
                max_level: u(&c["maxLevel"]) as u32,
                focal_pixels: f(&c["focalPixels"]),
                min_observer_cell_pixels: f(&c["minObserverCellPixels"]),
            };
            let view = LodView {
                observer_positions: vec![v3(&lab["probe"])],
                camera: Some(camera),
                distance_scale: 1.0,
                horizon_culling: true,
            };
            let selection = lod.select(&view);
            let mut requests = selection.requests.clone();
            if let Some(n) = per_frame {
                // Stable, highest first, as the lab's sort.
                requests.sort_by(|a, b| b.priority.total_cmp(&a.priority));
                requests.truncate(n);
            }
            let counts = (
                selection.render.len(),
                selection.requests.len(),
                selection.horizon_culled,
                selection.balance_collapses.len(),
                selection.visited,
            );
            let lab_counts = (
                u(&lab["render"]) as usize,
                u(&lab["requests"]) as usize,
                u(&lab["culled"]) as usize,
                u(&lab["collapses"]) as usize,
                u(&lab["visited"]) as usize,
            );
            assert_eq!(
                counts, lab_counts,
                "{name} frame {i}: render, requests, culled, collapses, visited"
            );
            let built: Vec<u64> = requests.iter().map(|r| r.key.code()).collect();
            let lab_built: Vec<u64> = lab["built"].as_array().unwrap().iter().map(u).collect();
            assert_eq!(built, lab_built, "{name} frame {i}: built tiles");
            if i % every == 0 {
                let lab_render: Vec<u64> = lab["renderCodes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(u)
                    .collect();
                assert_eq!(
                    selection.render, lab_render,
                    "{name} frame {i}: render order"
                );
                for (ours, theirs) in selection
                    .requests
                    .iter()
                    .zip(lab["requestList"].as_array().unwrap())
                {
                    assert_eq!(
                        ours.key.code(),
                        u(&theirs[0]),
                        "{name} frame {i}: request order"
                    );
                    let p = theirs[1].as_f64().unwrap_or(if ours.priority > 0.0 {
                        f64::INFINITY
                    } else {
                        f64::NEG_INFINITY
                    });
                    if ours.priority != p {
                        worst_priority =
                            worst_priority.max((ours.priority - p).abs() / p.abs().max(1.0));
                    }
                }
            }
            for request in &requests {
                lod.accept_tile(Arc::new(stub(request.key)));
            }
            total_frames += 1;
        }
        assert_eq!(
            (lod.cached_tile_count() as u64, lod.node_count() as u64),
            (u(&run["cached"]), u(&run["nodes"])),
            "{name}: cache at the end"
        );
        println!(
            "{name}: {} frames equal; {} cached, {} nodes",
            run["frames"].as_array().unwrap().len(),
            lod.cached_tile_count(),
            lod.node_count()
        );
    }
    println!("{total_frames} frames; request priorities within {worst_priority:.1e} (relative)");
    // Bit-identical: patch distances use V8's hypot, and priorities near a tie decide build order.
    assert!(
        worst_priority == 0.0,
        "request priorities differ by {worst_priority:e}"
    );
}

/// An empty tile, as the lab's bench: selection reads nothing but its presence.
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
