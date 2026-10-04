//! lab/scenery's CPU side against the TS original (golden/scenery.ts). The tables, noise volumes,
//! weather atlas and stars are bit-identical; the rest differs only where V8's sin/cos do.

use glam::DVec3;
use serde::Deserialize;
use void_scenery::atmosphere::{
    build_transmittance_table, earth_like_atmosphere, sky_radiance, transmittance_coords,
    transmittance_ray,
};
use void_scenery::clouds::{
    DETAIL_SIZE, DensityOptions, SHAPE_PERIOD, SHAPE_SIZE, WEATHER_WIDTH, build_cloud_noise,
    build_cloud_weather, cloud_density, cloud_shell_intervals, cloud_weather, sample_cloud_noise,
};
use void_scenery::tables::{build_irradiance_table, build_multiple_scattering_table, march_sky};
use void_scenery::{DEFAULT_STARS, OrbitView, generate_stars};

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/");

#[test]
fn airless_tables_and_sky_have_finite_vacuum_limits() {
    let mut p = earth_like_atmosphere(1_737_400.0);
    p.rayleigh_scattering = [0.0; 3];
    p.ozone_absorption = [0.0; 3];
    p.mie_scattering = 0.0;
    p.mie_extinction = 0.0;
    let trans = build_transmittance_table(&p);
    let multiple = build_multiple_scattering_table(&p, &trans, 8, 4);
    let irradiance = build_irradiance_table(&p, &trans, &multiple, 8, 4);
    assert!(
        trans
            .iter()
            .chain(&multiple)
            .chain(&irradiance)
            .all(|v| v.is_finite())
    );
    for texel in trans.as_chunks::<4>().0 {
        assert_eq!(&texel[..3], &[1.0; 3]);
    }
    for texel in irradiance.as_chunks::<4>().0 {
        assert_eq!(&texel[..3], &[0.0; 3]);
    }
    let sky = march_sky(
        &p,
        &trans,
        Some(&multiple),
        p.bottom_radius + 100.0,
        DVec3::Z,
        DVec3::Z,
        4,
    );
    assert_eq!(sky.radiance, [0.0; 3]);
    assert_eq!(sky.transmittance, [1.0; 3]);
}

#[derive(Deserialize, Clone, Copy)]
struct V {
    x: f64,
    y: f64,
    z: f64,
}
impl From<V> for DVec3 {
    fn from(v: V) -> Self {
        DVec3::new(v.x, v.y, v.z)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Golden {
    radius: f64,
    sky: Vec<SkyRay>,
    coords: Vec<Coord>,
    weather_samples: Vec<WeatherSample>,
    noise_samples: Vec<NoiseSample>,
    densities: Vec<DensitySample>,
    shells: Vec<Shell>,
    poses: Vec<PoseSample>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkyRay {
    altitude: f64,
    direction: V,
    sun: V,
    reference: [f64; 3],
    march: March,
    march_single: March,
}
#[derive(Deserialize)]
struct March {
    radiance: [f64; 3],
    transmittance: [f64; 3],
}
#[derive(Deserialize)]
struct Coord {
    x: f64,
    y: f64,
    r: f64,
    mu: f64,
    back: XY,
}
#[derive(Deserialize)]
struct XY {
    x: f64,
    y: f64,
}
#[derive(Deserialize)]
struct WeatherSample {
    d: V,
    w: [f64; 2],
}
#[derive(Deserialize)]
struct NoiseSample {
    point: V,
    values: [f64; 3],
}
#[derive(Deserialize)]
struct DensitySample {
    args: [f64; 9],
    density: f64,
}
#[derive(Deserialize)]
struct Shell {
    origin: V,
    direction: V,
    scene: Option<f64>,
    intervals: Vec<[f64; 2]>,
}
#[derive(Deserialize)]
struct PoseSample {
    position: V,
    forward: V,
    up: V,
    basis: Basis,
    tilt: f64,
}
#[derive(Deserialize)]
struct Basis {
    right: V,
    up: V,
    back: V,
}

fn golden() -> Golden {
    serde_json::from_str(&std::fs::read_to_string(format!("{GOLDEN}scenery.json")).unwrap())
        .unwrap()
}

fn raw(name: &str) -> Vec<u8> {
    std::fs::read(format!("{GOLDEN}{name}")).unwrap()
}

fn raw_f32(name: &str) -> Vec<f32> {
    raw(name)
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect()
}

/// Largest relative difference, against max(|expected|, floor).
fn worst(a: impl IntoIterator<Item = f64>, b: impl IntoIterator<Item = f64>, floor: f64) -> f64 {
    a.into_iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs() / y.abs().max(floor))
        .fold(0.0, f64::max)
}

fn worst_f32(a: &[f32], b: &[f32]) -> f64 {
    assert_eq!(a.len(), b.len());
    worst(
        a.iter().map(|&v| f64::from(v)),
        b.iter().map(|&v| f64::from(v)),
        1e-30,
    )
}

#[test]
fn atmosphere_tables() {
    let g = golden();
    let p = earth_like_atmosphere(g.radius);
    let transmittance = build_transmittance_table(&p);
    let d = worst_f32(&transmittance, &raw_f32("transmittance.bin"));
    println!("transmittance: worst relative {d:e}");
    assert!(d == 0.0, "transmittance {d:e}");

    let multiple = build_multiple_scattering_table(&p, &transmittance, 64, 20);
    let d = worst_f32(&multiple, &raw_f32("multiple.bin"));
    println!("multiple scattering: worst relative {d:e}");
    assert!(d == 0.0, "multiple scattering {d:e}");

    let irradiance = build_irradiance_table(&p, &transmittance, &multiple, 128, 24);
    let d = worst_f32(&irradiance, &raw_f32("irradiance.bin"));
    println!("irradiance: worst relative {d:e}");
    assert!(d == 0.0, "irradiance {d:e}");

    let mut worst_sky = (0.0f64, 0.0f64, 0.0f64);
    for ray in &g.sky {
        let (direction, sun) = (ray.direction.into(), ray.sun.into());
        let reference = sky_radiance(&p, ray.altitude, direction, sun, 500);
        let march = march_sky(
            &p,
            &transmittance,
            Some(&multiple),
            g.radius + ray.altitude,
            direction,
            sun,
            32,
        );
        let single = march_sky(
            &p,
            &transmittance,
            None,
            g.radius + ray.altitude,
            direction,
            sun,
            32,
        );
        worst_sky.0 = worst_sky.0.max(worst(reference, ray.reference, 1e-12));
        worst_sky.1 = worst_sky.1.max(worst(
            march.radiance.into_iter().chain(march.transmittance),
            ray.march
                .radiance
                .into_iter()
                .chain(ray.march.transmittance),
            1e-12,
        ));
        worst_sky.2 = worst_sky
            .2
            .max(worst(single.radiance, ray.march_single.radiance, 1e-12));
    }
    println!(
        "sky reference / march / single march: worst relative {:e} / {:e} / {:e}",
        worst_sky.0, worst_sky.1, worst_sky.2
    );
    assert!(worst_sky == (0.0, 0.0, 0.0), "{worst_sky:?}");

    for c in &g.coords {
        let (r, mu) = transmittance_ray(&p, c.x, c.y);
        let back = transmittance_coords(&p, r, mu);
        assert!(
            worst(
                [r, mu, back.0, back.1],
                [c.r, c.mu, c.back.x, c.back.y],
                1e-12
            ) <= 1e-12
        );
    }
}

#[test]
fn clouds() {
    let g = golden();
    let weather = build_cloud_weather(std::thread::available_parallelism().map_or(4, |n| n.get()));
    let rows: Vec<usize> = (0..64).map(|i| i * 16).chain([1023]).collect();
    let expected = raw("weather_rows.bin");
    let row_bytes = WEATHER_WIDTH * 4;
    let mut differing = 0;
    for (k, &y) in rows.iter().enumerate() {
        let ours = &weather[y * row_bytes..(y + 1) * row_bytes];
        let theirs = &expected[k * row_bytes..(k + 1) * row_bytes];
        differing += ours.iter().zip(theirs).filter(|(a, b)| a != b).count();
    }
    println!(
        "weather atlas: {differing} differing bytes of {}",
        expected.len()
    );
    assert_eq!(differing, 0, "weather atlas");
    let d = g
        .weather_samples
        .iter()
        .map(|s| {
            let (h, k) = cloud_weather(s.d.into());
            (h - s.w[0]).abs().max((k - s.w[1]).abs())
        })
        .fold(0.0, f64::max);
    println!("weather samples: worst {d:e}");
    assert!(d <= 1e-12);

    let shape = build_cloud_noise(SHAPE_SIZE, false);
    let detail = build_cloud_noise(DETAIL_SIZE, true);
    assert!(shape == raw("shape.bin"), "shape volume differs");
    assert!(detail == raw("detail.bin"), "detail volume differs");
    for s in &g.noise_samples {
        for c in 0..3 {
            let v = sample_cloud_noise(&shape, SHAPE_SIZE, s.point.into(), SHAPE_PERIOD, c);
            assert!(
                (v - s.values[c]).abs() <= 1e-12,
                "noise sample {v} vs {}",
                s.values[c]
            );
        }
    }

    let d = g
        .densities
        .iter()
        .map(|s| {
            let a = s.args;
            let o = DensityOptions {
                amount: a[5],
                detail_weight: a[6],
                footprint: a[7],
                macro_shape: a[8],
            };
            (cloud_density(a[0], a[1], a[2], a[3], a[4], o) - s.density).abs()
        })
        .fold(0.0, f64::max);
    println!("densities: worst {d:e}");
    assert!(d <= 1e-12);

    for s in &g.shells {
        let inner = g.radius + 6500.0;
        let ours = cloud_shell_intervals(
            s.origin.into(),
            s.direction.into(),
            inner,
            g.radius + 13000.0,
            s.scene.unwrap_or(f64::INFINITY),
        );
        assert_eq!(ours.len(), s.intervals.len());
        for (a, b) in ours.iter().zip(&s.intervals) {
            assert!(
                worst([a.0, a.1], [b[0], b[1]], 1.0) <= 1e-12,
                "{a:?} vs {b:?}"
            );
        }
    }
}

#[test]
fn stars() {
    let (positions, colors) = generate_stars(&DEFAULT_STARS);
    let flat = |v: &[[f32; 3]]| v.iter().flatten().copied().collect::<Vec<f32>>();
    let (dp, dc) = (
        worst_f32(&flat(&positions), &raw_f32("star_positions.bin")),
        worst_f32(&flat(&colors), &raw_f32("star_colors.bin")),
    );
    println!("stars: worst relative position {dp:e}, colour {dc:e}");
    assert!(dp == 0.0 && dc == 0.0);
}

#[test]
fn orbit_view() {
    let g = golden();
    let r = g.radius;
    let unit = |x: f64, y: f64, z: f64| DVec3::new(x, y, z).normalize();
    let mut view = OrbitView::new(unit(0.3, -0.5, 0.8), r + 20e3, 0.4, r * 40.0);
    let mut poses = Vec::new();
    let mut record = |view: &OrbitView| {
        let pose = view.pose();
        let (right, up, back) = view.basis();
        poses.push((pose, right, up, back, view.tilt_radians()));
    };
    record(&view);
    view.pan_screen(120.0, -40.0, 1.0, 900.0, 20e3);
    record(&view);
    view.orbit_around_center(0.3, -0.2);
    record(&view);
    view.turn(0.7, 1.1);
    record(&view);
    view.set_radius(r + 3e3);
    record(&view);
    view.place(unit(0.0, 0.0, 1.0), r + 400e3, 0.5, 0.0);
    record(&view);
    view.orbit_around_center(-1.2, 0.9);
    record(&view);
    view.turn(-2.0, 5.0);
    record(&view);
    view.set_radius(r * 100.0);
    record(&view);
    let mut d = 0.0f64;
    for ((pose, right, up, back, tilt), e) in poses.iter().zip(&g.poses) {
        let v = |a: DVec3| a.to_array();
        let ours = [
            v(pose.position).to_vec(),
            v(pose.forward).to_vec(),
            v(pose.up).to_vec(),
            v(*right).to_vec(),
            v(*up).to_vec(),
            v(*back).to_vec(),
            vec![*tilt],
        ]
        .concat();
        let e3 = |a: V| [a.x, a.y, a.z].to_vec();
        let theirs = [
            e3(e.position),
            e3(e.forward),
            e3(e.up),
            e3(e.basis.right),
            e3(e.basis.up),
            e3(e.basis.back),
            vec![e.tilt],
        ]
        .concat();
        d = d.max(worst(ours, theirs, 1.0));
    }
    println!("orbit view: worst {d:e}");
    assert!(d <= 1e-14);
}
