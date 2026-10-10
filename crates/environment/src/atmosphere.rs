//! Air by altitude (`void-aero` re-exports it).

fn finite(value: f64, label: &str) -> f64 {
    assert!(value.is_finite(), "{label}: non-finite");
    value
}

fn positive(value: f64, label: &str) -> f64 {
    finite(value, label);
    assert!(value > 0.0, "{label}: must be positive");
    value
}

/// Smoothstep from a to b, clamped to [0, 1].
pub fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = 0.0_f64.max(1.0_f64.min((x - a) / (b - a)));
    t * t * (3.0 - 2.0 * t)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Air {
    pub density: f64,
    pub pressure_pa: f64,
    pub temperature_k: f64,
    pub sound_speed: f64,
    pub viscosity: f64,
}

pub fn validate_air(air: &Air) {
    for (value, key) in [
        (air.density, "density"),
        (air.pressure_pa, "pressurePa"),
        (air.sound_speed, "soundSpeed"),
        (air.viscosity, "viscosity"),
    ] {
        finite(value, key);
        assert!(value >= 0.0, "Negative air {key}");
    }
    positive(air.temperature_k, "air temperature");
    if air.density > 0.0 {
        positive(air.sound_speed, "air sound speed");
        positive(air.viscosity, "air viscosity");
    }
}

const R: f64 = 287.05287;
const G: f64 = 9.80665;
const GEOPOTENTIAL_RADIUS: f64 = 6_356_766.0;
const LEVELS: [f64; 8] = [
    0.0, 11_000.0, 20_000.0, 32_000.0, 47_000.0, 51_000.0, 71_000.0, 84_852.0,
];
const LAPSE: [f64; 8] = [-0.0065, 0.0, 0.001, 0.0028, 0.0, -0.0028, -0.002, 0.0];

/// Lower US standard atmosphere layers, then an isothermal high-altitude extension (a game
/// approximation, not the 1976 standard's upper model), smoothly reaching exact vacuum between 105
/// and 120 km. No terrain height enters this model.
#[derive(Clone, Debug)]
pub struct EarthAtmosphere {
    pub density_scale: f64,
    temperatures: [f64; 8],
    pressures: [f64; 8],
}

impl EarthAtmosphere {
    pub const CEILING_METERS: f64 = 120_000.0;

    pub fn new(density_scale: f64) -> Self {
        positive(density_scale, "density scale");
        let (mut temperatures, mut pressures) = ([288.15; 8], [101_325.0; 8]);
        for i in 1..LEVELS.len() {
            let (dh, lapse) = (LEVELS[i] - LEVELS[i - 1], LAPSE[i - 1]);
            let (t0, p0) = (temperatures[i - 1], pressures[i - 1]);
            let t = t0 + lapse * dh;
            temperatures[i] = t;
            pressures[i] = layer_pressure(p0, t0, t, lapse, dh);
        }
        Self {
            density_scale,
            temperatures,
            pressures,
        }
    }
}

fn layer_pressure(p0: f64, t0: f64, t: f64, lapse: f64, dh: f64) -> f64 {
    if lapse == 0.0 {
        p0 * f64::exp(-G * dh / (R * t0))
    } else {
        p0 * f64::powf(t / t0, -G / (R * lapse))
    }
}

impl Air {
    /// What the model gives above its ceiling and in `Atmosphere::Vacuum`: no gas, and the 3 K
    /// background as temperature.
    pub const VACUUM: Air = Air {
        density: 0.0,
        pressure_pa: 0.0,
        temperature_k: 3.0,
        sound_speed: 0.0,
        viscosity: 0.0,
    };
}

/// Two atmospheres: Earth's air, and vacuum for comparison.
#[derive(Clone, Debug)]
pub enum Atmosphere {
    Earth(EarthAtmosphere),
    Vacuum,
}

impl Atmosphere {
    pub fn earth() -> Self {
        Self::Earth(EarthAtmosphere::new(1.0))
    }

    pub fn ceiling_meters(&self) -> f64 {
        match self {
            Self::Earth(_) => EarthAtmosphere::CEILING_METERS,
            Self::Vacuum => 0.0,
        }
    }

    /// Air at `altitude` metres above sea level; the domain starts at −5 km.
    pub fn sample(&self, altitude: f64) -> Air {
        finite(altitude, "altitude");
        let Self::Earth(earth) = self else {
            return Air::VACUUM;
        };
        assert!(
            altitude >= -5000.0,
            "Atmosphere: altitude below configured -5 km domain"
        );
        if altitude >= EarthAtmosphere::CEILING_METERS {
            return Air::VACUUM;
        }
        let h = GEOPOTENTIAL_RADIUS * altitude / (GEOPOTENTIAL_RADIUS + altitude);
        let mut i = 0;
        while i + 1 < LEVELS.len() && h >= LEVELS[i + 1] {
            i += 1;
        }
        let (dh, lapse, t0) = (h - LEVELS[i], LAPSE[i], earth.temperatures[i]);
        let t = t0 + lapse * dh;
        let pressure_pa = layer_pressure(earth.pressures[i], t0, t, lapse, dh)
            * earth.density_scale
            * (1.0 - smooth(105_000.0, EarthAtmosphere::CEILING_METERS, altitude));
        Air {
            pressure_pa,
            temperature_k: t,
            density: pressure_pa / (R * t),
            sound_speed: (1.4 * R * t).sqrt(),
            viscosity: 1.716e-5 * f64::powf(t / 273.15, 1.5) * (273.15 + 110.4) / (t + 110.4),
        }
    }
}
