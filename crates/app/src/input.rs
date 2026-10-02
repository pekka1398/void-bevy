//! What the game reads from the pilot in one frame, as data rather than as a borrow of Bevy's
//! `ButtonInput`. The window fills it from the keyboard and mouse; a recorded session fills it from
//! a file. The game cannot tell the difference, which is the point: the same frames that were flown
//! in the window can be flown again headlessly, in a test, with assertions on what came out.
//!
//! This is the same shape as the replaceable sources elsewhere in the workspace — the ephemeris the
//! propagator reads, the air the rocket flies through — moved one layer out to the pilot.

use std::fmt;

/// Every key the game acts on. Named for what the key is, not for what it does, because the
/// recording is a record of what the pilot pressed; what that means is the game's business and may
/// change without making old recordings unreadable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Key {
    Space,
    Tab,
    Comma,
    Period,
    Delete,
    Backspace,
    BracketLeft,
    BracketRight,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    PageUp,
    PageDown,
    Home,
    End,
    F1,
    F2,
    F3,
    F4,
    F5,
    A,
    B,
    D,
    E,
    G,
    K,
    L,
    N,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Shift,
    Control,
    Alt,
}

impl Key {
    /// Every key, in declaration order, which is also the order of the bits in an `Input`.
    pub const ALL: [Key; 42] = [
        Key::Space,
        Key::Tab,
        Key::Comma,
        Key::Period,
        Key::Delete,
        Key::Backspace,
        Key::BracketLeft,
        Key::BracketRight,
        Key::ArrowUp,
        Key::ArrowDown,
        Key::ArrowLeft,
        Key::ArrowRight,
        Key::PageUp,
        Key::PageDown,
        Key::Home,
        Key::End,
        Key::F1,
        Key::F2,
        Key::F3,
        Key::F4,
        Key::F5,
        Key::A,
        Key::B,
        Key::D,
        Key::E,
        Key::G,
        Key::K,
        Key::L,
        Key::N,
        Key::P,
        Key::Q,
        Key::R,
        Key::S,
        Key::T,
        Key::U,
        Key::V,
        Key::W,
        Key::X,
        Key::Y,
        Key::Shift,
        Key::Control,
        Key::Alt,
    ];

    fn bit(self) -> u64 {
        1 << Key::ALL
            .iter()
            .position(|&k| k == self)
            .expect("every key is in ALL")
    }

    /// The name a recording stores. Short, and the same as the key cap where there is one.
    pub fn name(self) -> &'static str {
        match self {
            Key::Space => "Space",
            Key::Tab => "Tab",
            Key::Comma => "Comma",
            Key::Period => "Period",
            Key::Delete => "Delete",
            Key::Backspace => "Backspace",
            Key::BracketLeft => "BracketLeft",
            Key::BracketRight => "BracketRight",
            Key::ArrowUp => "ArrowUp",
            Key::ArrowDown => "ArrowDown",
            Key::ArrowLeft => "ArrowLeft",
            Key::ArrowRight => "ArrowRight",
            Key::PageUp => "PageUp",
            Key::PageDown => "PageDown",
            Key::Home => "Home",
            Key::End => "End",
            Key::F1 => "F1",
            Key::F2 => "F2",
            Key::F3 => "F3",
            Key::F4 => "F4",
            Key::F5 => "F5",
            Key::A => "A",
            Key::B => "B",
            Key::D => "D",
            Key::E => "E",
            Key::G => "G",
            Key::K => "K",
            Key::L => "L",
            Key::N => "N",
            Key::P => "P",
            Key::Q => "Q",
            Key::R => "R",
            Key::S => "S",
            Key::T => "T",
            Key::U => "U",
            Key::V => "V",
            Key::W => "W",
            Key::X => "X",
            Key::Y => "Y",
            Key::Shift => "Shift",
            Key::Control => "Control",
            Key::Alt => "Alt",
        }
    }

    /// The key a recording names. A name this build does not know is an error rather than a key
    /// quietly dropped: a recording that cannot be replayed exactly is not a recording.
    pub fn from_name(name: &str) -> Key {
        *Key::ALL
            .iter()
            .find(|k| k.name() == name)
            .unwrap_or_else(|| panic!("input: unknown key {name:?}"))
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One frame of pilot input, and the simulated time it is to be flown for.
///
/// `held` and `pressed` are the two questions the game asks: a key that is down now, and a key that
/// went down this frame. Both are needed — the throttle creeps while shift is held, while staging
/// happens once per press — and both are recorded, rather than derived on replay from the previous
/// frame, because a frame dropped on the way to the file would otherwise turn a hold into a press.
/// A resolved map-label click, independent of cursor position and window size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusTarget {
    Vessel,
    Body(usize),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Input {
    /// Simulated seconds this frame covers, already clamped by whoever produced it.
    pub seconds: f64,
    held: u64,
    pressed: u64,
    /// Mouse drag in pixels while a button is down, and the wheel in pixels.
    pub drag: (f64, f64),
    pub scroll_pixels: f64,
    pub mouse_held: bool,
    pub mouse_pressed: bool,
    pub focus: Option<FocusTarget>,
}

impl Input {
    pub fn new(seconds: f64) -> Self {
        Self {
            seconds,
            ..Default::default()
        }
    }

    pub fn hold(&mut self, key: Key) -> &mut Self {
        self.held |= key.bit();
        self
    }

    /// A key that went down this frame, which is also held: nothing goes down without being down.
    pub fn press(&mut self, key: Key) -> &mut Self {
        self.pressed |= key.bit();
        self.held |= key.bit();
        self
    }

    pub fn held(&self, key: Key) -> bool {
        self.held & key.bit() != 0
    }

    pub fn just_pressed(&self, key: Key) -> bool {
        self.pressed & key.bit() != 0
    }

    pub fn any_just_pressed(&self, keys: [Key; 2]) -> bool {
        keys.iter().any(|&k| self.just_pressed(k))
    }

    /// +1 while `positive` is held, −1 while `negative` is, 0 for neither or both.
    pub fn axis(&self, positive: Key, negative: Key) -> f64 {
        self.held(positive) as i32 as f64 - self.held(negative) as i32 as f64
    }

    pub fn held_keys(&self) -> impl Iterator<Item = Key> + '_ {
        Key::ALL.into_iter().filter(|&k| self.held(k))
    }

    pub fn pressed_keys(&self) -> impl Iterator<Item = Key> + '_ {
        Key::ALL.into_iter().filter(|&k| self.just_pressed(k))
    }

    /// Nothing happened this frame but time passing, which is most frames of a warp.
    pub fn is_idle(&self) -> bool {
        self.held == 0
            && self.pressed == 0
            && self.drag == (0.0, 0.0)
            && self.scroll_pixels == 0.0
            && !self.mouse_held
            && !self.mouse_pressed
            && self.focus.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_has_its_own_bit_and_name() {
        let mut bits = 0u64;
        for key in Key::ALL {
            assert_eq!(bits & key.bit(), 0, "{key} shares a bit");
            bits |= key.bit();
            assert_eq!(Key::from_name(key.name()), key);
        }
        assert_eq!(bits.count_ones() as usize, Key::ALL.len());
    }

    #[test]
    fn a_press_is_also_a_hold_but_not_the_other_way() {
        let mut input = Input::new(1.0 / 60.0);
        input.press(Key::Space).hold(Key::Shift);
        assert!(input.just_pressed(Key::Space) && input.held(Key::Space));
        assert!(input.held(Key::Shift) && !input.just_pressed(Key::Shift));
        assert!(!input.is_idle());
        assert_eq!(input.axis(Key::Shift, Key::Control), 1.0);
        assert_eq!(input.axis(Key::Control, Key::Shift), -1.0);
        assert_eq!(Input::new(0.5).axis(Key::W, Key::S), 0.0);
        assert!(Input::new(0.5).is_idle());
    }

    #[test]
    #[should_panic(expected = "unknown key")]
    fn an_unknown_key_name_is_an_error() {
        Key::from_name("Escape");
    }
}
