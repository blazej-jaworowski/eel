use std::{
    collections::HashMap,
    fmt,
    hash::Hash,
    sync::{Arc, Mutex},
};

use sequence_trie::SequenceTrie;

use crate::{Editor, Result, tracing::ResultExt as _};

use super::{KeyAction, KeyEditor, KeyPress, KeySequence};

/// Result of looking up a key sequence in a [`Keymap`].
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

/// Common interface for key-sequence lookup.
pub trait Keymap<E: Editor> {
    type Action: KeyAction<E> + Clone;
    fn match_sequence(&self, seq: &[KeyPress]) -> MatchResult<Self::Action>;
}

#[derive(Clone)]
struct KeyTrie<A: Clone> {
    inner: SequenceTrie<KeyPress, A>,
}

impl<A: Clone> Default for KeyTrie<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: Clone> KeyTrie<A> {
    fn new() -> Self {
        Self {
            inner: SequenceTrie::new(),
        }
    }

    fn add_binding(&mut self, seq: &[KeyPress], action: A) {
        self.inner.insert(seq, action);
    }

    fn remove_binding(&mut self, seq: &[KeyPress]) {
        self.inner.remove(seq);
    }

    fn match_sequence(&self, seq: &[KeyPress]) -> MatchResult<A> {
        match self.inner.get_node(seq) {
            None => MatchResult::NoMatch,
            Some(node) => match node.value() {
                Some(a) => MatchResult::ExactMatch(a.clone()),
                // SequenceTrie invariant: a node with no value always has children.
                None => MatchResult::PartialMatch,
            },
        }
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

/// A trie of key-sequence → action bindings.
pub struct KeyMapping<E: Editor, A: KeyAction<E> + Clone = Arc<dyn KeyAction<E>>> {
    inner: KeyTrie<A>,
    _marker: std::marker::PhantomData<E>,
}

impl<E: Editor, A: KeyAction<E> + Clone> Default for KeyMapping<E, A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Editor, A: KeyAction<E> + Clone> Clone for KeyMapping<E, A> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<E: Editor, A: KeyAction<E> + Clone> KeyMapping<E, A> {
    pub fn new() -> Self {
        Self {
            inner: KeyTrie::new(),
            _marker: std::marker::PhantomData,
        }
    }

    /// Register `action` for `seq`, replacing any existing binding.
    pub fn add_binding(&mut self, seq: &[KeyPress], action: A) {
        self.inner.add_binding(seq, action);
    }

    /// Remove the binding for `seq`.
    pub fn remove_binding(&mut self, seq: &[KeyPress]) {
        self.inner.remove_binding(seq);
    }

    /// Iterate over all bindings as `(sequence, action)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (Vec<&KeyPress>, &A)> {
        self.inner.inner.iter()
    }
}

impl<E: Editor> KeyMapping<E, Arc<dyn KeyAction<E>>> {
    /// Register a closure or function as an action for `seq`.
    ///
    /// This is an ergonomic wrapper around [`add_binding`] for the
    /// `Arc<dyn KeyAction<E>>` action type; it boxes the action automatically.
    pub fn bind(&mut self, seq: &[KeyPress], action: impl KeyAction<E> + 'static) {
        self.add_binding(seq, Arc::new(action));
    }
}

impl<E: Editor, A: KeyAction<E> + Clone> Keymap<E> for KeyMapping<E, A> {
    type Action = A;
    fn match_sequence(&self, seq: &[KeyPress]) -> MatchResult<A> {
        self.inner.match_sequence(seq)
    }
}

impl<E: Editor, A: KeyAction<E> + Clone> fmt::Debug for KeyMapping<E, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let seqs: Vec<Vec<KeyPress>> = self
            .iter()
            .map(|(k, _)| k.into_iter().cloned().collect())
            .collect();
        f.debug_struct("KeyMapping")
            .field("bindings", &seqs)
            .finish()
    }
}

/// Two [`KeyMapping`]s are equal if they have the **same set of bound key
/// sequences** with the **same actions**.
///
/// Actions are compared with `==`.  For the common case of testing that a
/// macro-generated and a manually-built keymap are structurally identical,
/// use a comparable action type such as [`crate::mock::MockAction`].
impl<E: Editor, A: KeyAction<E> + Clone + PartialEq> PartialEq for KeyMapping<E, A> {
    fn eq(&self, other: &Self) -> bool {
        let mut a: Vec<(Vec<KeyPress>, A)> = self
            .iter()
            .map(|(k, v)| (k.into_iter().cloned().collect(), v.clone()))
            .collect();
        let mut b: Vec<(Vec<KeyPress>, A)> = other
            .iter()
            .map(|(k, v)| (k.into_iter().cloned().collect(), v.clone()))
            .collect();
        a.sort_unstable_by(|(k1, _), (k2, _)| k1.cmp(k2));
        b.sort_unstable_by(|(k1, _), (k2, _)| k1.cmp(k2));
        a == b
    }
}

impl<E: Editor, A: KeyAction<E> + Clone + Eq> Eq for KeyMapping<E, A> {}

/// Build a [`KeyMapping`] from a list of `"key-sequence" => action` pairs.
///
/// Key sequences are parsed by [`eel::keymap::key::parse_key_sequence`], so
/// they support plain characters and angle-bracket notation (`<C-j>`,
/// `<Enter>`, `<F1>`, etc.).
///
/// Each action is passed directly to [`KeyMapping::add_binding`], so the
/// resulting action type `A` is inferred from the expressions.  Use
/// `Arc::new(...)` to produce `Arc<dyn KeyAction<E>>`, or a concrete
/// action type such as `MockAction` for tests.
///
/// ```ignore
/// // With a concrete action type:
/// let km: KeyMapping<MyEditor, MyAction> = keymap! {
///     "j"     => MyAction::MoveDown,
///     "<C-j>" => MyAction::Ctrl,
/// };
///
/// // With Arc-boxed closures:
/// let km: KeyMapping<MyEditor> = keymap! {
///     "j"     => Arc::new(|_: &MyEditor| Ok(())),
///     "<C-j>" => Arc::new(|_: &MyEditor| Ok(())),
/// };
/// ```
#[macro_export]
macro_rules! keymap {
    ( $( $seq:expr => $action:expr ),* $(,)? ) => {{
        let mut _m = $crate::keymap::KeyMapping::new();
        $(
            _m.add_binding(
                &$crate::keymap::key::parse_key_sequence($seq)
                    .expect("invalid key sequence in keymap! macro"),
                $action,
            );
        )*
        _m
    }};
}

/// A keymap that dispatches to a per-buffer local inner keymap first, then
/// falls back to the global inner keymap `K`.
///
/// Construct with [`LocalizedKeymap::new`], passing an `Arc<E>` so that
/// [`Keymap::match_sequence`] can resolve the current buffer internally.
pub struct LocalizedKeymap<E: Editor, K> {
    editor: Arc<E>,
    global: K,
    local: HashMap<E::BufferHandle, K>,
}

impl<E, K> Clone for LocalizedKeymap<E, K>
where
    E: Editor,
    K: Clone,
    E::BufferHandle: Hash,
{
    fn clone(&self) -> Self {
        Self {
            editor: self.editor.clone(),
            global: self.global.clone(),
            local: self.local.clone(),
        }
    }
}

impl<E: Editor, K> LocalizedKeymap<E, K> {
    pub fn new(editor: Arc<E>, global: K) -> Self {
        Self {
            editor,
            global,
            local: HashMap::new(),
        }
    }

    /// Returns a shared reference to the global inner keymap.
    pub fn global(&self) -> &K {
        &self.global
    }

    /// Returns a mutable reference to the global inner keymap.
    pub fn global_mut(&mut self) -> &mut K {
        &mut self.global
    }
}

impl<E, K> LocalizedKeymap<E, K>
where
    E: Editor,
    E::BufferHandle: Hash,
{
    /// Directly set the local keymap for `buffer`.
    pub fn set_local(&mut self, buffer: E::BufferHandle, keymap: K) {
        self.local.insert(buffer, keymap);
    }

    /// Remove the local keymap for `buffer`.
    pub fn remove_local(&mut self, buffer: &E::BufferHandle) {
        self.local.remove(buffer);
    }

    /// Returns a mutable reference to the local keymap for `buffer`, or
    /// `None` if no local keymap has been set for it.  To create one, use
    /// [`LocalizedKeymap::set_local`].
    pub fn local_for(&mut self, buffer: &E::BufferHandle) -> Option<&mut K> {
        self.local.get_mut(buffer)
    }
}

impl<E, K> Keymap<E> for LocalizedKeymap<E, K>
where
    E: Editor,
    K: Keymap<E>,
    E::BufferHandle: Hash,
{
    type Action = K::Action;

    fn match_sequence(&self, seq: &[KeyPress]) -> MatchResult<K::Action> {
        let buf = self
            .editor
            .current_buffer()
            .log_err_msg("LocalizedKeymap: failed to get current buffer")
            .ok();
        let local = buf.as_ref().and_then(|b| self.local.get(b));
        match local
            .map(|km| km.match_sequence(seq))
            .unwrap_or(MatchResult::NoMatch)
        {
            MatchResult::NoMatch => self.global.match_sequence(seq),
            other => other,
        }
    }
}

impl<E, K> LocalizedKeymap<E, K>
where
    E: KeyEditor + 'static,
    K: Keymap<E> + Clone + Send + Sync + 'static,
    E::BufferHandle: Hash,
{
    /// Activate this keymap on the editor supplied at construction time.
    ///
    /// Calls [`KeyEditor::capture_keys`] to register a permanent handler.
    /// Key-press accumulation and binding lookup:
    /// - **Exact match** (local takes priority over global): action is called,
    ///   accumulator is reset.
    /// - **Partial match only**: accumulator grows, waiting for the next key.
    /// - **No match**: accumulator is silently reset.
    pub fn activate(self) -> Result<()> {
        let editor = self.editor.clone();
        let keymap = Arc::new(self);
        let current_seq: Arc<Mutex<KeySequence>> = Arc::new(Mutex::new(Vec::new()));

        editor.capture_keys(move |key_press| {
            let mut seq = current_seq.lock().unwrap();
            seq.push(key_press.clone());

            let result = keymap.match_sequence(&seq);

            match result {
                MatchResult::ExactMatch(action) => {
                    seq.clear();
                    drop(seq);
                    _ = action
                        .call(&keymap.editor)
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

    fn exact(b: &KeyTrie<i32>, chars: &str) -> Option<i32> {
        match b.match_sequence(&seq(chars)) {
            MatchResult::ExactMatch(v) => Some(v),
            _ => None,
        }
    }

    fn is_partial(b: &KeyTrie<i32>, chars: &str) -> bool {
        matches!(b.match_sequence(&seq(chars)), MatchResult::PartialMatch)
    }

    fn is_no_match(b: &KeyTrie<i32>, chars: &str) -> bool {
        matches!(b.match_sequence(&seq(chars)), MatchResult::NoMatch)
    }

    #[test]
    fn exact_match_single_key() {
        let mut b = KeyTrie::new();
        b.add_binding(&seq("a"), 1);
        assert_eq!(exact(&b, "a"), Some(1));
    }

    #[test]
    fn exact_match_multi_key() {
        let mut b = KeyTrie::new();
        b.add_binding(&seq("abc"), 42);
        assert_eq!(exact(&b, "abc"), Some(42));
    }

    #[test]
    fn partial_match_prefix() {
        let mut b = KeyTrie::new();
        b.add_binding(&seq("abc"), 1);
        assert!(is_partial(&b, "a"));
        assert!(is_partial(&b, "ab"));
    }

    #[test]
    fn no_match_absent() {
        let mut b = KeyTrie::new();
        b.add_binding(&seq("abc"), 1);
        assert!(is_no_match(&b, "x"));
        assert!(is_no_match(&b, "abx"));
        assert!(is_no_match(&b, "abcd"));
    }

    #[test]
    fn exact_match_wins_over_partial() {
        // "ab" is both an exact binding AND a prefix of "abc".
        let mut b = KeyTrie::new();
        b.add_binding(&seq("ab"), 10);
        b.add_binding(&seq("abc"), 20);
        assert_eq!(exact(&b, "ab"), Some(10));
        assert_eq!(exact(&b, "abc"), Some(20));
        assert!(is_partial(&b, "a"));
    }

    #[test]
    fn multiple_bindings_independent() {
        let mut b = KeyTrie::new();
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
        let mut b = KeyTrie::new();
        b.add_binding(&seq("a"), 1);
        b.add_binding(&seq("a"), 99);
        assert_eq!(exact(&b, "a"), Some(99));
    }

    #[test]
    fn remove_terminal_binding() {
        let mut b = KeyTrie::new();
        b.add_binding(&seq("a"), 1);
        b.remove_binding(&seq("a"));
        assert!(is_no_match(&b, "a"));
    }

    #[test]
    fn remove_preserves_sibling() {
        let mut b = KeyTrie::new();
        b.add_binding(&seq("a"), 1);
        b.add_binding(&seq("b"), 2);
        b.remove_binding(&seq("a"));
        assert!(is_no_match(&b, "a"));
        assert_eq!(exact(&b, "b"), Some(2));
    }

    #[test]
    fn remove_prunes_dead_interior_nodes() {
        let mut b = KeyTrie::new();
        b.add_binding(&seq("abc"), 1);
        b.remove_binding(&seq("abc"));
        // The interior nodes for 'a' and 'b' should be gone — verified via match.
        assert!(is_no_match(&b, "a"));
        assert!(is_no_match(&b, "ab"));
        assert!(b.is_empty());
    }

    #[test]
    fn remove_prefix_keeps_longer_binding() {
        let mut b = KeyTrie::new();
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

#[cfg(test)]
mod keymap_macro_tests {
    use super::*;
    use crate::keymap::action::KeyAction;
    use crate::keymap::key::parse_key_sequence;
    use crate::mock::{MockAction, MockEditor};

    fn km_with_actions(bindings: &[(&str, u32)]) -> KeyMapping<MockEditor, MockAction> {
        let mut km = KeyMapping::new();
        for (s, tag) in bindings {
            km.add_binding(&parse_key_sequence(s).unwrap(), MockAction(*tag));
        }
        km
    }

    #[test]
    fn macro_single_key() {
        let via_macro = keymap! { "j" => MockAction(0) };
        assert_eq!(via_macro, km_with_actions(&[("j", 0)]));
    }

    #[test]
    fn macro_multi_key_sequence() {
        let via_macro = keymap! { "abc" => MockAction(0) };
        assert_eq!(via_macro, km_with_actions(&[("abc", 0)]));
    }

    #[test]
    fn macro_special_key_notation() {
        let via_macro = keymap! {
            "<C-j>"   => MockAction(0),
            "<Enter>" => MockAction(1),
            "<S-Up>"  => MockAction(2),
        };
        assert_eq!(
            via_macro,
            km_with_actions(&[("<C-j>", 0), ("<Enter>", 1), ("<S-Up>", 2)])
        );
        assert_ne!(
            via_macro,
            km_with_actions(&[("<C-j>", 99), ("<Enter>", 1), ("<S-Up>", 2)])
        );
    }

    #[test]
    fn macro_multiple_bindings() {
        let via_macro = keymap! {
            "j"  => MockAction(0),
            "k"  => MockAction(1),
            "gg" => MockAction(2),
        };
        assert_eq!(via_macro, km_with_actions(&[("j", 0), ("k", 1), ("gg", 2)]));
        assert_ne!(
            via_macro,
            km_with_actions(&[("j", 0), ("k", 1), ("gg", 99)])
        );
    }

    #[test]
    fn macro_empty() {
        let via_macro: KeyMapping<MockEditor, MockAction> = keymap! {};
        assert_eq!(via_macro, km_with_actions(&[]));
    }

    #[test]
    fn keymapping_equality_same_tags() {
        let a = km_with_actions(&[("j", 0), ("k", 1)]);
        let b = km_with_actions(&[("j", 0), ("k", 1)]);
        assert_eq!(a, b);
    }

    #[test]
    fn keymapping_equality_different_tags() {
        // Same sequence, different action tag → not equal.
        let a = km_with_actions(&[("j", 0)]);
        let b = km_with_actions(&[("j", 99)]);
        assert_ne!(a, b);
    }

    #[test]
    fn keymapping_equality_different_sequences() {
        let a = km_with_actions(&[("j", 0)]);
        let b = km_with_actions(&[("k", 0)]);
        assert_ne!(a, b);
    }
}
