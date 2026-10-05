//! Explicit, serializable surface recipes. Gas and stars never imply solid terrain.
use glam::DVec3;
use serde::{Deserialize, Serialize};
use void_terrain::noise::perlin;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum SurfaceRecipe {
    SolidSurface,
    GasEnvelope {
        low: [f32; 3],
        high: [f32; 3],
        bands: f64,
        turbulence: f64,
        storm: f64,
    },
    EmissiveStar {
        color: [f32; 3],
        radiance: f32,
        granulation: f64,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RingRecipe {
    pub inner_radius: f64,
    pub outer_radius: f64,
    pub color: [f32; 3],
    pub opacity: f32,
}
impl RingRecipe {
    pub fn validate(&self) {
        assert!(
            self.inner_radius.is_finite()
                && self.outer_radius.is_finite()
                && self.inner_radius > 1.0
                && self.outer_radius > self.inner_radius
        );
        assert!(
            self.color
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        );
        assert!(self.opacity.is_finite() && (0.0..=1.0).contains(&self.opacity));
    }
}
impl SurfaceRecipe {
    pub fn validate(&self) {
        match self {
            Self::SolidSurface => {}
            Self::GasEnvelope {
                low,
                high,
                bands,
                turbulence,
                storm,
            } => {
                assert!(
                    low.iter()
                        .chain(high)
                        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                );
                assert!(bands.is_finite() && (1.0..=100.0).contains(bands));
                assert!(turbulence.is_finite() && (0.0..=2.0).contains(turbulence));
                assert!(storm.is_finite() && (0.0..=1.0).contains(storm));
            }
            Self::EmissiveStar {
                color,
                radiance,
                granulation,
            } => {
                assert!(
                    color
                        .iter()
                        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                );
                assert!(
                    radiance.is_finite() && *radiance > 0.0 && *radiance <= 32752.0,
                    "star radiance must fit the HDR target including granulation"
                );
                assert!(granulation.is_finite() && (0.0..=1.0).contains(granulation));
            }
        }
    }
    /// Linear albedo, or linear emitted radiance for a star. Body fixed z is the spin axis.
    pub fn color(&self, d: DVec3) -> [f32; 4] {
        assert!(d.is_finite() && (d.length() - 1.0).abs() < 1e-6);
        match self {
            Self::SolidSurface => panic!("solid color must come from terrain sampler"),
            Self::GasEnvelope {
                low,
                high,
                bands,
                turbulence,
                storm,
            } => {
                let noise = perlin(d.x * 13.0, d.y * 13.0, d.z * 13.0);
                let band = (d.z * bands * std::f64::consts::PI + turbulence * noise * 3.0).sin();
                // Local elliptical anticyclone, smoothly blended with latitude bands.
                let spot = (-((d.x - 0.92).powi(2) / 0.015
                    + (d.y - 0.23).powi(2) / 0.035
                    + (d.z + 0.28).powi(2) / 0.004))
                    .exp()
                    * storm;
                let t = (0.5 + 0.4 * band + 0.08 * noise) as f32;
                [
                    low[0] + (high[0] - low[0]) * t * (1.0 - spot as f32 * 0.3),
                    low[1] + (high[1] - low[1]) * t * (1.0 - spot as f32 * 0.7),
                    low[2] + (high[2] - low[2]) * t * (1.0 - spot as f32 * 0.8),
                    1.0,
                ]
            }
            Self::EmissiveStar {
                color,
                radiance,
                granulation,
            } => {
                let noise = perlin(d.x * 95.0, d.y * 95.0, d.z * 95.0);
                let gain = radiance * (1.0 + granulation * noise) as f32;
                [color[0] * gain, color[1] * gain, color[2] * gain, 1.0]
            }
        }
    }
}
