//! Experimental persistent workers: independent target rows, exact scalar accumulation order.
use std::sync::{Arc, Mutex, RwLock, mpsc};
use std::thread::{self, JoinHandle};
pub(crate) struct Pool {
    q: Arc<RwLock<Vec<f64>>>,
    senders: Vec<mpsc::Sender<Option<Vec<f64>>>>,
    results: Mutex<mpsc::Receiver<(usize, std::thread::Result<Vec<f64>>)>>,
    buffers: Vec<Vec<f64>>,
    threads: Vec<JoinHandle<()>>,
}
impl Pool {
    pub fn new(gm: &[f64], workers: usize) -> Self {
        assert!(
            workers > 0 && workers <= 16 && workers <= gm.len(),
            "invalid N-body worker count"
        );
        let gm: Arc<[f64]> = gm.into();
        let n = gm.len();
        let q: Arc<RwLock<Vec<f64>>> = Arc::new(RwLock::new(vec![0.0; 3 * n]));
        let (result_tx, results) = mpsc::channel();
        let mut senders = vec![];
        let mut threads = vec![];
        let mut buffers = vec![];
        for worker in 0..workers {
            let start = worker * n / workers;
            let end = (worker + 1) * n / workers;
            let (tx, rx) = mpsc::channel::<Option<Vec<f64>>>();
            senders.push(tx);
            buffers.push(vec![0.0; (end - start) * 3]);
            let (q, gm, result_tx) = (q.clone(), gm.clone(), result_tx.clone());
            threads.push(thread::spawn(move || {
                while let Some(mut out) = rx.recv().expect("N-body worker command channel") {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let q = q.read().expect("N-body input lock");
                        for i in start..end {
                            let mut a = [0.0; 3];
                            for k in 0..n {
                                if k == i {
                                    continue;
                                }
                                let (lo, hi) = if k < i { (k, i) } else { (i, k) };
                                let dx = q[3 * hi] - q[3 * lo];
                                let dy = q[3 * hi + 1] - q[3 * lo + 1];
                                let dz = q[3 * hi + 2] - q[3 * lo + 2];
                                let r2 = dx * dx + dy * dy + dz * dz;
                                assert!(r2 > 0.0, "N-body coincident pair {lo}, {hi}");
                                let inv = 1.0 / (r2 * r2.sqrt());
                                let s = gm[k] * inv;
                                for (c, d) in [dx, dy, dz].into_iter().enumerate() {
                                    if k < i {
                                        a[c] -= d * s;
                                    } else {
                                        a[c] += d * s;
                                    }
                                }
                            }
                            out[3 * (i - start)..3 * (i - start) + 3].copy_from_slice(&a);
                        }
                        drop(q);
                        out
                    }));
                    result_tx
                        .send((worker, result))
                        .expect("N-body worker result channel");
                }
            }));
        }
        Self {
            q,
            senders,
            results: Mutex::new(results),
            buffers,
            threads,
        }
    }
    pub fn compute(&mut self, q: &[f64], out: &mut [f64]) {
        self.q
            .write()
            .expect("N-body input lock")
            .copy_from_slice(q);
        for (tx, buffer) in self.senders.iter().zip(&mut self.buffers) {
            tx.send(Some(std::mem::take(buffer)))
                .expect("N-body worker command channel");
        }
        for _ in 0..self.senders.len() {
            let (worker, buffer) = self
                .results
                .get_mut()
                .expect("N-body result lock")
                .recv()
                .expect("N-body worker result channel");
            let buffer = buffer.unwrap_or_else(|error| std::panic::resume_unwind(error));
            let start = worker * (q.len() / 3) / self.senders.len();
            out[3 * start..3 * start + buffer.len()].copy_from_slice(&buffer);
            self.buffers[worker] = buffer;
        }
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        for tx in &self.senders {
            let _ = tx.send(None);
        }
        for thread in self.threads.drain(..) {
            if thread.join().is_err() && !std::thread::panicking() {
                panic!("N-body worker panicked");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[should_panic(expected = "N-body coincident pair")]
    fn worker_failure_reaches_owner_without_deadlock() {
        let mut pool = super::Pool::new(&[1.0; 4], 2);
        pool.compute(&[0.0; 12], &mut [0.0; 12]);
    }
}
