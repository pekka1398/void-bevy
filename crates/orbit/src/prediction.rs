//! Explicit, deterministic resource limits for disposable prediction work.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Debug, Default)]
pub struct CancellationToken(Arc<AtomicBool>);
impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}
#[derive(Clone, Copy, Debug)]
pub struct PredictionBudget {
    pub bytes: usize,
    pub ephemeris_steps: u64,
    pub vessel_trials: u64,
}
impl Default for PredictionBudget {
    fn default() -> Self {
        Self {
            bytes: 256 * 1024 * 1024,
            ephemeris_steps: 2_000_000,
            vessel_trials: 4_000_000,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PredictionError {
    Cancelled,
    BudgetExceeded {
        resource: &'static str,
        required: u64,
        limit: u64,
    },
    UnsupportedSnapshot,
    IncompatibleSnapshot,
}
impl std::fmt::Display for PredictionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str("prediction cancelled"),
            Self::BudgetExceeded {
                resource,
                required,
                limit,
            } => write!(
                f,
                "prediction {resource} budget exceeded: requires {required}, limit {limit}"
            ),
            Self::IncompatibleSnapshot => f.write_str("prediction snapshot source is incompatible"),
            Self::UnsupportedSnapshot => {
                f.write_str("source does not support an exact prediction snapshot")
            }
        }
    }
}
impl std::error::Error for PredictionError {}
#[derive(Clone, Copy, Debug, Default)]
pub struct PredictionUsage {
    pub reserved_bytes: u64,
    pub ephemeris_steps: u64,
    pub vessel_trials: u64,
}
#[derive(Default, Debug)]
struct Usage {
    bytes: u64,
    steps: u64,
    trials: u64,
    error: Option<PredictionError>,
}
/// Shared by all local views of one job. Bytes conservatively count cumulative reservations,
/// including retained shared source data; dropping candidates never increases this allowance.
#[derive(Clone, Debug)]
pub struct PredictionContext {
    budget: PredictionBudget,
    cancel: CancellationToken,
    usage: Arc<Mutex<Usage>>,
}
impl PredictionContext {
    pub fn new(budget: PredictionBudget, cancel: CancellationToken) -> Self {
        Self {
            budget,
            cancel,
            usage: Arc::default(),
        }
    }
    pub fn check(&self) -> Result<(), PredictionError> {
        let mut u = self.usage.lock().expect("prediction usage poisoned");
        if self.cancel.is_cancelled() {
            u.error = Some(PredictionError::Cancelled);
        }
        match &u.error {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }
    fn charge(&self, resource: &'static str, amount: u64) -> Result<(), PredictionError> {
        self.check()?;
        let mut u = self.usage.lock().expect("prediction usage poisoned");
        let (current, limit) = match resource {
            "bytes" => (u.bytes, self.budget.bytes as u64),
            "ephemeris steps" => (u.steps, self.budget.ephemeris_steps),
            _ => (u.trials, self.budget.vessel_trials),
        };
        let required = current.saturating_add(amount);
        if required > limit {
            let e = PredictionError::BudgetExceeded {
                resource,
                required,
                limit,
            };
            u.error = Some(e.clone());
            return Err(e);
        }
        match resource {
            "bytes" => u.bytes = required,
            "ephemeris steps" => u.steps = required,
            _ => u.trials = required,
        }
        Ok(())
    }
    pub fn reject(&self, error: PredictionError) -> PredictionError {
        self.usage.lock().expect("prediction usage poisoned").error = Some(error.clone());
        error
    }
    pub fn usage(&self) -> PredictionUsage {
        let u = self.usage.lock().expect("prediction usage poisoned");
        PredictionUsage {
            reserved_bytes: u.bytes,
            ephemeris_steps: u.steps,
            vessel_trials: u.trials,
        }
    }
    /// Reject impossible requested intervals before doing any integration or allocation.
    pub fn preflight_extension(&self, steps: u64, bytes: usize) -> Result<(), PredictionError> {
        self.check()?;
        let mut u = self.usage.lock().expect("prediction usage poisoned");
        let required = u.steps.saturating_add(steps);
        if required > self.budget.ephemeris_steps {
            let e = PredictionError::BudgetExceeded {
                resource: "ephemeris steps",
                required,
                limit: self.budget.ephemeris_steps,
            };
            u.error = Some(e.clone());
            return Err(e);
        }
        let required = u.bytes.saturating_add(bytes as u64);
        if required > self.budget.bytes as u64 {
            let e = PredictionError::BudgetExceeded {
                resource: "bytes",
                required,
                limit: self.budget.bytes as u64,
            };
            u.error = Some(e.clone());
            return Err(e);
        }
        Ok(())
    }
    pub fn reserve_bytes(&self, bytes: usize) -> Result<(), PredictionError> {
        self.charge("bytes", bytes as u64)
    }
    pub fn ephemeris_step(&self) -> Result<(), PredictionError> {
        self.charge("ephemeris steps", 1)
    }
    pub fn vessel_trial(&self) -> Result<(), PredictionError> {
        self.charge("vessel trials", 1)
    }
}
/// Transferable exact continuation; materialize thread-local Rc views only on the worker.
pub trait PredictionSnapshot: Send {
    fn into_source(self: Box<Self>) -> Box<dyn crate::EphemerisSource>;
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any + Send>;
}
