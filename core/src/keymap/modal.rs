use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::Editor;

use super::{
    KeyPress,
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
pub struct ModalKeymap<E: Editor, M: Mode> {
    pub(crate) state: Arc<Mutex<ModalState<M>>>,
    bindings: HashMap<M, KeyMapping<E>>,
}

impl<E, M> ModalKeymap<E, M>
where
    E: Editor,
    M: Mode,
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
    pub fn keymap_for_mode(&mut self, mode: M) -> &mut KeyMapping<E> {
        self.bindings.entry(mode).or_default()
    }
}

impl<E: Editor, M: Mode> Clone for ModalKeymap<E, M> {
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

impl<E: Editor, M: Mode> Keymap<E> for ModalKeymap<E, M> {
    /// Match `seq` against the bindings for the current mode.
    ///
    /// On the first key of a new sequence (`seq.len() == 1`), sets the
    /// `accumulating` flag.  If the flag was cleared by a mode change before
    /// a subsequent key arrives (`seq.len() > 1 && !accumulating`), returns
    /// [`MatchResult::NoMatch`] so the external accumulator resets cleanly.
    fn match_sequence(&self, seq: &[KeyPress]) -> MatchResult<Arc<dyn super::KeyAction<E>>> {
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

#[cfg(feature = "tests")]
pub mod tests {
    use std::sync::{Arc, mpsc};

    use super::ModalKeymap;
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

        let mut inner: ModalKeymap<E, TestMode> = ModalKeymap::new(TestMode::A);
        let mc = inner.mode_controller();

        // Binding only exists in mode B.
        inner
            .keymap_for_mode(TestMode::B)
            .add_binding(&[kp('a')], move |_: &E| {
                tx.send('x').unwrap();
                Ok(())
            });

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

        let mut inner: ModalKeymap<E, TestMode> = ModalKeymap::new(TestMode::A);
        let mc = inner.mode_controller();

        // 'i' in A switches to B.
        {
            let mc = mc.clone();
            inner
                .keymap_for_mode(TestMode::A)
                .add_binding(&[kp('i')], move |_: &E| {
                    mc.set_mode(TestMode::B);
                    Ok(())
                });
        }
        // 'a' in A sends 'y'; 'a' in B sends 'x'.
        {
            let tx_a = tx.clone();
            inner
                .keymap_for_mode(TestMode::A)
                .add_binding(&[kp('a')], move |_: &E| {
                    tx_a.send('y').unwrap();
                    Ok(())
                });
        }
        {
            let tx_b = tx.clone();
            inner
                .keymap_for_mode(TestMode::B)
                .add_binding(&[kp('a')], move |_: &E| {
                    tx_b.send('x').unwrap();
                    Ok(())
                });
        }

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

        let mut inner: ModalKeymap<E, TestMode> = ModalKeymap::new(TestMode::A);
        inner
            .keymap_for_mode(TestMode::A)
            .add_binding(&[kp('a')], move |_: &E| {
                tx.send('x').unwrap();
                Ok(())
            });

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

        let mut inner: ModalKeymap<E, TestMode> = ModalKeymap::new(TestMode::A);
        inner
            .keymap_for_mode(TestMode::A)
            .add_binding(&[kp('a')], move |_: &E| {
                tx_global.send('g').unwrap();
                Ok(())
            });

        let shared_state = inner.shared_state();
        let mut local: ModalKeymap<E, TestMode> =
            ModalKeymap::with_shared_state(shared_state.clone());
        local
            .keymap_for_mode(TestMode::A)
            .add_binding(&[kp('a')], move |_: &E| {
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

        let mut inner: ModalKeymap<E, TestMode> = ModalKeymap::new(TestMode::A);
        inner
            .keymap_for_mode(TestMode::A)
            .add_binding(&[kp('a'), kp('b')], move |_: &E| {
                tx.send('x').unwrap();
                Ok(())
            });

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

        let mut inner: ModalKeymap<E, TestMode> = ModalKeymap::new(TestMode::A);
        let mc = inner.mode_controller();

        // Two-key sequence in mode A.
        inner
            .keymap_for_mode(TestMode::A)
            .add_binding(&[kp('a'), kp('b')], move |_: &E| {
                tx.send('x').unwrap();
                Ok(())
            });

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
        let km: ModalKeymap<E, TestMode> = ModalKeymap::new(TestMode::A);
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
