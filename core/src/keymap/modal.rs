use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex},
};

use crate::Editor;

use super::{
    KeyAction, KeyPress,
    map::{KeyMapping, Keymap, MatchResult},
};

/// Marker trait for types that can serve as a [`ModalKeymap`] mode discriminant.
///
/// Implement this trait explicitly for each type you intend to use as a mode.
/// The supertrait bounds (`Eq + Hash + Clone + Send + Sync + 'static`) are
/// enforced at the trait level, so any impl that does not satisfy them will
/// fail to compile.
///
/// ```ignore
/// #[derive(Clone, Eq, PartialEq, Hash)]
/// enum MyMode { Normal, Insert }
/// impl Mode for MyMode {}
/// ```
pub trait Mode: Eq + std::hash::Hash + Clone + Send + Sync + 'static {}

/// Runtime state shared between a [`ModalKeymap`] and all [`ModeController`]
/// clones that were derived from it.
pub struct ModalState<M: Mode> {
    pub(crate) mode: M,
    /// Set to `true` when the first key of a new sequence arrives.
    /// Cleared by [`ModeController::set_mode`] on a mode change, so that a
    /// partial sequence accumulated in one mode can never fire in another.
    pub(crate) accumulating: bool,
}

/// A handle for reading and changing the active mode of a [`ModalKeymap`].
///
/// Obtained via [`ModalKeymap::mode_controller`].  Cheap to clone — all clones
/// share the same underlying state.  Safe to capture in key-action closures.
#[derive(Clone)]
pub struct ModeController<M: Mode> {
    state: Arc<Mutex<ModalState<M>>>,
}

impl<M: Mode> ModeController<M> {
    /// Returns the currently active mode.
    pub fn current_mode(&self) -> M {
        self.state.lock().unwrap().mode.clone()
    }

    /// Switch to `mode`.
    ///
    /// If `mode` differs from the current mode, also clears the `accumulating`
    /// flag so that a partial prefix started in the old mode can never complete
    /// in the new one.  Calling with the current mode is a no-op.
    pub fn set_mode(&self, mode: M) {
        let mut s = self.state.lock().unwrap();
        if s.mode != mode {
            s.accumulating = false;
        }
        s.mode = mode;
    }
}

/// A keymap that dispatches key presses according to the current mode.
///
/// Each mode has its own [`KeyMapping`] accessible via
/// [`ModalKeymap::keymap_for_mode`].
///
/// Mode transitions are performed through a [`ModeController`] obtained via
/// [`ModalKeymap::mode_controller`].  Calling [`ModeController::set_mode`]
/// atomically updates the mode and clears the `accumulating` flag, so stale
/// partial sequences can never fire across mode boundaries.
///
/// Unmatched key presses in any mode are silently dropped.
///
/// To add buffer-local bindings or to activate the keymap, wrap this in a
/// [`super::map::LocalizedKeymap`].
pub struct ModalKeymap<E: Editor, M: Mode, A: KeyAction<E> + Clone = Arc<dyn KeyAction<E>>> {
    pub(crate) state: Arc<Mutex<ModalState<M>>>,
    bindings: HashMap<M, KeyMapping<E, A>>,
}

impl<E, M, A> ModalKeymap<E, M, A>
where
    E: Editor,
    M: Mode,
    A: KeyAction<E> + Clone,
{
    pub fn new(initial: M) -> Self {
        Self {
            state: Arc::new(Mutex::new(ModalState {
                mode: initial,
                accumulating: false,
            })),
            bindings: HashMap::new(),
        }
    }

    /// Create a [`ModalKeymap`] that shares its mode state with an existing
    /// one.  This is useful for buffer-local modal keymaps that must track the
    /// same mode as the global keymap.
    pub fn with_shared_state(state: Arc<Mutex<ModalState<M>>>) -> Self {
        Self {
            state,
            bindings: HashMap::new(),
        }
    }

    /// Returns the [`Arc`] backing the shared mode state, so it can be passed
    /// to [`ModalKeymap::with_shared_state`].
    pub fn shared_state(&self) -> Arc<Mutex<ModalState<M>>> {
        self.state.clone()
    }

    /// Returns a [`ModeController`] that can be cloned and captured in actions
    /// to read or change the active mode.
    pub fn mode_controller(&self) -> ModeController<M> {
        ModeController {
            state: self.state.clone(),
        }
    }

    /// Returns a mutable reference to the [`KeyMapping`] for `mode`, creating
    /// an empty one if it does not yet exist.
    ///
    /// Use this to add or remove bindings for a specific mode:
    ///
    /// ```ignore
    /// km.keymap_for_mode(Mode::Normal).add_binding(&[kp('i')], action);
    /// ```
    pub fn keymap_for_mode(&mut self, mode: M) -> &mut KeyMapping<E, A> {
        self.bindings.entry(mode).or_default()
    }
}

impl<E, M, A> Clone for ModalKeymap<E, M, A>
where
    E: Editor,
    M: Mode,
    A: KeyAction<E> + Clone,
{
    /// Creates an independent clone of this keymap.
    ///
    /// The clone has:
    /// - the same bindings (deep-copied),
    /// - a fresh `accumulating` flag (false),
    /// - a snapshotted copy of the current mode.
    ///
    /// The original and the clone have **independent** runtime state: a
    /// [`ModeController`] obtained from one will not affect the other.
    fn clone(&self) -> Self {
        Self {
            bindings: self.bindings.clone(),
            state: Arc::new(Mutex::new(ModalState {
                mode: self.state.lock().unwrap().mode.clone(),
                accumulating: false,
            })),
        }
    }
}

impl<E, M, A> Keymap<E> for ModalKeymap<E, M, A>
where
    E: Editor,
    M: Mode,
    A: KeyAction<E> + Clone,
{
    type Action = A;

    /// Match `seq` against the bindings for the current mode.
    ///
    /// On the first key of a new sequence (`seq.len() == 1`), sets the
    /// `accumulating` flag.  If the flag was cleared by a mode change before
    /// a subsequent key arrives (`seq.len() > 1 && !accumulating`), returns
    /// [`MatchResult::NoMatch`] so the external accumulator resets cleanly.
    fn match_sequence(&self, seq: &[KeyPress]) -> MatchResult<A> {
        let mut s = self.state.lock().unwrap();
        if seq.len() == 1 {
            s.accumulating = true;
        } else if !s.accumulating {
            return MatchResult::NoMatch;
        }
        let mode = s.mode.clone();
        drop(s);

        match self.bindings.get(&mode) {
            Some(km) => km.match_sequence(seq),
            None => MatchResult::NoMatch,
        }
    }
}

impl<E, M, A> fmt::Debug for ModalKeymap<E, M, A>
where
    E: Editor,
    M: Mode + fmt::Debug,
    A: KeyAction<E> + Clone,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mode = self.state.lock().unwrap().mode.clone();
        f.debug_struct("ModalKeymap")
            .field("current_mode", &mode)
            .field("bindings", &self.bindings)
            .finish()
    }
}

/// Two [`ModalKeymap`]s are equal if they have the same current mode and the
/// same bindings (key sequences **and** actions) for every mode.
impl<E, M, A> PartialEq for ModalKeymap<E, M, A>
where
    E: Editor,
    M: Mode,
    A: KeyAction<E> + Clone + PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        let a_mode = self.state.lock().unwrap().mode.clone();
        let b_mode = other.state.lock().unwrap().mode.clone();
        a_mode == b_mode && self.bindings == other.bindings
    }
}

impl<E, M, A> Eq for ModalKeymap<E, M, A>
where
    E: Editor,
    M: Mode,
    A: KeyAction<E> + Clone + PartialEq + Eq,
{
}

/// Build a [`ModalKeymap`] from a list of `[Mode, …]: { bindings }` groups.
///
/// Multiple modes may share the same binding block — the block is expanded
/// once and the resulting [`KeyMapping`] is cloned into each mode slot.
///
/// The `editor:` and `controller:` headers enable bare block actions.
/// `controller: <name>` binds `ModalKeymap::mode_controller()` to `<name>`
/// inside every action block (it is cloned into each closure automatically).
///
/// ```ignore
/// let km = modal_keymap! {
///     editor: e,
///     controller: ctrl,
///     initial: MyMode::Normal,
///     [MyMode::Normal, MyMode::Visual]: {
///         "j" => { e.move_down(); Ok(()) },
///         "i" => { ctrl.set_mode(MyMode::Insert); Ok(()) },
///     },
///     [MyMode::Insert]: {
///         "<Escape>" => { ctrl.set_mode(MyMode::Normal); Ok(()) },
///     },
/// };
/// ```
pub use eel_macros::modal_keymap;

#[cfg(feature = "tests")]
pub mod tests {
    use std::sync::{Arc, mpsc};

    use super::{ModalKeymap, modal_keymap};
    use crate::keymap::KeyPress;
    use crate::keymap::map::LocalizedKeymap;
    use crate::keymap::tests::TestKeyEditor;

    #[derive(Debug, Clone, Eq, PartialEq, Hash)]
    enum TestMode {
        A,
        B,
    }

    impl super::super::Mode for TestMode {}

    fn kp(c: char) -> KeyPress {
        KeyPress::char(c)
    }

    fn collect(rx: &mpsc::Receiver<char>) -> Vec<char> {
        rx.try_iter().collect()
    }

    /// Bindings in one mode must not fire in a different mode.
    pub fn test_modal_mode_isolation<E: TestKeyEditor>(editor: E)
    where
        E::BufferHandle: std::hash::Hash,
    {
        let editor = Arc::new(editor);
        let (tx, rx) = mpsc::channel::<char>();

        let inner: ModalKeymap<E, TestMode> = modal_keymap! {
            initial: TestMode::A,
            [TestMode::B]: {
                "a" => { tx.send('x').unwrap(); Ok(()) },
            },
        };
        let mc = inner.mode_controller();

        let km = LocalizedKeymap::new(editor.clone(), inner);
        km.activate().unwrap();

        // In mode A, 'a' must be silently dropped.
        editor.send_test_key(&kp('a'));
        assert_eq!(collect(&rx), vec![], "key must not fire in wrong mode");

        // Switch to mode B — now 'a' must fire.
        mc.set_mode(TestMode::B);
        editor.send_test_key(&kp('a'));
        assert_eq!(collect(&rx), vec!['x']);
    }

    /// An action can switch the active mode; subsequent keys dispatch in the new mode.
    pub fn test_modal_mode_transition<E: TestKeyEditor>(editor: E)
    where
        E::BufferHandle: std::hash::Hash,
    {
        let editor = Arc::new(editor);
        let (tx, rx) = mpsc::channel::<char>();

        let tx_a = tx.clone();
        let tx_b = tx.clone();

        let inner: ModalKeymap<E, TestMode> = modal_keymap! {
            controller: ctrl,
            initial: TestMode::A,
            [TestMode::A]: {
                "i" => { ctrl.set_mode(TestMode::B); Ok(()) },
                "a" => { tx_a.send('y').unwrap(); Ok(()) },
            },
            [TestMode::B]: {
                "a" => { tx_b.send('x').unwrap(); Ok(()) },
            },
        };
        let mc = inner.mode_controller();

        let km = LocalizedKeymap::new(editor.clone(), inner);
        km.activate().unwrap();

        // 'i' in A switches to B, no output.
        editor.send_test_key(&kp('i'));
        assert_eq!(collect(&rx), vec![]);
        assert_eq!(mc.current_mode(), TestMode::B);

        // In B, 'a' fires B's binding ('x'), not A's ('y').
        editor.send_test_key(&kp('a'));
        assert_eq!(collect(&rx), vec!['x']);
    }

    /// Unbound keys are silently dropped and do not corrupt the accumulator.
    pub fn test_modal_no_match_dropped<E: TestKeyEditor>(editor: E)
    where
        E::BufferHandle: std::hash::Hash,
    {
        let editor = Arc::new(editor);
        let (tx, rx) = mpsc::channel::<char>();

        let inner: ModalKeymap<E, TestMode> = modal_keymap! {
            initial: TestMode::A,
            [TestMode::A]: {
                "a" => { tx.send('x').unwrap(); Ok(()) },
            },
        };

        let km = LocalizedKeymap::new(editor.clone(), inner);
        km.activate().unwrap();

        // 'b' has no binding; must be silently dropped.
        editor.send_test_key(&kp('b'));
        assert_eq!(collect(&rx), vec![]);

        // 'a' still works — accumulator was properly reset.
        editor.send_test_key(&kp('a'));
        assert_eq!(collect(&rx), vec!['x']);
    }

    /// Buffer-local bindings take priority over global bindings within the same mode.
    pub fn test_modal_local_binding_priority<E: TestKeyEditor>(editor: E)
    where
        E::BufferHandle: std::hash::Hash,
    {
        let editor = Arc::new(editor);
        let (tx, rx) = mpsc::channel::<char>();

        let current_buf = editor.current_buffer().expect("no current buffer");

        let tx_global = tx.clone();
        let tx_local = tx.clone();

        let inner: ModalKeymap<E, TestMode> = modal_keymap! {
            initial: TestMode::A,
            [TestMode::A]: {
                "a" => { tx_global.send('g').unwrap(); Ok(()) },
            },
        };

        let shared_state = inner.shared_state();
        let mut local: ModalKeymap<E, TestMode> =
            ModalKeymap::with_shared_state(shared_state.clone());
        local
            .keymap_for_mode(TestMode::A)
            .bind(&[kp('a')], move |_: &E| {
                tx_local.send('l').unwrap();
                Ok(())
            });

        let mut km = LocalizedKeymap::new(editor.clone(), inner);
        km.set_local(current_buf, local);
        km.activate().unwrap();

        editor.send_test_key(&kp('a'));
        assert_eq!(collect(&rx), vec!['l']);
    }

    /// Multi-key sequences accumulate correctly within a single mode.
    pub fn test_modal_seq_accumulation<E: TestKeyEditor>(editor: E)
    where
        E::BufferHandle: std::hash::Hash,
    {
        let editor = Arc::new(editor);
        let (tx, rx) = mpsc::channel::<char>();

        let inner: ModalKeymap<E, TestMode> = modal_keymap! {
            initial: TestMode::A,
            [TestMode::A]: {
                "ab" => { tx.send('x').unwrap(); Ok(()) },
            },
        };

        let km = LocalizedKeymap::new(editor.clone(), inner);
        km.activate().unwrap();

        // 'a' alone is a partial match — must not fire yet.
        editor.send_test_key(&kp('a'));
        assert_eq!(collect(&rx), vec![]);

        // 'b' completes the sequence.
        editor.send_test_key(&kp('b'));
        assert_eq!(collect(&rx), vec!['x']);
    }

    /// Changing mode mid-sequence resets the accumulator.
    pub fn test_modal_mode_change_clears_seq<E: TestKeyEditor>(editor: E)
    where
        E::BufferHandle: std::hash::Hash,
    {
        let editor = Arc::new(editor);
        let (tx, rx) = mpsc::channel::<char>();

        let inner: ModalKeymap<E, TestMode> = modal_keymap! {
            initial: TestMode::A,
            [TestMode::A]: {
                "ab" => { tx.send('x').unwrap(); Ok(()) },
            },
        };
        let mc = inner.mode_controller();

        let km = LocalizedKeymap::new(editor.clone(), inner);
        km.activate().unwrap();

        // Start accumulating in A.
        editor.send_test_key(&kp('a'));
        assert_eq!(collect(&rx), vec![]);

        // Switch to B mid-sequence — clears accumulating flag.
        mc.set_mode(TestMode::B);

        // 'b' would complete the A sequence, but accumulating was cleared.
        editor.send_test_key(&kp('b'));
        assert_eq!(
            collect(&rx),
            vec![],
            "action must not fire after mode change"
        );
    }

    /// Cloning a [`ModalKeymap`] produces an independent copy: mode changes
    /// via one keymap's [`ModeController`] must not affect the other.
    pub fn test_modal_clone_state_independent<E: TestKeyEditor>(_editor: E)
    where
        E::BufferHandle: std::hash::Hash,
    {
        let km: ModalKeymap<E, TestMode> = modal_keymap! { initial: TestMode::A, };
        let mc_orig = km.mode_controller();

        // Advance original to mode B before cloning.
        mc_orig.set_mode(TestMode::B);

        // Clone captures the current mode (B) but has independent state.
        let clone = km.clone();
        let mc_clone = clone.mode_controller();

        assert_eq!(
            mc_clone.current_mode(),
            TestMode::B,
            "clone starts in snapshotted mode"
        );

        // Switching original back to A must not affect the clone.
        mc_orig.set_mode(TestMode::A);
        assert_eq!(
            mc_clone.current_mode(),
            TestMode::B,
            "clone is unaffected by original mode change"
        );

        // Switching clone to A must not affect the original.
        mc_clone.set_mode(TestMode::A);
        assert_eq!(
            mc_orig.current_mode(),
            TestMode::A,
            "original is unaffected by clone mode change"
        );
    }
}

#[cfg(feature = "tests")]
#[macro_export]
macro_rules! eel_modal_tests {
    ($test_tag:path, $editor_factory:expr, $prefix:tt) => {
        $crate::eel_tests!(
            test_tag: $test_tag,
            editor_factory: $editor_factory,
            editor_bounds: {
                E: $crate::keymap::tests::TestKeyEditor,
                E::BufferHandle: ::std::hash::Hash,
            },
            module_path: $crate::keymap::modal::tests,
            prefix: $prefix,
            tests: [
                test_modal_mode_isolation,
                test_modal_mode_transition,
                test_modal_no_match_dropped,
                test_modal_local_binding_priority,
                test_modal_seq_accumulation,
                test_modal_mode_change_clears_seq,
                test_modal_clone_state_independent,
            ],
        );
    };

    ($test_tag:path, $editor_factory:expr) => {
        $crate::eel_modal_tests!($test_tag, $editor_factory, "");
    };
}

#[cfg(test)]
mod macro_tests {
    use super::*;
    use crate::keymap::key::parse_key_sequence;
    use crate::mock::{MockAction, MockEditor};

    #[derive(Debug, Clone, Eq, PartialEq, Hash)]
    enum Mode {
        Normal,
        Insert,
        Visual,
    }
    impl super::super::Mode for Mode {}

    fn modal_km_with_actions(
        initial: Mode,
        entries: &[(Mode, &[(&str, u32)])],
    ) -> ModalKeymap<MockEditor, Mode, MockAction> {
        let mut km = ModalKeymap::new(initial);
        for (mode, bindings) in entries {
            for (s, tag) in *bindings {
                km.keymap_for_mode(mode.clone())
                    .add_binding(&parse_key_sequence(s).unwrap(), MockAction(*tag));
            }
        }
        km
    }

    #[test]
    fn single_mode_single_binding() {
        let via_macro = modal_keymap! {
            initial: Mode::Normal,
            [Mode::Normal]: {
                "j" => MockAction(0),
            },
        };
        let manual = modal_km_with_actions(Mode::Normal, &[(Mode::Normal, &[("j", 0)])]);
        assert_eq!(via_macro, manual);
    }

    #[test]
    fn single_mode_multi_binding() {
        let via_macro = modal_keymap! {
            initial: Mode::Normal,
            [Mode::Normal]: {
                "j"     => MockAction(0),
                "gg"    => MockAction(1),
                "<C-j>" => MockAction(2),
            },
        };
        let manual = modal_km_with_actions(
            Mode::Normal,
            &[(Mode::Normal, &[("j", 0), ("gg", 1), ("<C-j>", 2)])],
        );
        assert_eq!(via_macro, manual);
        assert_ne!(
            via_macro,
            modal_km_with_actions(
                Mode::Normal,
                &[(Mode::Normal, &[("j", 0), ("gg", 99), ("<C-j>", 2)])],
            )
        );
    }

    #[test]
    fn multiple_separate_modes() {
        let via_macro = modal_keymap! {
            initial: Mode::Normal,
            [Mode::Normal]: {
                "j" => MockAction(0),
            },
            [Mode::Insert]: {
                "<Escape>" => MockAction(1),
            },
        };
        let manual = modal_km_with_actions(
            Mode::Normal,
            &[
                (Mode::Normal, &[("j", 0)]),
                (Mode::Insert, &[("<Escape>", 1)]),
            ],
        );
        assert_eq!(via_macro, manual);
        assert_ne!(
            via_macro,
            modal_km_with_actions(
                Mode::Normal,
                &[
                    (Mode::Normal, &[("j", 99)]),
                    (Mode::Insert, &[("<Escape>", 1)])
                ],
            )
        );
    }

    #[test]
    fn multi_mode_shared_block() {
        let via_macro = modal_keymap! {
            initial: Mode::Normal,
            [Mode::Normal, Mode::Visual]: {
                "j" => MockAction(0),
                "k" => MockAction(1),
            },
        };
        let manual = modal_km_with_actions(
            Mode::Normal,
            &[
                (Mode::Normal, &[("j", 0), ("k", 1)]),
                (Mode::Visual, &[("j", 0), ("k", 1)]),
            ],
        );
        assert_eq!(via_macro, manual);
        assert_ne!(
            via_macro,
            modal_km_with_actions(
                Mode::Normal,
                &[
                    (Mode::Normal, &[("j", 0), ("k", 1)]),
                    (Mode::Visual, &[("j", 99), ("k", 1)]),
                ],
            )
        );
    }

    #[test]
    fn multi_mode_shared_and_exclusive() {
        let via_macro = modal_keymap! {
            initial: Mode::Normal,
            [Mode::Normal, Mode::Visual]: {
                "j" => MockAction(0),
            },
            [Mode::Insert]: {
                "<Escape>" => MockAction(1),
            },
        };
        let manual = modal_km_with_actions(
            Mode::Normal,
            &[
                (Mode::Normal, &[("j", 0)]),
                (Mode::Visual, &[("j", 0)]),
                (Mode::Insert, &[("<Escape>", 1)]),
            ],
        );
        assert_eq!(via_macro, manual);
        assert_ne!(
            via_macro,
            modal_km_with_actions(
                Mode::Normal,
                &[
                    (Mode::Normal, &[("j", 99)]),
                    (Mode::Visual, &[("j", 0)]),
                    (Mode::Insert, &[("<Escape>", 1)]),
                ],
            )
        );
    }

    #[test]
    fn initial_mode_differs_not_equal() {
        let km_normal = modal_keymap! {
            initial: Mode::Normal,
            [Mode::Normal]: { "j" => MockAction(0) },
        };
        let km_insert = modal_keymap! {
            initial: Mode::Insert,
            [Mode::Normal]: { "j" => MockAction(0) },
        };
        assert_ne!(km_normal, km_insert);
    }

    #[test]
    fn empty_modal_keymap() {
        let via_macro: ModalKeymap<MockEditor, Mode, MockAction> = modal_keymap! {
            initial: Mode::Normal,
        };
        let manual: ModalKeymap<MockEditor, Mode, MockAction> = ModalKeymap::new(Mode::Normal);
        assert_eq!(via_macro, manual);
    }

    #[test]
    fn bare_block_with_editor_name() {
        use std::sync::{Arc, Mutex};
        let fired = Arc::new(Mutex::new(false));
        let fired_clone = fired.clone();
        let km: ModalKeymap<MockEditor, Mode> = modal_keymap! {
            initial: Mode::Normal,
            [Mode::Normal]: {
                "j" => {
                    *fired_clone.lock().unwrap() = true;
                    Ok(())
                },
            },
        };
        let seq = parse_key_sequence("j").unwrap();
        if let MatchResult::ExactMatch(action) = km.match_sequence(&seq) {
            action.call(&MockEditor).unwrap();
        } else {
            panic!("expected ExactMatch");
        }
        assert!(*fired.lock().unwrap());
    }

    #[test]
    fn controller_bound_in_block() {
        // `controller: ctrl` should make the mode controller available inside
        // each bare block so mode transitions can be triggered from within actions.
        let km: ModalKeymap<MockEditor, Mode> = modal_keymap! {
            controller: ctrl,
            initial: Mode::Normal,
            [Mode::Normal]: {
                "i" => {
                    ctrl.set_mode(Mode::Insert);
                    Ok(())
                },
            },
        };
        let mc = km.mode_controller();
        assert_eq!(mc.current_mode(), Mode::Normal);

        let seq = parse_key_sequence("i").unwrap();
        if let MatchResult::ExactMatch(action) = km.match_sequence(&seq) {
            action.call(&MockEditor).unwrap();
        } else {
            panic!("expected ExactMatch");
        }
        assert_eq!(mc.current_mode(), Mode::Insert);
    }
}
