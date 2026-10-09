//! Optional, engine-free timing collection. Durations are system wall time, not process CPU time
//! or GPU time. Reports distinguish those quantities and never fabricate GPU statistics.
pub mod render;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path, time::Instant};

pub struct Profiler {
    epoch: Instant,
    spans: Vec<Value>,
    samples: BTreeMap<String, Vec<f64>>,
    threads: BTreeMap<String, u64>,
    counters: BTreeMap<String, (u64, f64, f64)>,
}
impl Default for Profiler {
    fn default() -> Self {
        Self::new()
    }
}
impl Profiler {
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
            spans: vec![],
            samples: BTreeMap::new(),
            threads: BTreeMap::new(),
            counters: BTreeMap::new(),
        }
    }
    pub fn sample(&mut self, name: &str, milliseconds: f64) {
        assert!(
            !name.is_empty() && milliseconds.is_finite() && milliseconds >= 0.0,
            "profiling: invalid timing sample"
        );
        self.samples
            .entry(name.into())
            .or_default()
            .push(milliseconds);
    }
    /// Count or gauge observations (units belong to the metric name), summarized online without retaining every frame.
    pub fn counter(&mut self, name: &str, value: f64) {
        assert!(
            !name.is_empty() && value.is_finite() && value >= 0.0,
            "profiling: invalid counter sample"
        );
        let entry = self.counters.entry(name.into()).or_insert((0, 0.0, 0.0));
        entry.0 += 1;
        entry.1 += value;
        entry.2 = entry.2.max(value);
    }
    pub fn span(&mut self, name: &str, start: Instant, end: Instant) {
        assert!(
            start >= self.epoch && end >= start,
            "profiling: invalid span timestamps"
        );
        let duration = end.duration_since(start).as_secs_f64();
        self.sample(name, duration * 1000.0);
        let key = format!("{:?}", std::thread::current().id());
        let next = self.threads.len() as u64 + 1;
        let tid = *self.threads.entry(key).or_insert(next);
        self.spans
            .push(json!({"name":name,"cat":"system_wall","ph":"X",
            "ts":start.duration_since(self.epoch).as_secs_f64()*1e6,"dur":duration*1e6,
            "pid":std::process::id(),"tid":tid}));
    }
    pub fn report(&self) -> Value {
        let metrics: BTreeMap<_, _> = self
            .samples
            .iter()
            .map(|(name, values)| {
                let mut sorted = values.clone();
                sorted.sort_by(f64::total_cmp);
                let quantile =
                    |p: f64| sorted[((sorted.len() as f64 * p).ceil() as usize).max(1) - 1];
                (
                    name,
                    json!({"samples":sorted.len(),"min_ms":sorted[0],
                "mean_ms":sorted.iter().sum::<f64>()/sorted.len() as f64,
                "p50_ms":quantile(0.5),"p95_ms":quantile(0.95),"max_ms":sorted[sorted.len()-1]}),
                )
            })
            .collect();
        let counters: BTreeMap<_, _> = self.counters.iter().map(|(name, (samples, total, max))|
            (name, json!({"samples": samples, "total": total, "mean": total / *samples as f64, "max": max}))).collect();
        json!({"counters": counters, "traceEvents":self.spans,"displayTimeUnit":"ms","metrics":metrics,
            "measurement":"system wall durations and supplied frame intervals; GPU time is not measured",
            "threads":self.threads})
    }
    pub fn write(&self, path: impl AsRef<Path>) {
        let path = path.as_ref();
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent).expect("profiling: create directory");
        let bytes = serde_json::to_vec(&self.report()).expect("profiling: encode report");
        fs::write(path, bytes).expect("profiling: write report");
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nearest_rank_quantiles_and_trace_units_are_explicit() {
        let mut profile = Profiler::new();
        for n in (1..=100).rev() {
            profile.sample("frame", n as f64);
        }
        let start = Instant::now();
        profile.span(
            "simulation",
            start,
            start + std::time::Duration::from_micros(250),
        );
        let report = profile.report();
        assert_eq!(report["metrics"]["frame"]["p50_ms"], 50.0);
        assert_eq!(report["metrics"]["frame"]["p95_ms"], 95.0);
        assert_eq!(report["metrics"]["frame"]["mean_ms"], 50.5);
        assert_eq!(report["traceEvents"][0]["dur"], 250.0);
        assert_eq!(report["metrics"]["simulation"]["p95_ms"], 0.25);
        assert_eq!(report["traceEvents"][0]["ph"], "X");
    }
    #[test]
    fn counters_have_dimensionless_online_summaries() {
        let mut profile = Profiler::new();
        profile.counter("tiles", 2.0);
        profile.counter("tiles", 4.0);
        let report = profile.report();
        assert_eq!(
            report["counters"]["tiles"],
            json!({
                "samples": 2, "total": 6.0, "mean": 3.0, "max": 4.0
            })
        );
        assert_eq!(report["metrics"], json!({}));
    }
    #[test]
    fn empty_capture_has_no_invented_samples() {
        assert_eq!(Profiler::new().report()["metrics"], json!({}));
    }
    #[test]
    #[should_panic(expected = "invalid timing sample")]
    fn nonfinite_timing_fails() {
        Profiler::new().sample("frame", f64::NAN);
    }
}
