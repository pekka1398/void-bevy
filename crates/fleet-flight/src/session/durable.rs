//! Durable command intentions precede simulation; outcomes commit commands afterwards.
//! Incomplete recordings require an explicit recovery operation, never implicit tail skipping.
use super::*;
use std::fs::File;

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Line {
    Header {
        journal: String,
        recording: Box<Recording>,
    },
    Intent {
        index: usize,
        action: Action,
    },
    Commit {
        index: usize,
        outcome: Outcome,
    },
    Mark {
        mark: Mark,
    },
    End {
        actions: usize,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingCommand {
    pub index: usize,
    pub action: Action,
}
pub struct Recovery {
    pub recording: Recording,
    pub pending: Option<PendingCommand>,
    pub discarded_tail_bytes: usize,
    pub ended_normally: bool,
}
impl Recovery {
    /// Reconstruct and verify the durable prefix, keeping an uncommitted command for investigation.
    pub fn read(path: impl AsRef<Path>) -> Self {
        decode(&fs::read(path).expect("journal: read file"), true)
    }
    pub fn write(&self, path: impl AsRef<Path>) {
        assert!(
            !path.as_ref().exists(),
            "journal: recovered destination must not exist"
        );
        self.recording.write(path.as_ref());
        let report = serde_json::json!({"pending":self.pending,"discarded_tail_bytes":self.discarded_tail_bytes,
            "ended_normally":self.ended_normally,"committed_actions":self.recording.entries.len()});
        let mut report_path = path.as_ref().as_os_str().to_os_string();
        report_path.push(".recovery.json");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(std::path::PathBuf::from(report_path))
            .expect("journal: create recovery report");
        serde_json::to_writer(&mut file, &report).expect("journal: serialize recovery report");
        file.write_all(b"\n")
            .expect("journal: finish recovery report");
        file.sync_all().expect("journal: sync recovery report");
    }
}

pub(super) struct Writer {
    file: File,
    pending: Option<usize>,
}
impl Writer {
    pub(super) fn create(path: &Path, recording: Recording) -> Self {
        recording.validate();
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent).expect("journal: create directory");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .expect("journal: create new recording (destination must not exist)");
        let mut writer = Self {
            file,
            pending: None,
        };
        writer.line(Line::Header {
            journal: "fleet-stream-v1".into(),
            recording: Box::new(recording),
        });
        File::open(parent)
            .expect("journal: open directory")
            .sync_all()
            .expect("journal: sync directory");
        writer
    }
    fn line(&mut self, line: Line) {
        // Encode fully before touching the file; an I/O failure may leave only an explicit EOF tail.
        let mut bytes = serde_json::to_vec(&line).expect("journal: encode line");
        bytes.push(b'\n');
        self.file.write_all(&bytes).expect("journal: write line");
        self.file.sync_data().expect("journal: sync line");
    }
    pub(super) fn intent(&mut self, index: usize, action: &Action) {
        assert!(
            self.pending.is_none(),
            "journal: previous command did not commit"
        );
        self.line(Line::Intent {
            index,
            action: action.clone(),
        });
        self.pending = Some(index);
    }
    pub(super) fn commit(&mut self, index: usize, outcome: &Outcome) {
        assert_eq!(self.pending, Some(index), "journal: command index changed");
        self.line(Line::Commit {
            index,
            outcome: outcome.clone(),
        });
        self.pending = None;
    }
    pub(super) fn mark(&mut self, mark: Mark) {
        assert!(self.pending.is_none(), "journal: mark during a command");
        self.line(Line::Mark { mark });
    }
    pub(super) fn finish(&mut self, actions: usize) {
        assert!(self.pending.is_none(), "journal: unfinished command");
        self.line(Line::End { actions });
    }
}
pub(super) fn is_stream(bytes: &[u8]) -> bool {
    let first: serde_json::Value = serde_json::Deserializer::from_slice(bytes)
        .into_iter()
        .next()
        .expect("session: empty file")
        .expect("session: invalid first object");
    first.get("kind").and_then(|v| v.as_str()) == Some("Header")
}
pub(super) fn read_complete(bytes: &[u8]) -> Recording {
    decode(bytes, false).recording
}
fn decode(bytes: &[u8], recover: bool) -> Recovery {
    let complete_end = bytes.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    let tail = bytes.len() - complete_end;
    assert!(
        recover || tail == 0,
        "journal: incomplete EOF line; use --recover-recording"
    );
    let mut recording: Option<Recording> = None;
    let mut pending: Option<PendingCommand> = None;
    let mut ended = false;
    for (n, line) in bytes[..complete_end].split(|&b| b == b'\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        assert!(!ended, "journal: data after end marker");
        let line: Line = serde_json::from_slice(line)
            .unwrap_or_else(|e| panic!("journal: invalid line {}: {e}", n + 1));
        match line {
            Line::Header {
                journal,
                recording: initial,
            } => {
                assert_eq!(journal, "fleet-stream-v1", "journal: unsupported format");
                assert!(recording.is_none() && n == 0, "journal: misplaced header");
                initial.validate();
                recording = Some(*initial);
            }
            Line::Intent { index, action } => {
                let r = recording.as_ref().expect("journal: missing header");
                assert!(pending.is_none(), "journal: two uncommitted intentions");
                assert_eq!(index, r.entries.len(), "journal: intention index gap");
                pending = Some(PendingCommand { index, action });
            }
            Line::Commit { index, outcome } => {
                let p = pending.take().expect("journal: commit without intention");
                assert_eq!(index, p.index, "journal: commit index gap");
                recording
                    .as_mut()
                    .expect("journal: missing header")
                    .entries
                    .push(Entry {
                        action: p.action,
                        outcome,
                    });
            }
            Line::Mark { mark } => {
                assert!(pending.is_none(), "journal: mark before commit");
                let r = recording.as_mut().expect("journal: missing header");
                assert_eq!(
                    mark.after_actions,
                    r.entries.len(),
                    "journal: misplaced mark"
                );
                if r.marks
                    .last()
                    .is_some_and(|m| m.after_actions == mark.after_actions)
                {
                    assert_eq!(
                        r.marks.last().unwrap().state,
                        mark.state,
                        "journal: mark changed without a command"
                    );
                    *r.marks.last_mut().unwrap() = mark;
                } else {
                    r.marks.push(mark);
                }
            }
            Line::End { actions } => {
                assert!(
                    pending.is_none(),
                    "journal: ended with an unfinished command"
                );
                let r = recording.as_ref().expect("journal: missing header");
                assert_eq!(actions, r.entries.len(), "journal: wrong final count");
                r.validate();
                ended = true;
            }
        }
    }
    assert!(
        recover || ended,
        "journal: recording did not finish; use --recover-recording"
    );
    let mut recording = recording.expect("journal: missing complete header");
    if !recover {
        recording.validate();
        return Recovery {
            recording,
            pending,
            discarded_tail_bytes: tail,
            ended_normally: ended,
        };
    }
    // Re-execute only committed commands, checking every mark and outcome. No guess about pending effects.
    let mut sim = recording
        .base
        .as_ref()
        .map_or_else(|| recording.initial.build(), |base| base.restore());
    let mut marks = recording.marks.iter().peekable();
    for index in 0..=recording.entries.len() {
        if index > 0 {
            let entry = &recording.entries[index - 1];
            assert_eq!(
                entry.action.apply(&mut sim),
                entry.outcome,
                "journal: changed command outcome at {index}"
            );
        }
        if marks.peek().is_some_and(|m| m.after_actions == index) {
            assert_eq!(
                world_mark(&sim),
                marks.next().unwrap().state,
                "journal: state diverged at {index}"
            );
        }
    }
    assert!(marks.next().is_none(), "journal: unreached mark");
    if recording.marks.last().unwrap().after_actions < recording.entries.len() {
        assert!(recover, "journal: missing final mark");
        recording.marks.push(Mark {
            after_actions: recording.entries.len(),
            state: world_mark(&sim),
        });
    }
    recording.validate();
    Recovery {
        recording,
        pending,
        discarded_tail_bytes: tail,
        ended_normally: ended,
    }
}
