//! Headless seam lab. Serialized cases contain the actual inputs, not just a random seed.
mod fleet;
mod multiscale;
use glam::{DQuat, DVec3};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
};

/// Bump whenever fixtures or case interpretation changes. Older corpus is explicitly rejected.
pub const VERSION: u32 = 1;
pub(crate) fn maximum(previous: f64, value: f64) -> f64 {
    assert!(
        value.is_finite() && value >= 0.0,
        "seam metric: non-finite or negative observation {value}"
    );
    previous.max(value)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub version: u32,
    pub seed: u64,
    pub index: usize,
    pub input: Input,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Input {
    Separate {
        altitude: f64,
        rotation: DQuat,
        velocity: DVec3,
        spin: DVec3,
    },
    Join {
        rotation: DQuat,
        velocity: DVec3,
        spin: DVec3,
        gap: f64,
        distant: bool,
        #[serde(with = "cell_strings")]
        cells: [i128; 3],
    },
    Encounter {
        distance: f64,
        speed: f64,
        miss: f64,
    },
    Frame {
        position: DVec3,
        velocity: DVec3,
        time: f64,
        #[serde(with = "cell_strings")]
        cells: [i128; 3],
    },
    Transfer {
        position: DVec3,
        velocity: DVec3,
        #[serde(with = "cell_strings")]
        cells: [i128; 3],
    },
}
// JSON numbers cannot carry the full i128 cell range; store decimal strings exactly, as SplitPosition does.
mod cell_strings {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(cells: &[i128; 3], serializer: S) -> Result<S::Ok, S::Error> {
        cells.map(|c| c.to_string()).serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[i128; 3], D::Error> {
        let strings = <[String; 3]>::deserialize(deserializer)?;
        let mut cells = [0; 3];
        for (cell, text) in cells.iter_mut().zip(strings) {
            *cell = text.parse().map_err(serde::de::Error::custom)?;
        }
        Ok(cells)
    }
}
impl Case {
    pub fn read(path: impl AsRef<Path>) -> Self {
        let value: Value = serde_json::from_slice(&fs::read(path).expect("seam case: read"))
            .expect("seam case: JSON");
        let c: Self = serde_json::from_value(value.get("case").unwrap_or(&value).clone())
            .expect("seam case: schema");
        assert_eq!(
            c.version, VERSION,
            "seam case: incompatible fixture version"
        );
        c
    }
    pub fn check(&self) -> Value {
        assert_eq!(
            self.version, VERSION,
            "seam case: incompatible fixture version"
        );
        match &self.input {
            Input::Separate { .. } | Input::Join { .. } | Input::Encounter { .. } => {
                fleet::check(&self.input)
            }
            Input::Frame { .. } | Input::Transfer { .. } => multiscale::check(&self.input),
        }
    }
}
/// Catch only to persist evidence. A failed check is still a failure and the CLI exits nonzero.
pub fn run_case(c: &Case, directory: &Path) -> Result<Value, String> {
    capture(c, directory, || c.check())
}
fn capture(c: &Case, directory: &Path, check: impl FnOnce() -> Value) -> Result<Value, String> {
    match catch_unwind(AssertUnwindSafe(check)) {
        Ok(metrics) => Ok(metrics),
        Err(payload) => {
            let reason = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or("non-string panic".into());
            fs::create_dir_all(directory).expect("seam case: create failure directory");
            let data=serde_json::to_vec_pretty(&json!({"case":c,"failure":reason,"replay":"cargo run -p void-seam-check -- --case <this file>"})).expect("seam case: encode");
            // Never overwrite earlier evidence, including repeated failures from the same seed.
            let mut suffix = 0u64;
            let path = loop {
                let path =
                    directory.join(format!("case-{:016x}-{}-{suffix}.json", c.seed, c.index));
                match fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                {
                    Ok(mut f) => {
                        use std::io::Write;
                        f.write_all(&data).expect("seam case: write");
                        f.sync_all().expect("seam case: sync");
                        break path;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => suffix += 1,
                    Err(e) => panic!("seam case: preserve evidence: {e}"),
                }
            };
            Err(format!("{reason}; case saved to {}", path.display()))
        }
    }
}
struct Rng(u64);
impl Rng {
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        let u = ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64;
        lo + (hi - lo) * u
    }
    fn vector(&mut self, scale: f64) -> DVec3 {
        DVec3::new(
            self.range(-scale, scale),
            self.range(-scale, scale),
            self.range(-scale, scale),
        )
    }
}
/// Count is per family: separation, local join, distant join, encounter, frame, transfer.
pub fn cases(seed: u64, count: usize) -> Vec<Case> {
    assert!(count > 0, "seam sweep: zero cases");
    let mut r = Rng(seed);
    let mut out = vec![];
    for i in 0..count {
        let axis = r.vector(1.0).normalize();
        let rotation = DQuat::from_axis_angle(axis, r.range(-3.0, 3.0));
        let cells = if i % 2 == 0 {
            [0; 3]
        } else {
            [10_i128.pow(24), -10_i128.pow(23), 10_i128.pow(22)]
        };
        for input in [
            Input::Separate {
                altitude: r.range(200_000.0, 1_000_000.0),
                rotation,
                velocity: r.vector(20.0),
                spin: r.vector(0.05),
            },
            Input::Join {
                rotation,
                velocity: r.vector(0.1),
                spin: r.vector(0.02),
                gap: r.range(0.01, 0.2),
                distant: false,
                cells,
            },
            Input::Join {
                rotation,
                velocity: r.vector(0.1),
                spin: r.vector(0.02),
                gap: r.range(0.01, 0.2),
                distant: true,
                cells,
            },
            Input::Encounter {
                distance: r.range(3750.0, 3950.0),
                speed: r.range(8.75, 9.25),
                miss: r.range(25.0, 35.0),
            },
            Input::Frame {
                position: r.vector(10.0),
                velocity: r.vector(100.0),
                time: r.range(0.0, 100.0),
                cells,
            },
            Input::Transfer {
                position: DVec3::new(
                    r.range(7.5e8, 8.5e8),
                    r.range(1.95e9, 2.05e9),
                    r.range(-1e7, 1e7),
                ),
                velocity: DVec3::new(r.range(480_000.0, 520_000.0), r.range(-100.0, 100.0), 0.0),
                cells,
            },
        ] {
            out.push(Case {
                version: VERSION,
                seed,
                index: out.len(),
                input,
            });
        }
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[should_panic(expected = "non-finite")]
    fn a_nan_observation_cannot_disappear_into_a_maximum() {
        maximum(0.0, f64::NAN);
    }
    #[test]
    fn failures_preserve_actual_inputs_and_never_overwrite_evidence() {
        let d = std::env::temp_dir().join(format!("void-seam-corpus-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        let case = cases(42, 1).remove(0);
        for _ in 0..2 {
            assert!(
                capture(&case, &d, || panic!("injected mismatch"))
                    .unwrap_err()
                    .contains("injected mismatch")
            );
        }
        let files: Vec<_> = fs::read_dir(&d).unwrap().collect();
        assert_eq!(files.len(), 2);
        for f in files {
            let read = Case::read(f.unwrap().path());
            assert_eq!(
                serde_json::to_value(read).unwrap(),
                serde_json::to_value(&case).unwrap()
            );
        }
        fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn deterministic_cases_cover_the_six_seams() {
        let a = cases(0x5eed, 2);
        assert_eq!(a.len(), 12);
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            serde_json::to_value(cases(0x5eed, 2)).unwrap()
        );
    }
}
