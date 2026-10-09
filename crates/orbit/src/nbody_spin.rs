//! Opt-in SIMD workers with atomic generation/completion, no channel wakeup per phase.
use std::{
    any::Any,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
};
#[repr(align(64))]
struct Counter(AtomicU64);
pub(crate) struct SpinPool {
    q: Arc<RwLock<Vec<f64>>>,
    generation: Arc<Counter>,
    done: Arc<Counter>,
    stop: Arc<AtomicBool>,
    outputs: Vec<Arc<Mutex<Vec<f64>>>>,
    parked: Vec<Arc<AtomicBool>>,
    error: Arc<Mutex<Option<Box<dyn Any + Send>>>>,
    threads: Vec<JoinHandle<()>>,
}
impl SpinPool {
    pub fn new(gm: &[f64], count: usize) -> Self {
        assert!(std::is_x86_feature_detected!("avx2"), "AVX2 unavailable");
        assert!(
            count > 0 && count <= 8 && count <= gm.len(),
            "invalid SIMD worker count"
        );
        let n = gm.len();
        let gm: Arc<[f64]> = gm.into();
        let q = Arc::new(RwLock::new(vec![0.0f64; 3 * n]));
        let generation = Arc::new(Counter(AtomicU64::new(0)));
        let done = Arc::new(Counter(AtomicU64::new(0)));
        let stop = Arc::new(AtomicBool::new(false));
        let error = Arc::new(Mutex::new(None));
        let mut outputs = vec![];
        let mut threads = vec![];
        let mut parked = vec![];
        for worker in 0..count {
            let begin = worker * n / count;
            let end = (worker + 1) * n / count;
            let output = Arc::new(Mutex::new(vec![0.0; 3 * n]));
            outputs.push(output.clone());
            let (q, gm, generation, done, stop, error) = (
                q.clone(),
                gm.clone(),
                generation.clone(),
                done.clone(),
                stop.clone(),
                error.clone(),
            );
            let sleeping = Arc::new(AtomicBool::new(false));
            parked.push(sleeping.clone());
            threads.push(thread::spawn(move || {
                let mut seen = 0;
                let mut idle = 0;
                loop {
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    let next = generation.0.load(Ordering::Acquire);
                    if next == seen {
                        if idle < 2048 {
                            idle += 1;
                            std::hint::spin_loop();
                        } else {
                            // Publish before checking work again: an unpark token covers
                            // the race between this check and park (including Drop).
                            sleeping.store(true, Ordering::SeqCst);
                            if generation.0.load(Ordering::SeqCst) == seen
                                && !stop.load(Ordering::SeqCst)
                            {
                                thread::park();
                            }
                            sleeping.store(false, Ordering::SeqCst);
                            idle = 0;
                        }
                        continue;
                    }
                    idle = 0;
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let q = q.read().expect("SIMD worker input lock");
                        let mut out = output.lock().expect("SIMD worker output lock");
                        // SAFETY: AVX2 checked; owned arrays are 3*n, ranges fixed inside 0..n.
                        unsafe {
                            crate::nbody_simd::rows_range(&q, &gm, &mut out, begin, end);
                        }
                    }));
                    if let Err(e) = result {
                        *error.lock().expect("SIMD worker error lock") = Some(e);
                    }
                    seen = next;
                    done.0.fetch_add(1, Ordering::Release);
                }
            }));
        }
        Self {
            q,
            generation,
            done,
            stop,
            outputs,
            parked,
            error,
            threads,
        }
    }
    pub fn compute(&mut self, q: &[f64], out: &mut [f64]) {
        self.q.write().expect("SIMD input lock").copy_from_slice(q);
        self.done.0.store(0, Ordering::Relaxed);
        self.generation.0.fetch_add(1, Ordering::SeqCst);
        for (sleeping, worker) in self.parked.iter().zip(&self.threads) {
            if sleeping.load(Ordering::SeqCst) {
                worker.thread().unpark();
            }
        }
        while self.done.0.load(Ordering::Acquire) < self.outputs.len() as u64 {
            std::hint::spin_loop();
        }
        if let Some(e) = self.error.lock().expect("SIMD error lock").take() {
            std::panic::resume_unwind(e);
        }
        let n = q.len() / 3;
        for (worker, slot) in self.outputs.iter().enumerate() {
            let start = worker * n / self.outputs.len();
            let end = (worker + 1) * n / self.outputs.len();
            let slot = slot.lock().expect("SIMD output lock");
            out[3 * start..3 * end].copy_from_slice(&slot[3 * start..3 * end]);
        }
    }
}
impl Drop for SpinPool {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for worker in &self.threads {
            worker.thread().unpark();
        }
        for thread in self.threads.drain(..) {
            if thread.join().is_err() && !std::thread::panicking() {
                panic!("SIMD worker panicked");
            }
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn failure_reaches_owner_without_deadlock() {
        if !std::is_x86_feature_detected!("avx2") {
            return;
        }
        let mut pool = super::SpinPool::new(&[1.0; 8], 2);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                pool.compute(&[0.0; 24], &mut [0.0; 24]);
            }))
            .is_err()
        );
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[test]
    fn idle_workers_park_resume_and_drop() {
        if !std::is_x86_feature_detected!("avx2") {
            return;
        }
        let mut pool = SpinPool::new(&[1.0; 8], 2);
        let q: Vec<f64> = (0..24).map(|i| i as f64).collect();
        let mut out = [0.0; 24];
        let mut expected = [0.0; 24];
        unsafe {
            crate::nbody_simd::rows(&q, &[1.0; 8], &mut expected);
        }
        for _ in 0..3 {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while !pool.parked.iter().all(|p| p.load(Ordering::SeqCst)) {
                assert!(std::time::Instant::now() < deadline, "workers did not park");
                thread::yield_now();
            }
            pool.compute(&q, &mut out);
            assert_eq!(out.map(f64::to_bits), expected.map(f64::to_bits));
        }
    }
}
