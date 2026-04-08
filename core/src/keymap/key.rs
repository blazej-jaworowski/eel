#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

/// Errors produced by [`parse_key_sequence`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseKeyError {
    #[error("unclosed '<' in key sequence")]
    UnclosedBracket,
    #[error("unknown key notation '<{0}>'")]
    UnknownNotation(String),
}

impl TryFrom<&str> for SpecialKey {
    type Error = ParseKeyError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        let key = match s {
            "Enter" => Self::Enter,
            "Escape" => Self::Escape,
            "Backspace" => Self::Backspace,
            "Tab" => Self::Tab,
            "Up" => Self::Up,
            "Down" => Self::Down,
            "Left" => Self::Left,
            "Right" => Self::Right,
            "Home" => Self::Home,
            "End" => Self::End,
            "PageUp" => Self::PageUp,
            "PageDown" => Self::PageDown,
            "Delete" => Self::Delete,
            "Insert" => Self::Insert,
            _ => {
                return s
                    .strip_prefix('F')
                    .and_then(|n| n.parse::<u8>().ok())
                    .map(Self::F)
                    .ok_or_else(|| ParseKeyError::UnknownNotation(s.to_string()));
            }
        };

        Ok(key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    Char(char),
    Special(SpecialKey),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

/// Parse a key-sequence notation string into a [`Vec<KeyPress>`].
///
/// Plain characters are each a single key press.  Angle-bracket notations
/// `<…>` denote a single key press with optional modifiers.
///
/// **Modifier prefixes** (case-sensitive, may be combined):
/// - `C-` — Ctrl
/// - `S-` — Shift
///
/// **Canonical shift model** (same as the rest of eel):
/// - `<S-letter>` → uppercase letter with no explicit shift modifier  
///   e.g. `<S-a>` = `KeyPress::char('A')`
/// - `<C-letter>` → ctrl + lowercase letter  
///   e.g. `<C-A>` = `ctrl + 'a'`
/// - `<C-S-letter>` → ctrl+shift + lowercase letter
///
/// **Special key names** (case-sensitive, canonical name per key):
/// `<Enter>`, `<Escape>`, `<Backspace>`, `<Tab>`,
/// `<Up>`, `<Down>`, `<Left>`, `<Right>`,
/// `<Home>`, `<End>`, `<PageUp>`, `<PageDown>`,
/// `<Delete>`, `<Insert>`, `<F1>`..`<F12>`,
/// `<LT>` → `'<'` (since `<` is the angle-bracket delimiter)
///
/// # Errors
///
/// Returns [`ParseKeyError::UnclosedBracket`] if a `<` is never closed, or
/// [`ParseKeyError::UnknownNotation`] if an angle-bracket token is unrecognised.
pub fn parse_key_sequence(s: &str) -> Result<Vec<KeyPress>, ParseKeyError> {
    let mut s = s;
    let mut result = Vec::new();
    while !s.is_empty() {
        result.push(parse_next_key(&mut s)?);
    }
    Ok(result)
}

fn parse_next_key(s: &mut &str) -> Result<KeyPress, ParseKeyError> {
    if let Some(rest) = s.strip_prefix('<') {
        let end = rest.find('>').ok_or(ParseKeyError::UnclosedBracket)?;
        *s = &rest[end + 1..];
        return parse_angle_bracket(&rest[..end]);
    }

    let c = s.chars().next().unwrap();
    *s = &s[c.len_utf8()..];
    Ok(KeyPress::char(c))
}

fn parse_angle_bracket(inner: &str) -> Result<KeyPress, ParseKeyError> {
    let mut mods = Modifiers::none();
    let mut rest = inner;

    // Strip modifier prefixes (C- and S-) case-sensitively; allow any order.
    loop {
        match rest.as_bytes() {
            [b'C', b'-', ..] => {
                mods.ctrl = true;
                rest = &rest[2..];
            }
            [b'S', b'-', ..] => {
                mods.shift = true;
                rest = &rest[2..];
            }
            _ => break,
        }
    }

    // <LT> is a special case: '<' is the angle-bracket delimiter and cannot
    // be written as a plain character in a sequence string.
    if rest == "LT" {
        return Ok(KeyPress::new(Key::Char('<'), mods));
    }

    let key = if rest.chars().count() == 1 {
        let c = rest.chars().next().unwrap();
        if mods.ctrl {
            // Normalise to lowercase regardless of how the user wrote it.
            Key::Char(c.to_ascii_lowercase())
        } else if mods.shift && c.is_ascii_alphabetic() {
            // <S-letter>: absorb shift into uppercase, clear the modifier.
            mods.shift = false;
            Key::Char(c.to_ascii_uppercase())
        } else {
            Key::Char(c)
        }
    } else {
        Key::Special(SpecialKey::try_from(rest)?)
    };

    Ok(KeyPress::new(key, mods))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kp(c: char) -> KeyPress {
        KeyPress::char(c)
    }

    fn ctrl(c: char) -> KeyPress {
        KeyPress::new(Key::Char(c), Modifiers::ctrl())
    }

    fn shift_special(s: SpecialKey) -> KeyPress {
        KeyPress::new(Key::Special(s), Modifiers::shift())
    }

    fn ctrl_special(s: SpecialKey) -> KeyPress {
        KeyPress::new(Key::Special(s), Modifiers::ctrl())
    }

    fn seq(s: &str) -> Vec<KeyPress> {
        parse_key_sequence(s).unwrap()
    }

    #[test]
    fn plain_lowercase() {
        assert_eq!(seq("abc"), vec![kp('a'), kp('b'), kp('c')]);
    }

    #[test]
    fn plain_uppercase() {
        assert_eq!(seq("ABC"), vec![kp('A'), kp('B'), kp('C')]);
    }

    #[test]
    fn plain_digit() {
        assert_eq!(seq("123"), vec![kp('1'), kp('2'), kp('3')]);
    }

    #[test]
    fn plain_symbol() {
        assert_eq!(seq("!?"), vec![kp('!'), kp('?')]);
    }

    #[test]
    fn enter() {
        assert_eq!(seq("<Enter>"), vec![KeyPress::special(SpecialKey::Enter)]);
    }

    #[test]
    fn escape() {
        assert_eq!(seq("<Escape>"), vec![KeyPress::special(SpecialKey::Escape)]);
    }

    #[test]
    fn backspace() {
        assert_eq!(
            seq("<Backspace>"),
            vec![KeyPress::special(SpecialKey::Backspace)]
        );
    }

    #[test]
    fn tab() {
        assert_eq!(seq("<Tab>"), vec![KeyPress::special(SpecialKey::Tab)]);
    }

    #[test]
    fn arrow_keys() {
        assert_eq!(
            seq("<Up><Down><Left><Right>"),
            vec![
                KeyPress::special(SpecialKey::Up),
                KeyPress::special(SpecialKey::Down),
                KeyPress::special(SpecialKey::Left),
                KeyPress::special(SpecialKey::Right),
            ]
        );
    }

    #[test]
    fn navigation_keys() {
        assert_eq!(
            seq("<Home><End><PageUp><PageDown>"),
            vec![
                KeyPress::special(SpecialKey::Home),
                KeyPress::special(SpecialKey::End),
                KeyPress::special(SpecialKey::PageUp),
                KeyPress::special(SpecialKey::PageDown),
            ]
        );
    }

    #[test]
    fn delete_insert() {
        assert_eq!(
            seq("<Delete><Insert>"),
            vec![
                KeyPress::special(SpecialKey::Delete),
                KeyPress::special(SpecialKey::Insert),
            ]
        );
    }

    #[test]
    fn function_keys() {
        assert_eq!(
            seq("<F1><F6><F12>"),
            vec![
                KeyPress::special(SpecialKey::F(1)),
                KeyPress::special(SpecialKey::F(6)),
                KeyPress::special(SpecialKey::F(12)),
            ]
        );
    }

    #[test]
    fn lt() {
        assert_eq!(seq("<LT>"), vec![kp('<')]);
    }

    #[test]
    fn ctrl_lowercase() {
        assert_eq!(seq("<C-j>"), vec![ctrl('j')]);
    }

    #[test]
    fn ctrl_uppercase_normalised() {
        // <C-A> is normalised to ctrl+'a' (same as <C-a>)
        assert_eq!(seq("<C-A>"), vec![ctrl('a')]);
        assert_eq!(seq("<C-a>"), vec![ctrl('a')]);
    }

    #[test]
    fn shift_letter_absorbed_into_case() {
        // <S-a> → uppercase 'A', no shift modifier
        assert_eq!(seq("<S-a>"), vec![kp('A')]);
        assert_eq!(seq("<S-A>"), vec![kp('A')]);
    }

    #[test]
    fn ctrl_shift_letter() {
        let expected = KeyPress::new(
            Key::Char('a'),
            Modifiers {
                ctrl: true,
                shift: true,
            },
        );
        assert_eq!(seq("<C-S-a>"), vec![expected.clone()]);
        assert_eq!(seq("<S-C-a>"), vec![expected]);
    }

    #[test]
    fn shift_special_key() {
        assert_eq!(seq("<S-Up>"), vec![shift_special(SpecialKey::Up)]);
        assert_eq!(seq("<S-Tab>"), vec![shift_special(SpecialKey::Tab)]);
    }

    #[test]
    fn ctrl_special_key() {
        assert_eq!(seq("<C-Up>"), vec![ctrl_special(SpecialKey::Up)]);
    }

    #[test]
    fn mixed_plain_and_special() {
        assert_eq!(seq("g<C-j>x"), vec![kp('g'), ctrl('j'), kp('x')]);
    }

    #[test]
    fn multi_key_sequence() {
        assert_eq!(seq("gg"), vec![kp('g'), kp('g')]);
    }

    #[test]
    fn unclosed_angle_bracket() {
        assert_eq!(
            parse_key_sequence("<C-a"),
            Err(ParseKeyError::UnclosedBracket)
        );
    }

    #[test]
    fn unknown_notation() {
        assert!(matches!(
            parse_key_sequence("<Bogus>"),
            Err(ParseKeyError::UnknownNotation(_))
        ));
    }

    #[test]
    fn wrong_case_special_key_is_error() {
        // Special key names are case-sensitive.
        assert!(matches!(
            parse_key_sequence("<escape>"),
            Err(ParseKeyError::UnknownNotation(_))
        ));
        assert!(matches!(
            parse_key_sequence("<ENTER>"),
            Err(ParseKeyError::UnknownNotation(_))
        ));
        assert!(matches!(
            parse_key_sequence("<pageup>"),
            Err(ParseKeyError::UnknownNotation(_))
        ));
    }

    #[test]
    fn lowercase_modifier_is_error() {
        // Modifier prefixes are case-sensitive: only C- and S-, not c- or s-.
        // A lowercase modifier is not stripped, so the whole token is treated
        // as an unknown key name.
        assert!(matches!(
            parse_key_sequence("<c-a>"),
            Err(ParseKeyError::UnknownNotation(_))
        ));
        assert!(matches!(
            parse_key_sequence("<s-Up>"),
            Err(ParseKeyError::UnknownNotation(_))
        ));
    }
}
