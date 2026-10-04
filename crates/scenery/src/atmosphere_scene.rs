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

/// Height above the body's cloud datum. Noise recipe is currently shared, not Earth heights.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudProfile {
    pub bottom_meters: f64,
    pub top_meters: f64,
    pub extinction_per_meter: f64,
    pub coverage: f64,
}
impl CloudProfile {
    pub fn earth() -> Self {
        Self {
            bottom_meters: 1500.0,
            top_meters: 8000.0,
            extinction_per_meter: 0.0011,
            coverage: 0.62,
        }
    }
    pub fn validate(&self) {
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
