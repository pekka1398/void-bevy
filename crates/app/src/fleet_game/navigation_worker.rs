//! A single owned child process keeps expensive numerical work off the render thread.
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Instant,
};
use void_fleet_flight::navigation_job::{NavigationJob, NavigationResult};

pub(super) const LIMIT_SECONDS: u64 = 120;
pub(super) struct Worker {
    child: Child,
    directory: PathBuf,
    pub started: Instant,
}
impl Worker {
    pub fn start(job: NavigationJob) -> Result<Self, String> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("void-navigation-{}-{sequence}", std::process::id()));
        std::fs::create_dir(&directory)
            .map_err(|e| format!("Cannot create navigation job: {e}"))?;
        let start = || -> Result<Child, String> {
            let input =
                std::fs::File::create(directory.join("input.json")).map_err(|e| e.to_string())?;
            serde_json::to_writer(std::io::BufWriter::new(input), &job)
                .map_err(|e| e.to_string())?;
            let log =
                std::fs::File::create(directory.join("worker.log")).map_err(|e| e.to_string())?;
            Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
                .arg("--navigation-worker")
                .arg(&directory)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(log)
                .spawn()
                .map_err(|e| format!("Cannot start navigation worker: {e}"))
        };
        match start() {
            Ok(child) => Ok(Self {
                child,
                directory,
                started: Instant::now(),
            }),
            Err(e) => {
                let _ = std::fs::remove_dir_all(directory);
                Err(e)
            }
        }
    }
    pub fn poll(&mut self) -> Option<Result<NavigationResult, String>> {
        match self.child.try_wait() {
            Ok(Some(status)) if status.success() => {
                let file = std::fs::File::open(self.directory.join("output.json"))
                    .expect("navigation worker succeeded without output");
                Some(
                    serde_json::from_reader(std::io::BufReader::new(file))
                        .expect("invalid navigation worker result"),
                )
            }
            Ok(Some(status)) => {
                let detail =
                    std::fs::read_to_string(self.directory.join("worker.log")).unwrap_or_default();
                panic!("navigation worker failed ({status}): {detail}");
            }
            Err(e) => panic!("cannot inspect owned navigation worker: {e}"),
            Ok(None) if self.started.elapsed().as_secs() >= LIMIT_SECONDS => {
                self.stop();
                Some(Err(format!(
                    "Navigation search exceeded {LIMIT_SECONDS}s; no plan changed. Reduce the wait/flight window or choose another departure state."
                )))
            }
            Ok(None) => None,
        }
    }
    fn stop(&mut self) {
        if self
            .child
            .try_wait()
            .expect("inspect navigation worker")
            .is_none()
        {
            // Child owns the exact PID; never discover or kill processes by name.
            self.child.kill().expect("cancel navigation worker");
            self.child.wait().expect("reap navigation worker");
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
/// Invoked before Bevy initialization: this process never creates a renderer or window.
pub(super) fn run(directory: &str) {
    let directory = std::path::Path::new(directory);
    let input = std::fs::File::open(directory.join("input.json")).expect("navigation input");
    let job: NavigationJob =
        serde_json::from_reader(std::io::BufReader::new(input)).expect("navigation job");
    let result = job.solve();
    let output = std::fs::File::create(directory.join("output.json")).expect("navigation output");
    serde_json::to_writer(std::io::BufWriter::new(output), &result)
        .expect("write navigation result");
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    fn sleeping_worker(label: &str) -> Worker {
        let directory = std::env::temp_dir().join(format!(
            "void-navigation-test-{}-{label}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let child = Command::new("sleep").arg("30").spawn().unwrap();
        Worker {
            child,
            directory,
            started: Instant::now(),
        }
    }
    #[test]
    fn navigation_cancel_and_timeout_reap_only_the_owned_child_and_remove_files() {
        let worker = sleeping_worker("cancel");
        let pid = worker.child.id();
        let directory = worker.directory.clone();
        drop(worker);
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
        assert!(!directory.exists());
        let mut worker = sleeping_worker("timeout");
        worker.started -= std::time::Duration::from_secs(LIMIT_SECONDS + 1);
        assert!(worker.poll().unwrap().unwrap_err().contains("exceeded"));
        assert!(worker.child.try_wait().unwrap().is_some());
    }
}
