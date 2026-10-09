//! lab/landing's placeholder hills (`HillsTerrain.ts`): fractal Perlin noise on the unit sphere,
//! shaped into rolling hills. The landing planets (Pebble, Luna, Terra, Aurelia) use it.

use glam::DVec3;
use serde::{Deserialize, Serialize};

use crate::noise::perlin;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HillsOptions {
    pub name: String,
    pub radius_meters: f64,
    pub max_height_meters: f64,
    /// Wavelength of the largest hills along the surface, metres.
    pub wavelength_meters: f64,
    pub octaves: u32,
}

#[derive(Clone, Debug)]
pub struct Hills {
    max_height: f64,
    frequency: f64,
    octaves: u32,
    norm: f64,
}

impl Hills {
    pub fn new(o: &HillsOptions) -> Self {
        assert!(
            o.radius_meters > 0.0
                && o.max_height_meters > 0.0
                && o.wavelength_meters > 0.0
                && o.octaves >= 1,
            "hills terrain: {o:?}"
        );
        // Noise has about one feature per unit, so this many units span the radius.
        let mut norm = 0.0;
        for octave in 0..o.octaves {
            norm += f64::powf(0.5, f64::from(octave));
        }
        Self {
            max_height: o.max_height_meters,
            frequency: o.radius_meters / o.wavelength_meters,
            octaves: o.octaves,
            norm,
        }
    }

    /// Height above the reference sphere and colour at a unit direction; the hills have no detail
    /// to band-limit, so the cell size is not needed.
    pub fn sample(&self, d: DVec3) -> (f64, [f64; 3]) {
        let mut sum = 0.0;
        let (mut f, mut a) = (self.frequency, 1.0);
        for octave in 0..self.octaves {
            sum += a * perlin(d.x * f + 17.3 * f64::from(octave), d.y * f, d.z * f);
            f *= 2.0;
            a *= 0.5;
        }
        // Gradient noise stays within about ±1; clamp so the contract bound holds exactly.
        let unit = (0.5 + 0.75 * (sum / self.norm)).clamp(0.0, 1.0);
        let height = self.max_height * unit * unit;
        (
            height,
            [0.18 + 0.35 * unit, 0.32 + 0.1 * unit, 0.14 + 0.2 * unit],
        )
    }
}
