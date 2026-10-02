//! The labs' session log (`lab/landing/src/debug/LabLog.ts`): one JSON object per line, appended to
//! `lab-log/<stream>.jsonl` under this workspace, each stamped with the wall time, so a session can
//! be read back afterwards. Debug builds only, as the labs log only under the dev server.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

pub struct LabLog {
    file: BufWriter<File>,
}

impl LabLog {
    /// None in release builds.
    pub fn open(stream: &str) -> Option<Self> {
        if !cfg!(debug_assertions) {
            return None;
        }
        assert!(
            !stream.is_empty()
                && stream
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "lab log: invalid stream name {stream:?}"
        );
        let directory = concat!(env!("CARGO_MANIFEST_DIR"), "/../../lab-log");
        std::fs::create_dir_all(directory).expect("lab log: create lab-log/");
        let path = format!("{directory}/{stream}.jsonl");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap_or_else(|e| panic!("lab log: open {path}: {e}"));
        Some(Self {
            file: BufWriter::new(file),
        })
    }

    /// Append one event (a JSON object); `wall` comes first.
    pub fn write(&mut self, event: Value) {
        let Value::Object(fields) = event else {
            panic!("lab log: an event is a JSON object, got {event}");
        };
        let mut line = Map::new();
        line.insert("wall".into(), Value::String(wall_time()));
        line.extend(fields);
        serde_json::to_writer(&mut self.file, &Value::Object(line)).expect("lab log: write");
        self.file.write_all(b"\n").expect("lab log: write");
    }

    pub fn flush(&mut self) {
        self.file.flush().expect("lab log: flush");
    }
}

/// UTC now as JavaScript's `toISOString`: 2026-10-01T09:43:51.615Z.
fn wall_time() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after 1970");
    let (seconds, millis) = (now.as_secs() as i64, now.subsec_millis());
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn wall_time_is_iso() {
        let t = super::wall_time();
        assert_eq!(t.len(), 24, "{t}");
        assert!(
            t.starts_with("20") && t.ends_with('Z') && &t[10..11] == "T",
            "{t}"
        );
    }
}
