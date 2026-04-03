#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SpecialKey {
    Enter,
    Escape,
    Backspace,
    Tab,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Insert,
    F(u8),
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    Special(SpecialKey),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
}

impl Modifiers {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn ctrl() -> Self {
        Self {
            ctrl: true,
            ..Default::default()
        }
    }

    pub fn shift() -> Self {
        Self {
            shift: true,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyPress {
    pub key: Key,
    pub modifiers: Modifiers,
}

impl KeyPress {
    pub fn new(key: Key, modifiers: Modifiers) -> Self {
        Self { key, modifiers }
    }

    pub fn char(c: char) -> Self {
        Self::new(Key::Char(c), Modifiers::none())
    }

    pub fn special(s: SpecialKey) -> Self {
        Self {
            key: Key::Special(s),
            modifiers: Modifiers::default(),
        }
    }
}

/// An ordered sequence of key presses forming a binding trigger.
pub type KeySequence = Vec<KeyPress>;
