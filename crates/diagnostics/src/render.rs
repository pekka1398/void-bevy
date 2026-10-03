//! Unit-aware render observations. A delivered GPU diagnostic is a sample, never a fabricated
//! zero for an unavailable query. Pass times remain separate because nested spans overlap.
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug)]
pub struct RenderValue {
    pub path: String,
    pub unit: String,
    pub value: f64,
}
#[derive(Default)]
pub struct RenderCapture {
    ids: BTreeSet<u32>,
    metrics: BTreeMap<String, (String, Vec<f64>)>,
}
impl RenderCapture {
    pub fn sample(&mut self, frame: u32, values: Vec<RenderValue>) {
        assert!(
            self.ids.insert(frame),
            "render capture: duplicate frame {frame}"
        );
        assert!(!values.is_empty(), "render capture: empty delivered frame");
        for v in values {
            assert!(
                !v.path.is_empty() && v.value.is_finite() && v.value >= 0.0,
                "render capture: invalid metric {}",
                v.path
            );
            let (unit, samples) = self
                .metrics
                .entry(v.path)
                .or_insert_with(|| (v.unit.clone(), vec![]));
            assert_eq!(*unit, v.unit, "render capture: metric unit changed");
            samples.push(v.value);
        }
    }
    pub fn frames(&self) -> usize {
        self.ids.len()
    }
    pub fn report(&self) -> Value {
        let metrics: BTreeMap<_, _> = self
            .metrics
            .iter()
            .map(|(name, (unit, values))| {
                let mut sorted = values.clone();
                sorted.sort_by(f64::total_cmp);
                let q = |p: f64| sorted[((sorted.len() as f64 * p).ceil() as usize).max(1) - 1];
                (
                    name,
                    json!({"unit":unit, "samples":sorted.len(), "min":sorted[0],
                "mean":sorted.iter().sum::<f64>()/sorted.len() as f64,
                "p50":q(0.5), "p95":q(0.95), "max":sorted[sorted.len()-1]}),
                )
            })
            .collect();
        json!({"delivered_frames":self.ids.len(), "frame_ids":self.ids, "metrics":metrics,
            "aggregation":"nearest-rank p50/p95 per diagnostic path; overlapping pass times are not summed"})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn render_query_units_and_missing_capabilities_remain_distinct() {
        let mut r = RenderCapture::default();
        assert!(r.report()["metrics"].as_object().unwrap().is_empty());
        for frame in 1..=20 {
            r.sample(
                frame,
                vec![
                    RenderValue {
                        path: "render/main/elapsed_gpu".into(),
                        unit: "ms".into(),
                        value: frame as f64 / 10.0,
                    },
                    RenderValue {
                        path: "render/main/clipper_primitives_out".into(),
                        unit: "count".into(),
                        value: frame as f64 * 3.0,
                    },
                ],
            );
        }
        let report = r.report();
        assert_eq!(report["metrics"]["render/main/elapsed_gpu"]["p50"], 1.0);
        assert_eq!(report["metrics"]["render/main/elapsed_gpu"]["p95"], 1.9);
        assert_eq!(
            report["metrics"]["render/main/clipper_primitives_out"]["unit"],
            "count"
        );
        assert!(report["metrics"].get("draw_calls").is_none());
    }
    #[test]
    #[should_panic(expected = "duplicate frame")]
    fn delivered_frames_cannot_be_counted_twice() {
        let mut r = RenderCapture::default();
        let value = || {
            vec![RenderValue {
                path: "render/time".into(),
                unit: "ms".into(),
                value: 1.0,
            }]
        };
        r.sample(1, value());
        r.sample(1, value());
    }
}

/// CPU draw API submissions. A GPU-counted multi-draw has an unknown number of records until its
/// count buffer is read; its max_count is never reported as the number actually issued.
#[derive(Clone, Copy, Debug, Default)]
pub struct DrawSubmissions {
    pub commands: u64,
    pub known_records: u64,
    pub gpu_counted_commands: u64,
}
impl DrawSubmissions {
    /// The exact detailed_trace messages in the pinned Bevy 0.19.1 TrackedRenderPass.
    pub fn observe(&mut self, message: &str) {
        let gpu_counted = message.starts_with("multi draw indirect count:")
            || message.starts_with("multi draw indexed indirect count:");
        let fixed_multi = message.starts_with("multi draw indirect:")
            || message.starts_with("multi draw indexed indirect:");
        let single = [
            "draw:",
            "draw indexed:",
            "draw indirect:",
            "draw indexed indirect:",
        ]
        .iter()
        .any(|p| message.starts_with(p));
        if !(gpu_counted || fixed_multi || single) {
            assert!(
                !message.starts_with("draw") && !message.starts_with("multi draw"),
                "draw capture: unknown draw event {message}"
            );
            return;
        }
        self.commands += 1;
        if gpu_counted {
            self.gpu_counted_commands += 1;
        } else if fixed_multi {
            let count = message
                .split_whitespace()
                .last()
                .unwrap()
                .strip_suffix('x')
                .expect("draw capture: multi-draw count suffix")
                .parse::<u64>()
                .expect("draw capture: multi-draw count");
            self.known_records += count;
        } else {
            self.known_records += 1;
        }
    }
}
#[cfg(test)]
mod draw_tests {
    use super::*;
    #[test]
    fn indirect_count_limits_are_not_claimed_as_actual_draws() {
        let mut d = DrawSubmissions::default();
        for message in [
            "draw: 0..3 0..1",
            "draw indexed: 0..12 0 0..8",
            "multi draw indirect: Buffer 0, 7x",
            "multi draw indexed indirect count: Buffer 0 (Buffer 0)x, max 99x",
        ] {
            d.observe(message);
        }
        assert_eq!(
            (d.commands, d.known_records, d.gpu_counted_commands),
            (4, 9, 1)
        );
    }
}
