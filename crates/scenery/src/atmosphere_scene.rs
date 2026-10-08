//! Authored optical profiles and a conservative ordering contract for separate atmosphere passes.
//! Optical scattering is independent of the physical pressure model. Radius comes from the body
//! and its air datum, rather than being repeated in an authored profile.
use crate::{AtmosphereParams, earth_like_atmosphere};
use glam::DVec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum AtmosphereProfile {
    EarthScaled {
        density_scale: f64,
    },
    Custom {
        height_meters: f64,
        rayleigh_scattering: [f64; 3],
        rayleigh_scale_height: f64,
        mie_scattering: f64,
        mie_extinction: f64,
        mie_scale_height: f64,
        mie_anisotropy: f64,
        ozone_absorption: [f64; 3],
        ozone_center_height: f64,
        ozone_width: f64,
    },
}
impl AtmosphereProfile {
    pub fn parameters(&self, bottom_radius: f64) -> AtmosphereParams {
        assert!(
            bottom_radius.is_finite() && bottom_radius > 0.0,
            "invalid optical radius"
        );
        let p = match *self {
            Self::EarthScaled { density_scale } => {
                assert!(
                    density_scale.is_finite() && density_scale > 0.0,
                    "invalid optical density"
                );
                let mut p = earth_like_atmosphere(bottom_radius);
                p.rayleigh_scattering = p.rayleigh_scattering.map(|v| v * density_scale);
                p.ozone_absorption = p.ozone_absorption.map(|v| v * density_scale);
                p.mie_scattering *= density_scale;
                p.mie_extinction *= density_scale;
                p
            }
            Self::Custom {
                height_meters,
                rayleigh_scattering,
                rayleigh_scale_height,
                mie_scattering,
                mie_extinction,
                mie_scale_height,
                mie_anisotropy,
                ozone_absorption,
                ozone_center_height,
                ozone_width,
            } => AtmosphereParams {
                bottom_radius,
                top_radius: bottom_radius + height_meters,
                rayleigh_scattering,
                rayleigh_scale_height,
                mie_scattering,
                mie_extinction,
                mie_scale_height,
                mie_anisotropy,
                ozone_absorption,
                ozone_center_height,
                ozone_width,
            },
        };
        assert!(
            p.top_radius.is_finite() && p.top_radius > p.bottom_radius,
            "invalid optical height"
        );
        assert!(
            [p.rayleigh_scale_height, p.mie_scale_height, p.ozone_width]
                .iter()
                .all(|v| v.is_finite() && *v > 0.0),
            "invalid optical scale height"
        );
        assert!(
            p.rayleigh_scattering
                .iter()
                .chain(p.ozone_absorption.iter())
                .chain([&p.mie_scattering, &p.mie_extinction, &p.ozone_center_height])
                .all(|v| v.is_finite() && *v >= 0.0),
            "invalid optical coefficient"
        );
        assert!(
            p.mie_extinction >= p.mie_scattering,
            "extinction below scattering"
        );
        assert!(
            p.mie_anisotropy.is_finite() && p.mie_anisotropy.abs() < 1.0,
            "invalid Mie anisotropy"
        );
        p
    }
}

/// A weather field or an unbroken aerosol deck; never infer morphology from the body name.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum CloudMorphology {
    EarthWeather,
    ContinuousDeck,
}

/// Authored appearance of a continuous deck. No planet palette is implicit in its transport.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudDeckAppearance {
    pub absorber_tint: [f64; 3],
    pub latitude_frequency: f64,
    pub band_contrast: f64,
    pub warp: f64,
    pub texture_scale: [f64; 3],
}

/// Height above the body's cloud datum. Noise recipe is currently shared, not Earth heights.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudProfile {
    pub bottom_meters: f64,
    pub top_meters: f64,
    pub extinction_per_meter: f64,
    pub coverage: f64,
    pub morphology: CloudMorphology,
    pub single_scattering_albedo: [f64; 3],
    pub deck: Option<CloudDeckAppearance>,
}
impl CloudProfile {
    pub fn earth() -> Self {
        Self {
            bottom_meters: 1500.0,
            top_meters: 8000.0,
            extinction_per_meter: 0.0011,
            coverage: 0.62,
            morphology: CloudMorphology::EarthWeather,
            single_scattering_albedo: [0.99; 3],
            deck: None,
        }
    }
    pub fn vesper() -> Self {
        Self {
            bottom_meters: 48000.0,
            top_meters: 70000.0,
            extinction_per_meter: 0.003,
            coverage: 1.0,
            morphology: CloudMorphology::ContinuousDeck,
            single_scattering_albedo: [0.999, 0.998, 0.995],
            deck: Some(CloudDeckAppearance {
                absorber_tint: [0.94, 0.88, 0.76],
                latitude_frequency: 11.0,
                band_contrast: 0.04,
                warp: 0.55,
                texture_scale: [2.0, 2.0, 7.0],
            }),
        }
    }
    /// The deck's integrated density moment (smooth tapers and mean structure).
    pub fn vertical_optical_depth(&self) -> f64 {
        assert_eq!(self.morphology, CloudMorphology::ContinuousDeck);
        (self.top_meters - self.bottom_meters) * self.extinction_per_meter * 0.65
    }
    pub fn validate(&self) {
        assert_eq!(
            self.deck.is_some(),
            self.morphology == CloudMorphology::ContinuousDeck,
            "cloud morphology/appearance mismatch"
        );
        if let Some(deck) = &self.deck {
            assert!(
                deck.absorber_tint
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
                "invalid deck tint"
            );
            assert!(
                deck.latitude_frequency.is_finite()
                    && deck.latitude_frequency > 0.0
                    && deck.band_contrast.is_finite()
                    && (0.0..=0.5).contains(&deck.band_contrast)
                    && deck.warp.is_finite()
                    && deck.warp >= 0.0
                    && deck.texture_scale.iter().all(|v| v.is_finite() && *v > 0.0),
                "invalid deck texture"
            );
        }
        assert!(
            self.single_scattering_albedo
                .iter()
                .all(|v| v.is_finite() && *v > 0.0 && *v <= 1.0),
            "invalid cloud albedo"
        );
        assert!(
            self.morphology != CloudMorphology::ContinuousDeck || self.coverage == 1.0,
            "continuous cloud deck requires full coverage"
        );
        assert!(
            self.bottom_meters.is_finite()
                && self.bottom_meters >= 0.0
                && self.top_meters.is_finite()
                && self.top_meters > self.bottom_meters,
            "invalid cloud heights"
        );
        assert!(
            self.extinction_per_meter.is_finite()
                && self.extinction_per_meter > 0.0
                && self.coverage.is_finite()
                && (0.0..=1.0).contains(&self.coverage),
            "invalid cloud coefficients"
        );
    }
}

/// Camera-relative, computed in f64 using the frame tree. No absolute barycentric f32 positions.
#[derive(Clone, Copy, Debug)]
pub struct AtmosphereVolume {
    pub body: usize,
    pub center: DVec3,
    pub radius: f64,
}

/// Back-to-front whole-volume passes. Disjoint angular footprints need no relative ordering.
/// Overlapping footprints require disjoint radial intervals: this deliberately rejects ambiguous
/// views rather than pretending a centre-distance sort is a per-pixel depth sort. Interpenetrating
/// media need a joint ray marcher and are outside this renderer's contract.
pub fn ordered_volumes(volumes: &[AtmosphereVolume]) -> Vec<usize> {
    for (i, a) in volumes.iter().enumerate() {
        assert!(
            a.center.is_finite() && a.radius.is_finite() && a.radius > 0.0,
            "invalid atmosphere volume"
        );
        for b in &volumes[..i] {
            assert_ne!(a.body, b.body, "duplicate atmosphere body");
            assert!(
                (a.center - b.center).length() > a.radius + b.radius,
                "interpenetrating atmosphere volumes"
            );
            let (da, db) = (a.center.length(), b.center.length());
            let overlap = if da <= a.radius || db <= b.radius {
                true
            } else {
                let separation = (a.center.dot(b.center) / (da * db)).clamp(-1.0, 1.0).acos();
                separation <= (a.radius / da).asin() + (b.radius / db).asin()
            };
            assert!(
                !overlap || da + a.radius < db - b.radius || db + b.radius < da - a.radius,
                "ambiguous atmosphere depth ordering; joint per-ray composition required"
            );
        }
    }
    let mut indices: Vec<_> = (0..volumes.len()).collect();
    indices.sort_by(|&a, &b| {
        let distance = |i: usize| (volumes[i].center.length() - volumes[i].radius).max(0.0);
        distance(b)
            .total_cmp(&distance(a))
            .then(volumes[a].body.cmp(&volumes[b].body))
    });
    indices
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separate_media_are_ordered_far_to_near_and_airless_is_empty() {
        assert!(ordered_volumes(&[]).is_empty());
        let v = [
            AtmosphereVolume {
                body: 1,
                center: DVec3::X * 20.0,
                radius: 3.0,
            },
            AtmosphereVolume {
                body: 2,
                center: DVec3::X * 100.0,
                radius: 5.0,
            },
        ];
        assert_eq!(ordered_volumes(&v), [1, 0]);
        // Noncommutative transport: far light is attenuated by the near atmosphere.
        let (scene, far_t, far_s, near_t, near_s) = (2.0, 0.5, 1.0, 0.25, 3.0);
        let composed = (scene * far_t + far_s) * near_t + near_s;
        assert_eq!(composed, 3.5);
        assert_ne!(composed, (scene * near_t + near_s) * far_t + far_s);
    }
    #[test]
    #[should_panic(expected = "interpenetrating")]
    fn overlapping_media_are_rejected() {
        ordered_volumes(&[
            AtmosphereVolume {
                body: 0,
                center: DVec3::ZERO,
                radius: 10.0,
            },
            AtmosphereVolume {
                body: 1,
                center: DVec3::X,
                radius: 10.0,
            },
        ]);
    }
    #[test]
    #[should_panic(expected = "ambiguous atmosphere depth ordering")]
    fn disjoint_spheres_with_ambiguous_projected_depth_are_rejected() {
        ordered_volumes(&[
            AtmosphereVolume {
                body: 0,
                center: DVec3::new(100.0, 0.0, 0.0),
                radius: 30.0,
            },
            AtmosphereVolume {
                body: 1,
                center: DVec3::new(140.0, 60.0, 0.0),
                radius: 30.0,
            },
        ]);
    }
    #[test]
    fn disjoint_angular_footprints_need_no_radial_separation() {
        let volumes = [
            AtmosphereVolume {
                body: 1,
                center: DVec3::X * 100.0,
                radius: 10.0,
            },
            AtmosphereVolume {
                body: 2,
                center: DVec3::Y * 100.0,
                radius: 10.0,
            },
        ];
        assert_eq!(ordered_volumes(&volumes), [0, 1]);
    }
    #[test]
    fn continuous_deck_is_explicit_and_optically_thick() {
        let p = CloudProfile::vesper();
        p.validate();
        assert!(p.vertical_optical_depth() > 40.0);
        assert_eq!(
            serde_json::from_str::<CloudProfile>(&serde_json::to_string(&p).unwrap()).unwrap(),
            p
        );
        let mut invalid = p;
        invalid.coverage = 0.99;
        assert!(std::panic::catch_unwind(|| invalid.validate()).is_err());
    }
    #[test]
    fn continuous_deck_palette_is_authored_and_no_earth_appearance_is_inferred() {
        let mut p = CloudProfile::vesper();
        let deck = p.deck.as_mut().unwrap();
        deck.absorber_tint = [1.0; 3];
        deck.band_contrast = 0.0;
        deck.latitude_frequency = 3.0;
        deck.texture_scale = [1.0, 2.0, 3.0];
        p.validate();
        assert_eq!(
            serde_json::from_str::<CloudProfile>(&serde_json::to_string(&p).unwrap()).unwrap(),
            p
        );
        p.deck = None;
        assert!(std::panic::catch_unwind(|| p.validate()).is_err());
        assert!(CloudProfile::earth().deck.is_none());
    }
    #[test]
    fn custom_profile_round_trip_and_non_earth_parameters() {
        let p = AtmosphereProfile::Custom {
            height_meters: 50000.0,
            rayleigh_scattering: [1e-6, 2e-6, 3e-6],
            rayleigh_scale_height: 11000.0,
            mie_scattering: 0.0,
            mie_extinction: 0.0,
            mie_scale_height: 1000.0,
            mie_anisotropy: 0.0,
            ozone_absorption: [0.0; 3],
            ozone_center_height: 0.0,
            ozone_width: 1.0,
        };
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<AtmosphereProfile>(&json).unwrap(), p);
        assert_eq!(p.parameters(3e6).top_radius, 3.05e6);
    }
}
