pub mod action;
pub mod key;
pub mod map;

pub use action::KeyAction;
pub use key::{Key, KeyPress, KeySequence, Modifiers, SpecialKey};
pub use map::{KeyMapping, Keymap, MatchResult};

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

    use super::{KeyEditor, KeyPress, Keymap};
    use crate::keymap::key::{Key, Modifiers};

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

        let mut keymap: Keymap<E> = Keymap::new();
        keymap.add_global(&[kp('a')], move |_: &E| {
            tx.send('x').unwrap();
            Ok(())
        });

        keymap.activate(Arc::clone(&editor)).unwrap();
        editor.send_test_key(&kp('a'));

        assert_eq!(collect(&rx), vec!['x']);
    }

    pub fn test_keymap_multi_key_sequence<E: TestKeyEditor>(editor: E)
    where
        E::BufferHandle: std::hash::Hash,
    {
        let editor = Arc::new(editor);
        let (tx, rx) = mpsc::channel::<char>();

        let mut keymap: Keymap<E> = Keymap::new();
        keymap.add_global(&[kp('a'), kp('b')], move |_: &E| {
            tx.send('x').unwrap();
            Ok(())
        });

        keymap.activate(Arc::clone(&editor)).unwrap();

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

        let mut keymap: Keymap<E> = Keymap::new();
        keymap.add_global(&[kp('a'), kp('b')], move |_: &E| {
            tx.send('x').unwrap();
            Ok(())
        });

        keymap.activate(Arc::clone(&editor)).unwrap();

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

        let mut keymap: Keymap<E> = Keymap::new();
        keymap.add_global(&[kp('a')], move |_: &E| {
            tx1.send('g').unwrap();
            Ok(())
        });
        keymap.add_local(current_buf, &[kp('a')], move |_: &E| {
            tx2.send('l').unwrap();
            Ok(())
        });

        keymap.activate(Arc::clone(&editor)).unwrap();
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

        let mut keymap: Keymap<E> = Keymap::new();
        keymap.add_global(&[kp('a')], move |_: &E| {
            tx1.send('g').unwrap();
            Ok(())
        });
        // Local binding is on other_buf — should not intercept keys while on current_buf
        keymap.add_local(other_buf.clone(), &[kp('a')], move |_: &E| {
            tx2.send('o').unwrap();
            Ok(())
        });

        editor.set_current_buffer(&current_buf).unwrap();
        keymap.activate(Arc::clone(&editor)).unwrap();
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
            // Shift+letter (normalized: Key::Char(lowercase) + shift)
            KeyPress::char('A'), // = shift+a
            KeyPress::char('Z'), // = shift+z
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
            // Edge chars that require angle-bracket notation
            KeyPress::char('<'),
            KeyPress::char('\\'),
            // Ctrl+letter (raw control bytes 0x01–0x1A)
            KeyPress::new(Key::Char('a'), Modifiers::ctrl()),
            KeyPress::new(Key::Char('z'), Modifiers::ctrl()),
            // Meta (Alt) combos
            KeyPress::new(Key::Char('a'), Modifiers::meta()),
            KeyPress::new(Key::Char('z'), Modifiers::meta()),
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
            KeyPress::special(SpecialKey::F(1)),
            KeyPress::special(SpecialKey::F(12)),
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
                    test_keypress_roundtrip,
                ],
            );
        };

        ($test_tag:path, $editor_factory:expr) => {
            $crate::eel_keyeditor_tests!($test_tag, $editor_factory, "");
        };
    }
}
