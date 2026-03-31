use std::{
    collections::HashMap,
    hash::Hash,
    sync::{Arc, Mutex},
};

use sequence_trie::SequenceTrie;

use crate::{Editor, Result, tracing::ResultExt as _};

use super::{KeyAction, KeyEditor, KeyPress, KeySequence};

/// Result of looking up a key sequence in [`KeyMapping`].
pub enum MatchResult<A> {
    /// The sequence is not a prefix of any registered binding.
    NoMatch,
    /// The sequence is a strict prefix of at least one registered binding,
    /// but does not itself have an action.
    PartialMatch,
    /// The sequence exactly matches a registered binding.
    /// Contains a clone of the stored action.
    ExactMatch(A),
}

/// A trie of key-sequence → action bindings.
#[derive(Clone)]
pub struct KeyMapping<A: Clone> {
    inner: SequenceTrie<KeyPress, A>,
}

impl<A: Clone> Default for KeyMapping<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: Clone> KeyMapping<A> {
    pub fn new() -> Self {
        Self {
            inner: SequenceTrie::new(),
        }
    }

    /// Register `action` for `seq`, replacing any existing binding.
    pub fn add_binding(&mut self, seq: &[KeyPress], action: A) {
        self.inner.insert(seq, action);
    }

    /// Remove the binding for `seq`.
    pub fn remove_binding(&mut self, seq: &[KeyPress]) {
        self.inner.remove(seq);
    }

    /// Match `seq` against the trie.
    ///
    /// - [`MatchResult::ExactMatch`] — `seq` has a registered action.
    ///   Returned even when the node also has children.
    /// - [`MatchResult::PartialMatch`] — `seq` is a strict prefix of at least
    ///   one longer binding.
    /// - [`MatchResult::NoMatch`] — `seq` is not a prefix of any binding.
    pub fn match_sequence(&self, seq: &[KeyPress]) -> MatchResult<A> {
        match self.inner.get_node(seq) {
            None => MatchResult::NoMatch,
            Some(node) => match node.value() {
                Some(a) => MatchResult::ExactMatch(a.clone()),
                // SequenceTrie invariant: a node with no value always has children.
                None => MatchResult::PartialMatch,
            },
        }
    }
}

pub type KeyBindings<E> = KeyMapping<Arc<dyn KeyAction<E>>>;

/// A keymap with global bindings and optional per-buffer local bindings.
///
/// Activate via [`Keymap::activate`], which routes all captured key presses
/// through the registered bindings using prefix-trie matching.
pub struct Keymap<E: Editor> {
    global: KeyBindings<E>,
    local: HashMap<E::BufferHandle, KeyBindings<E>>,
}

impl<E: Editor> Clone for Keymap<E> {
    fn clone(&self) -> Self {
        Self {
            global: self.global.clone(),
            local: self.local.clone(),
        }
    }
}

impl<E: Editor> Default for Keymap<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Editor> Keymap<E> {
    pub fn new() -> Self {
        Self {
            global: KeyMapping::new(),
            local: HashMap::new(),
        }
    }

    pub fn add_global(&mut self, seq: &[KeyPress], action: impl KeyAction<E>) {
        self.global.add_binding(seq, Arc::new(action));
    }

    pub fn remove_global(&mut self, seq: &[KeyPress]) {
        self.global.remove_binding(seq);
    }

    pub fn add_local(
        &mut self,
        buffer: E::BufferHandle,
        seq: &[KeyPress],
        action: impl KeyAction<E>,
    ) where
        E::BufferHandle: Hash,
    {
        self.local
            .entry(buffer)
            .or_default()
            .add_binding(seq, Arc::new(action));
    }

    pub fn remove_local(&mut self, buffer: &E::BufferHandle, seq: &[KeyPress])
    where
        E::BufferHandle: Hash,
    {
        if let Some(bindings) = self.local.get_mut(buffer) {
            bindings.remove_binding(seq);
        }
    }
}

impl<E: KeyEditor + 'static> Keymap<E> {
    /// Activate this keymap on `editor`.
    ///
    /// Calls [`KeyEditor::capture_keys`] to register a permanent handler.
    /// Key-press accumulation and binding lookup use [`KeyMapping::match_sequence`]:
    /// - **Exact match** (local takes priority over global): action is called,
    ///   accumulator is reset.
    /// - **Partial match only**: accumulator grows, waiting for the next key.
    /// - **No match**: accumulator is silently reset.
    pub fn activate(&self, editor: Arc<E>) -> Result<()>
    where
        E::BufferHandle: Hash,
    {
        let keymap = Arc::new(self.clone());
        let editor_for_cb = Arc::clone(&editor);
        let current_seq: Arc<Mutex<KeySequence>> = Arc::new(Mutex::new(Vec::new()));

        editor.capture_keys(move |key_press| {
            let mut seq = current_seq.lock().unwrap();
            seq.push(key_press.clone());

            let current_buf = editor_for_cb.current_buffer().ok();
            let local = current_buf
                .as_ref()
                .and_then(|buf| keymap.local.get(buf))
                .map(|bind| bind.match_sequence(&seq))
                .unwrap_or(MatchResult::NoMatch);

            // Local bindings take priority; fall back to global.
            let result = match local {
                MatchResult::NoMatch => keymap.global.match_sequence(&seq),
                other => other,
            };

            match result {
                MatchResult::ExactMatch(action) => {
                    seq.clear();
                    drop(seq);
                    _ = action
                        .call(&editor_for_cb)
                        .log_err_msg("Keymap action failed");
                }
                MatchResult::PartialMatch => { /* keep accumulating */ }
                MatchResult::NoMatch => seq.clear(),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kp(c: char) -> KeyPress {
        KeyPress::char(c)
    }

    fn seq(chars: &str) -> Vec<KeyPress> {
        chars.chars().map(kp).collect()
    }

    fn exact(b: &KeyMapping<i32>, chars: &str) -> Option<i32> {
        match b.match_sequence(&seq(chars)) {
            MatchResult::ExactMatch(v) => Some(v),
            _ => None,
        }
    }

    fn is_partial(b: &KeyMapping<i32>, chars: &str) -> bool {
        matches!(b.match_sequence(&seq(chars)), MatchResult::PartialMatch)
    }

    fn is_no_match(b: &KeyMapping<i32>, chars: &str) -> bool {
        matches!(b.match_sequence(&seq(chars)), MatchResult::NoMatch)
    }

    #[test]
    fn exact_match_single_key() {
        let mut b = KeyMapping::new();
        b.add_binding(&seq("a"), 1);
        assert_eq!(exact(&b, "a"), Some(1));
    }

    #[test]
    fn exact_match_multi_key() {
        let mut b = KeyMapping::new();
        b.add_binding(&seq("abc"), 42);
        assert_eq!(exact(&b, "abc"), Some(42));
    }

    #[test]
    fn partial_match_prefix() {
        let mut b = KeyMapping::new();
        b.add_binding(&seq("abc"), 1);
        assert!(is_partial(&b, "a"));
        assert!(is_partial(&b, "ab"));
    }

    #[test]
    fn no_match_absent() {
        let mut b = KeyMapping::new();
        b.add_binding(&seq("abc"), 1);
        assert!(is_no_match(&b, "x"));
        assert!(is_no_match(&b, "abx"));
        assert!(is_no_match(&b, "abcd"));
    }

    #[test]
    fn exact_match_wins_over_partial() {
        // "ab" is both an exact binding AND a prefix of "abc".
        let mut b = KeyMapping::new();
        b.add_binding(&seq("ab"), 10);
        b.add_binding(&seq("abc"), 20);
        assert_eq!(exact(&b, "ab"), Some(10));
        assert_eq!(exact(&b, "abc"), Some(20));
        assert!(is_partial(&b, "a"));
    }

    #[test]
    fn multiple_bindings_independent() {
        let mut b = KeyMapping::new();
        b.add_binding(&seq("a"), 1);
        b.add_binding(&seq("b"), 2);
        b.add_binding(&seq("cd"), 3);
        assert_eq!(exact(&b, "a"), Some(1));
        assert_eq!(exact(&b, "b"), Some(2));
        assert_eq!(exact(&b, "cd"), Some(3));
        assert!(is_partial(&b, "c"));
        assert!(is_no_match(&b, "d"));
    }

    #[test]
    fn add_binding_replaces_existing() {
        let mut b = KeyMapping::new();
        b.add_binding(&seq("a"), 1);
        b.add_binding(&seq("a"), 99);
        assert_eq!(exact(&b, "a"), Some(99));
    }

    #[test]
    fn remove_terminal_binding() {
        let mut b = KeyMapping::new();
        b.add_binding(&seq("a"), 1);
        b.remove_binding(&seq("a"));
        assert!(is_no_match(&b, "a"));
    }

    #[test]
    fn remove_preserves_sibling() {
        let mut b = KeyMapping::new();
        b.add_binding(&seq("a"), 1);
        b.add_binding(&seq("b"), 2);
        b.remove_binding(&seq("a"));
        assert!(is_no_match(&b, "a"));
        assert_eq!(exact(&b, "b"), Some(2));
    }

    #[test]
    fn remove_prunes_dead_interior_nodes() {
        let mut b = KeyMapping::new();
        b.add_binding(&seq("abc"), 1);
        b.remove_binding(&seq("abc"));
        // The interior nodes for 'a' and 'b' should be gone — verified via match.
        assert!(is_no_match(&b, "a"));
        assert!(is_no_match(&b, "ab"));
        assert!(b.inner.is_empty());
    }

    #[test]
    fn remove_prefix_keeps_longer_binding() {
        let mut b = KeyMapping::new();
        b.add_binding(&seq("ab"), 1);
        b.add_binding(&seq("abc"), 2);
        // Remove the shorter binding; the longer one must still work.
        b.remove_binding(&seq("ab"));
        // "ab" no longer has an action, but it IS still a prefix of "abc" → PartialMatch.
        assert!(is_partial(&b, "ab"));
        assert!(is_partial(&b, "a"));
        assert_eq!(exact(&b, "abc"), Some(2));
    }
}
