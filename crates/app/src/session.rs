//! A flown session, written frame by frame and read back. One JSON object per line, as the labs'
//! session log is, so a recording can be read and edited by hand.
//!
//! Two things are being kept apart here, because they are different jobs and only the second one
//! catches anything:
//!
//! - **Reproduction.** The frames: enough to put the game back into the situation that went wrong,
//!   so it can be looked at. A recording alone does exactly this and nothing more — replay it and
//!   the bug happens again, and nothing says so.
//! - **Regression.** The marks: a state digest written every so often during the recording, which
//!   replay compares against. That is what turns a session into a check that fails by itself.
//!
//! A recording is only as deterministic as the game it drives, which is why the frame carries its
//! own `seconds` instead of a wall clock: replay does not depend on how fast the machine is. What it
//! does depend on is the simulation being a function of its state and the input, which the marks are
//! there to prove rather than assume.

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::input::{Input, Key};

/// What the session was flown against. A recording made on another planet is not a recording of
/// this one, so replay refuses it rather than producing a confusing divergence.
#[derive(Clone, Debug, PartialEq)]
pub struct Header {
    pub planet: String,
    pub terrain: String,
    /// The game's own build, so a recording from an older one is not silently trusted.
    pub version: u32,
}

/// The current frame format. Raised when a change would make older recordings replay differently.
pub const VERSION: u32 = 1;

/// A state digest at one moment of the flight, in the units a reader can check by eye: this is both
/// what replay compares and what a person reads to see what the session did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub frame: usize,
    pub sim_time: f64,
    /// Body-fixed position and velocity of the upper stage, metres and metres per second.
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub mass_kg: f64,
    pub stage: u8,
}

impl Mark {
    /// How far this mark is from another, as (metres, metres per second). Mass and stage are exact
    /// or they are a different flight, so they are compared rather than measured.
    pub fn distance(&self, other: &Mark) -> (f64, f64) {
        let d = |a: [f64; 3], b: [f64; 3]| {
            ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
        };
        (
            d(self.position, other.position),
            d(self.velocity, other.velocity),
        )
    }
}

#[derive(Clone, Debug)]
pub enum Line {
    Header(Header),
    Frame(Input),
    Mark(Mark),
}

/// A whole session in memory: the header, the frames in order, and the marks.
#[derive(Clone, Debug)]
pub struct Session {
    pub header: Header,
    pub frames: Vec<Input>,
    pub marks: Vec<Mark>,
}

impl Session {
    pub fn read(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        let file = File::open(path).unwrap_or_else(|e| panic!("session: open {path:?}: {e}"));
        let mut header = None;
        let (mut frames, mut marks) = (Vec::new(), Vec::new());
        for (n, line) in BufReader::new(file).lines().enumerate() {
            let line = line.unwrap_or_else(|e| panic!("session: read {path:?} line {n}: {e}"));
            if line.trim().is_empty() {
                continue;
            }
            match parse(&line) {
                Line::Header(h) => {
                    assert!(header.is_none(), "session: {path:?} has two headers");
                    assert_eq!(
                        h.version, VERSION,
                        "session: {path:?} was recorded by format {} and this build reads {VERSION}",
                        h.version
                    );
                    header = Some(h);
                }
                Line::Frame(f) => {
                    assert!(
                        header.is_some(),
                        "session: {path:?} starts without a header"
                    );
                    frames.push(f);
                }
                Line::Mark(m) => marks.push(m),
            }
        }
        Self {
            header: header.unwrap_or_else(|| panic!("session: {path:?} has no header")),
            frames,
            marks,
        }
    }

    /// Total simulated seconds, which is not the wall time it took to fly.
    pub fn seconds(&self) -> f64 {
        self.frames.iter().map(|f| f.seconds).sum()
    }
}

fn parse(line: &str) -> Line {
    let value: Value =
        serde_json::from_str(line).unwrap_or_else(|e| panic!("session: {line}: {e}"));
    let number = |key: &str| -> f64 {
        value[key]
            .as_f64()
            .unwrap_or_else(|| panic!("session: {key} is not a number in {line}"))
    };
    let triple = |key: &str| -> [f64; 3] {
        let a = value[key]
            .as_array()
            .unwrap_or_else(|| panic!("session: {key} is not an array in {line}"));
        assert_eq!(a.len(), 3, "session: {key} is not three numbers in {line}");
        [0, 1, 2].map(|i| {
            a[i].as_f64()
                .unwrap_or_else(|| panic!("session: {key} holds a non-number in {line}"))
        })
    };
    match value["kind"].as_str() {
        Some("header") => Line::Header(Header {
            planet: value["planet"]
                .as_str()
                .unwrap_or_else(|| panic!("session: no planet in {line}"))
                .into(),
            terrain: value["terrain"]
                .as_str()
                .unwrap_or_else(|| panic!("session: no terrain in {line}"))
                .into(),
            version: number("version") as u32,
        }),
        Some("frame") => {
            let mut input = Input::new(number("seconds"));
            let keys = |key: &str| -> Vec<Key> {
                match &value[key] {
                    Value::Null => Vec::new(),
                    Value::Array(a) => {
                        a.iter()
                            .map(|k| {
                                Key::from_name(k.as_str().unwrap_or_else(|| {
                                    panic!("session: {key} holds {k} in {line}")
                                }))
                            })
                            .collect()
                    }
                    other => panic!("session: {key} is {other} in {line}"),
                }
            };
            for key in keys("held") {
                input.hold(key);
            }
            for key in keys("pressed") {
                input.press(key);
            }
            if value.get("drag").is_some() {
                let d = triple("drag");
                input.drag = (d[0], d[1]);
            }
            if value.get("scroll").is_some() {
                input.scroll_pixels = number("scroll");
            }
            input.mouse_held = value["mouseHeld"].as_bool().unwrap_or(false);
            input.mouse_pressed = value["mousePressed"].as_bool().unwrap_or(false);
            Line::Frame(input)
        }
        Some("mark") => Line::Mark(Mark {
            frame: number("frame") as usize,
            sim_time: number("simTime"),
            position: triple("position"),
            velocity: triple("velocity"),
            mass_kg: number("massKg"),
            stage: number("stage") as u8,
        }),
        other => panic!("session: a line is {other:?}, not a header, frame or mark: {line}"),
    }
}

/// Writes a session as it is flown. Dropped at the end of the flight, which flushes it.
pub struct Recorder {
    file: BufWriter<File>,
    path: PathBuf,
    frames: usize,
}

impl Recorder {
    pub fn create(path: impl Into<PathBuf>, header: &Header) -> Self {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|e| panic!("session: create {parent:?}: {e}"));
        }
        let file = File::create(&path).unwrap_or_else(|e| panic!("session: create {path:?}: {e}"));
        let mut recorder = Self {
            file: BufWriter::new(file),
            path,
            frames: 0,
        };
        recorder.line(json!({
            "kind": "header",
            "version": VERSION,
            "planet": header.planet,
            "terrain": header.terrain,
        }));
        recorder
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    pub fn frame(&mut self, input: &Input) {
        self.frames += 1;
        // An idle frame is most of a warp, so it is written as time alone rather than as two empty
        // arrays: a quarter of an hour of warping is thousands of lines either way, but this way
        // they are readable.
        let mut fields = serde_json::Map::new();
        fields.insert("kind".into(), json!("frame"));
        fields.insert("seconds".into(), json!(input.seconds));
        let held: Vec<_> = input
            .held_keys()
            .filter(|&k| !input.just_pressed(k))
            .map(Key::name)
            .collect();
        let pressed: Vec<_> = input.pressed_keys().map(Key::name).collect();
        if !held.is_empty() {
            fields.insert("held".into(), json!(held));
        }
        if !pressed.is_empty() {
            fields.insert("pressed".into(), json!(pressed));
        }
        if input.drag != (0.0, 0.0) {
            fields.insert("drag".into(), json!([input.drag.0, input.drag.1, 0.0]));
        }
        if input.scroll_pixels != 0.0 {
            fields.insert("scroll".into(), json!(input.scroll_pixels));
        }
        if input.mouse_held {
            fields.insert("mouseHeld".into(), json!(true));
        }
        if input.mouse_pressed {
            fields.insert("mousePressed".into(), json!(true));
        }
        self.line(Value::Object(fields));
    }

    pub fn mark(&mut self, mark: &Mark) {
        self.line(json!({
            "kind": "mark",
            "frame": mark.frame,
            "simTime": mark.sim_time,
            "position": mark.position,
            "velocity": mark.velocity,
            "massKg": mark.mass_kg,
            "stage": mark.stage,
        }));
    }

    fn line(&mut self, value: Value) {
        serde_json::to_writer(&mut self.file, &value).expect("session: write");
        self.file.write_all(b"\n").expect("session: write");
    }

    pub fn flush(&mut self) {
        self.file.flush().expect("session: flush");
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Key;

    #[test]
    fn a_session_round_trips_through_a_file() {
        let directory = std::env::temp_dir().join("void-session-test");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("round-trip.jsonl");
        let header = Header {
            planet: "aurelia".into(),
            terrain: "layered".into(),
            version: VERSION,
        };
        let mut frames = Vec::new();
        let mut idle = Input::new(1.0 / 60.0);
        frames.push(idle);
        let mut staging = Input::new(1.0 / 60.0);
        staging.press(Key::Space).hold(Key::Shift);
        frames.push(staging);
        let mut steering = Input::new(0.02);
        steering.hold(Key::W).hold(Key::D);
        steering.drag = (3.0, -4.0);
        steering.scroll_pixels = 120.0;
        steering.mouse_held = true;
        frames.push(steering);
        idle.seconds = 0.05;
        frames.push(idle);
        let mark = Mark {
            frame: 2,
            sim_time: 0.0333,
            position: [6.4e6, 1.0, -2.0],
            velocity: [0.0, 1.5, 0.0],
            mass_kg: 9870.0,
            stage: 1,
        };
        {
            let mut recorder = Recorder::create(&path, &header);
            for frame in &frames {
                recorder.frame(frame);
            }
            recorder.mark(&mark);
            assert_eq!(recorder.frames(), frames.len());
        }
        let read = Session::read(&path);
        assert_eq!(read.header, header);
        assert_eq!(read.frames, frames, "the frames must come back identical");
        assert_eq!(read.marks, vec![mark]);
        assert_eq!(
            read.seconds(),
            frames.iter().map(|f| f.seconds).sum::<f64>()
        );
        // A held key that is also pressed is written once, as a press, and comes back as both.
        assert!(read.frames[1].held(Key::Space) && read.frames[1].just_pressed(Key::Space));
        assert!(read.frames[1].held(Key::Shift) && !read.frames[1].just_pressed(Key::Shift));
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    #[should_panic(expected = "starts without a header")]
    fn a_session_without_a_header_is_an_error() {
        let directory = std::env::temp_dir().join("void-session-test");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("headless.jsonl");
        std::fs::write(&path, "{\"kind\":\"frame\",\"seconds\":0.016}\n").unwrap();
        let _ = Session::read(&path);
    }
}
