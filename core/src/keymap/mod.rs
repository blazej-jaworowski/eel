pub mod action;
pub mod key {
    pub use eel_key_parse::*;
}
pub mod map;

#[cfg(feature = "modal")]
pub mod modal;

pub use action::KeyAction;
pub use key::{Key, KeyPress, KeySequence, Modifiers, SpecialKey};
pub use map::{KeyMapping, Keymap, LocalizedKeymap, MatchResult, keymap};

#[cfg(feature = "modal")]
pub use modal::{ModalKeymap, Mode, ModeController, modal_keymap};

use crate::{Editor, Result};

/// An editor that supports exclusive key-press capture.
///
/// Calling [`KeyEditor::capture_keys`] installs a permanent handler: every
/// subsequent key press is routed to the callback instead of the editor's
/// normal processing.  Calling it again **replaces** the previous handler.
pub trait KeyEditor: Editor {
    /// Register a permanent, exclusive key-press handler.
    ///
    /// While the handler is registered, all other editor key-processing is
    /// suspended.  Every key press invokes `callback` with the pressed key.
    /// Calling this method again replaces the previous handler.
    fn capture_keys<F>(&self, callback: F) -> Result<()>
    where
        F: Fn(&KeyPress) + Send + Sync + 'static;
}

#[cfg(feature = "tests")]
pub mod tests {
    use std::sync::{Arc, mpsc};

    use super::{KeyEditor, KeyPress, Keymap, LocalizedKeymap, keymap};
    use crate::keymap::key::{Key, Modifiers};
    use crate::keymap::map::KeyMapping;

    /// Extension of [`KeyEditor`] for use in generic tests.
    ///
    /// Implementors must send `key` through the editor's **real** input pipeline
    /// (not directly to the stored handler), so that tests exercise the full
    /// `capture_keys` interception path including any override behaviour.
    /// The method must block until the key has been fully processed.
    pub trait TestKeyEditor: KeyEditor {
        fn send_test_key(&self, key: &KeyPress);
    }

    fn kp(c: char) -> KeyPress {
        KeyPress::char(c)
    }

    fn collect(rx: &mpsc::Receiver<char>) -> Vec<char> {
        rx.try_iter().collect()
    }

    pub fn test_capture_keys_receives_press<E: TestKeyEditor>(editor: E) {
        let (tx, rx) = mpsc::channel::<char>();

        editor
            .capture_keys(move |key| {
                if let Key::Char(c) = key.key {
                    tx.send(c).unwrap();
                }
            })
            .unwrap();

        editor.send_test_key(&kp('a'));
        editor.send_test_key(&kp('b'));

        assert_eq!(collect(&rx), vec!['a', 'b']);
    }

    pub fn test_capture_keys_replaces_handler<E: TestKeyEditor>(editor: E) {
        let (tx, rx) = mpsc::channel::<char>();

        let tx1 = tx.clone();
        editor
            .capture_keys(move |_| {
                tx1.send('a').unwrap();
            })
            .unwrap();

        let tx2 = tx.clone();
        editor
            .capture_keys(move |_| {
                tx2.send('b').unwrap();
            })
            .unwrap();

        editor.send_test_key(&kp('x'));

        assert_eq!(collect(&rx), vec!['b']);
    }

    pub fn test_keymap_global_binding<E: TestKeyEditor>(editor: E)
    where
        E::BufferHandle: std::hash::Hash,
    {
        let editor = Arc::new(editor);
        let (tx, rx) = mpsc::channel::<char>();

        let km = LocalizedKeymap::new(
            editor.clone(),
            keymap! {
                "a" => { tx.send('x').unwrap(); Ok(()) },
            },
        );

        km.activate().unwrap();
        editor.send_test_key(&kp('a'));

        assert_eq!(collect(&rx), vec!['x']);
    }

    pub fn test_keymap_multi_key_sequence<E: TestKeyEditor>(editor: E)
    where
        E::BufferHandle: std::hash::Hash,
    {
        let editor = Arc::new(editor);
        let (tx, rx) = mpsc::channel::<char>();

        let km = LocalizedKeymap::new(
            editor.clone(),
            keymap! {
                "ab" => { tx.send('x').unwrap(); Ok(()) },
            },
        );

        km.activate().unwrap();

        editor.send_test_key(&kp('a'));
        assert_eq!(
            collect(&rx),
            vec![],
            "should not fire after prefix key only"
        );

        editor.send_test_key(&kp('b'));
        assert_eq!(collect(&rx), vec!['x']);
    }

    pub fn test_keymap_no_match_resets<E: TestKeyEditor>(editor: E)
    where
        E::BufferHandle: std::hash::Hash,
    {
        let editor = Arc::new(editor);
        let (tx, rx) = mpsc::channel::<char>();

        let km = LocalizedKeymap::new(
            editor.clone(),
            keymap! {
                "ab" => { tx.send('x').unwrap(); Ok(()) },
            },
        );

        km.activate().unwrap();

        // "ax" — no match, should reset accumulator
        editor.send_test_key(&kp('a'));
        editor.send_test_key(&kp('x'));
        assert_eq!(collect(&rx), vec![], "no match should not fire");

        // "ab" sent fresh should fire
        editor.send_test_key(&kp('a'));
        editor.send_test_key(&kp('b'));
        assert_eq!(collect(&rx), vec!['x']);
    }

    pub fn test_keymap_local_binding_priority<E: TestKeyEditor>(editor: E)
    where
        E::BufferHandle: std::hash::Hash,
    {
        let editor = Arc::new(editor);
        let (tx, rx) = mpsc::channel::<char>();

        let current_buf = editor.current_buffer().expect("no current buffer");

        let tx1 = tx.clone();
        let tx2 = tx.clone();

        let mut km = LocalizedKeymap::new(
            editor.clone(),
            keymap! {
                "a" => { tx1.send('g').unwrap(); Ok(()) },
            },
        );

        let local: KeyMapping<E> = keymap! {
            "a" => { tx2.send('l').unwrap(); Ok(()) },
        };
        km.set_local(current_buf, local);

        km.activate().unwrap();
        editor.send_test_key(&kp('a'));

        assert_eq!(collect(&rx), vec!['l']);
    }

    pub fn test_keymap_local_fallback_to_global<E: TestKeyEditor>(editor: E)
    where
        E::BufferHandle: std::hash::Hash,
    {
        let editor = Arc::new(editor);
        let (tx, rx) = mpsc::channel::<char>();

        let current_buf = editor.current_buffer().expect("no current buffer");
        let other_buf = editor.new_buffer().expect("failed to create buffer");

        let tx1 = tx.clone();
        let tx2 = tx.clone();

        let mut km = LocalizedKeymap::new(
            editor.clone(),
            keymap! {
                "a" => { tx1.send('g').unwrap(); Ok(()) },
            },
        );

        // Local binding is on other_buf — should not intercept keys while on current_buf
        let local: KeyMapping<E> = keymap! {
            "a" => { tx2.send('o').unwrap(); Ok(()) },
        };
        km.set_local(other_buf.clone(), local);

        editor.set_current_buffer(&current_buf).unwrap();
        km.activate().unwrap();
        editor.send_test_key(&kp('a'));

        assert_eq!(collect(&rx), vec!['g']);

        editor.kill_buffer(&other_buf).unwrap();
    }

    pub fn test_keypress_roundtrip<E: TestKeyEditor>(editor: E) {
        use crate::keymap::key::SpecialKey;

        let cases: Vec<KeyPress> = vec![
            // Lowercase letters
            KeyPress::char('a'),
            KeyPress::char('z'),
            // Uppercase letters (canonical: Key::Char(uppercase) + no modifiers)
            KeyPress::char('A'),
            KeyPress::char('Z'),
            // Digits
            KeyPress::char('0'),
            KeyPress::char('9'),
            // Common symbols
            KeyPress::char(' '),
            KeyPress::char('.'),
            KeyPress::char(','),
            KeyPress::char('/'),
            KeyPress::char(';'),
            KeyPress::char('\''),
            KeyPress::char('`'),
            KeyPress::char('['),
            KeyPress::char(']'),
            KeyPress::char('-'),
            KeyPress::char('='),
            // Shift-required symbol chars (plain Key::Char + no modifier in the model)
            KeyPress::char('!'),
            KeyPress::char('>'),
            KeyPress::char('?'),
            KeyPress::char('_'),
            KeyPress::char('+'),
            KeyPress::char('|'),
            KeyPress::char('~'),
            // Edge chars that require angle-bracket notation
            KeyPress::char('<'),
            KeyPress::char('\\'),
            // Ctrl+letter
            KeyPress::new(Key::Char('a'), Modifiers::ctrl()),
            KeyPress::new(Key::Char('z'), Modifiers::ctrl()),
            // Ctrl+digit
            KeyPress::new(Key::Char('0'), Modifiers::ctrl()),
            KeyPress::new(Key::Char('9'), Modifiers::ctrl()),
            // Ctrl + angle-bracket / backslash
            KeyPress::new(Key::Char('<'), Modifiers::ctrl()),
            KeyPress::new(Key::Char('\\'), Modifiers::ctrl()),
            // Ctrl+Shift+letter
            KeyPress::new(
                Key::Char('a'),
                Modifiers {
                    ctrl: true,
                    shift: true,
                },
            ),
            // Special keys (single raw bytes)
            KeyPress::special(SpecialKey::Enter),
            KeyPress::special(SpecialKey::Tab),
            KeyPress::special(SpecialKey::Escape),
            KeyPress::special(SpecialKey::Backspace),
            // Special keys (require feedkeys notation expansion)
            KeyPress::special(SpecialKey::Up),
            KeyPress::special(SpecialKey::Down),
            KeyPress::special(SpecialKey::Left),
            KeyPress::special(SpecialKey::Right),
            KeyPress::special(SpecialKey::Home),
            KeyPress::special(SpecialKey::End),
            KeyPress::special(SpecialKey::PageUp),
            KeyPress::special(SpecialKey::PageDown),
            KeyPress::special(SpecialKey::Delete),
            KeyPress::special(SpecialKey::Insert),
            // Ctrl + navigation keys
            KeyPress::new(Key::Special(SpecialKey::Up), Modifiers::ctrl()),
            KeyPress::new(Key::Special(SpecialKey::Down), Modifiers::ctrl()),
            KeyPress::new(Key::Special(SpecialKey::Left), Modifiers::ctrl()),
            KeyPress::new(Key::Special(SpecialKey::Right), Modifiers::ctrl()),
            KeyPress::new(Key::Special(SpecialKey::Enter), Modifiers::ctrl()),
            KeyPress::new(Key::Special(SpecialKey::Backspace), Modifiers::ctrl()),
            // Shift + navigation/special keys
            KeyPress::new(Key::Special(SpecialKey::Up), Modifiers::shift()),
            KeyPress::new(Key::Special(SpecialKey::Down), Modifiers::shift()),
            KeyPress::new(Key::Special(SpecialKey::Left), Modifiers::shift()),
            KeyPress::new(Key::Special(SpecialKey::Right), Modifiers::shift()),
            KeyPress::new(Key::Special(SpecialKey::Tab), Modifiers::shift()),
            KeyPress::new(Key::Special(SpecialKey::Enter), Modifiers::shift()),
            // Function keys
            KeyPress::special(SpecialKey::F(1)),
            KeyPress::special(SpecialKey::F(6)),
            KeyPress::special(SpecialKey::F(12)),
            // Modifier + function keys
            KeyPress::new(Key::Special(SpecialKey::F(1)), Modifiers::shift()),
            KeyPress::new(Key::Special(SpecialKey::F(12)), Modifiers::shift()),
            KeyPress::new(Key::Special(SpecialKey::F(1)), Modifiers::ctrl()),
        ];

        let (tx, rx) = mpsc::channel::<KeyPress>();

        editor
            .capture_keys(move |key| {
                tx.send(key.clone()).unwrap();
            })
            .unwrap();

        for key in &cases {
            editor.send_test_key(key);
        }

        let received: Vec<KeyPress> = rx.try_iter().collect();
        assert_eq!(received, cases);
    }

    pub fn test_keymap_remove_local<E: TestKeyEditor>(editor: E)
    where
        E::BufferHandle: std::hash::Hash,
    {
        let editor = Arc::new(editor);
        let (tx, rx) = mpsc::channel::<char>();

        let current_buf = editor.current_buffer().expect("no current buffer");

        let tx1 = tx.clone();
        let tx2 = tx.clone();

        let mut km = LocalizedKeymap::new(
            editor.clone(),
            keymap! {
                "a" => { tx1.send('g').unwrap(); Ok(()) },
            },
        );

        // Add then immediately remove a local binding.
        let local: KeyMapping<E> = keymap! {
            "a" => { tx2.send('l').unwrap(); Ok(()) },
        };
        km.set_local(current_buf.clone(), local);
        km.remove_local(&current_buf);

        // Activate: no local binding present, global should fire.
        editor.set_current_buffer(&current_buf).unwrap();
        km.activate().unwrap();
        editor.send_test_key(&kp('a'));

        assert_eq!(collect(&rx), vec!['g']);
    }

    pub fn test_keymap_global_mut<E: TestKeyEditor>(editor: E)
    where
        E::BufferHandle: std::hash::Hash,
    {
        let editor = Arc::new(editor);
        let (tx, rx) = mpsc::channel::<char>();

        let mut km = LocalizedKeymap::new(editor.clone(), KeyMapping::<E>::new());

        let tx1 = tx.clone();
        km.global_mut().add_binding(
            &[kp('z')],
            Arc::new(move |_: &E| {
                tx1.send('z').unwrap();
                Ok(())
            }),
        );

        km.activate().unwrap();
        editor.send_test_key(&kp('z'));

        assert_eq!(collect(&rx), vec!['z']);
    }

    /// Bind a [`KeyMapping`] directly via [`Keymap::activate_keymap`] (no
    /// [`LocalizedKeymap`] wrapper).
    pub fn test_keymap_direct_activate<E: TestKeyEditor>(editor: E) {
        let editor = Arc::new(editor);
        let (tx, rx) = mpsc::channel::<char>();

        let tx1 = tx.clone();
        let tx2 = tx.clone();

        let km: KeyMapping<E> = keymap! {
            "a"  => { tx1.send('a').unwrap(); Ok(()) },
            "bc" => { tx2.send('b').unwrap(); Ok(()) },
        };

        km.activate_keymap(editor.clone()).unwrap();

        editor.send_test_key(&kp('a'));
        assert_eq!(collect(&rx), vec!['a']);

        editor.send_test_key(&kp('b'));
        assert_eq!(collect(&rx), vec![], "partial match must not fire");

        editor.send_test_key(&kp('c'));
        assert_eq!(collect(&rx), vec!['b']);
    }

    /// An empty-sequence catch-all binding fires for any keypress that has no
    /// more-specific match, but not for partial matches.
    pub fn test_keymap_catchall<E: TestKeyEditor>(editor: E) {
        let editor = Arc::new(editor);
        let (tx, rx) = mpsc::channel::<char>();

        let tx1 = tx.clone();
        let tx2 = tx.clone();

        let km: KeyMapping<E> = keymap! {
            ""   => { tx1.send('c').unwrap(); Ok(()) }, // catch-all
            "ab" => { tx2.send('x').unwrap(); Ok(()) },
        };

        km.activate_keymap(editor.clone()).unwrap();

        // Unmatched single key → catch-all fires.
        editor.send_test_key(&kp('z'));
        assert_eq!(
            collect(&rx),
            vec!['c'],
            "unmatched key should fire catch-all"
        );

        // Partial match → catch-all must NOT fire (still accumulating).
        editor.send_test_key(&kp('a'));
        assert_eq!(
            collect(&rx),
            vec![],
            "partial match must not fire catch-all"
        );

        // Unmatched continuation ("az") → no specific binding → catch-all fires.
        editor.send_test_key(&kp('z'));
        assert_eq!(
            collect(&rx),
            vec!['c'],
            "unmatched continuation should fire catch-all"
        );

        // Specific binding wins — catch-all must be silent.
        editor.send_test_key(&kp('a'));
        editor.send_test_key(&kp('b'));
        assert_eq!(
            collect(&rx),
            vec!['x'],
            "specific binding must not be shadowed by catch-all"
        );
    }

    #[macro_export]
    macro_rules! eel_keyeditor_tests {
        ($test_tag:path, $editor_factory:expr, $prefix:tt) => {
            $crate::eel_tests!(
                test_tag: $test_tag,
                editor_factory: $editor_factory,
                editor_bounds: {
                    E: $crate::keymap::tests::TestKeyEditor,
                    E::BufferHandle: ::std::hash::Hash,
                },
                module_path: $crate::keymap::tests,
                prefix: $prefix,
                tests: [
                    test_capture_keys_receives_press,
                    test_capture_keys_replaces_handler,
                    test_keymap_global_binding,
                    test_keymap_multi_key_sequence,
                    test_keymap_no_match_resets,
                    test_keymap_local_binding_priority,
                    test_keymap_local_fallback_to_global,
                    test_keymap_remove_local,
                    test_keymap_global_mut,
                    test_keymap_direct_activate,
                    test_keymap_catchall,
                    test_keypress_roundtrip,
                ],
            );
        };

        ($test_tag:path, $editor_factory:expr) => {
            $crate::eel_keyeditor_tests!($test_tag, $editor_factory, "");
        };
    }
}

#[cfg(test)]
mod macro_tests {
    use eel_macros::{key, keys};

    use crate::keymap::KeyPress;
    use crate::keymap::key::{Key, Modifiers, SpecialKey};

    #[test]
    fn key_macro_char() {
        assert_eq!(key!("a"), KeyPress::char('a'));
        assert_eq!(key!("Z"), KeyPress::char('Z'));
        assert_eq!(key!("1"), KeyPress::char('1'));
    }

    #[test]
    fn key_macro_special() {
        assert_eq!(key!("<Enter>"), KeyPress::special(SpecialKey::Enter));
        assert_eq!(key!("<Escape>"), KeyPress::special(SpecialKey::Escape));
        assert_eq!(key!("<F5>"), KeyPress::special(SpecialKey::F(5)));
        assert_eq!(key!("<PageUp>"), KeyPress::special(SpecialKey::PageUp));
    }

    #[test]
    fn key_macro_modified() {
        assert_eq!(
            key!("<C-a>"),
            KeyPress::new(Key::Char('a'), Modifiers::ctrl())
        );
        assert_eq!(
            key!("<S-Up>"),
            KeyPress::new(Key::Special(SpecialKey::Up), Modifiers::shift())
        );
    }

    #[test]
    fn keys_macro_sequence() {
        assert_eq!(keys!("ab"), &[KeyPress::char('a'), KeyPress::char('b')]);
        assert_eq!(
            keys!("g<C-j>x"),
            &[
                KeyPress::char('g'),
                KeyPress::new(Key::Char('j'), Modifiers::ctrl()),
                KeyPress::char('x'),
            ]
        );
    }

    #[test]
    fn keys_macro_empty() {
        let seq: &[KeyPress] = keys!("");
        assert!(seq.is_empty());
    }

    #[test]
    fn keys_macro_single() {
        assert_eq!(keys!("<Enter>"), &[KeyPress::special(SpecialKey::Enter)]);
    }
}
