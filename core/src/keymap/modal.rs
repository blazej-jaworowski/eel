use std::{
    collections::HashMap,
    hash::Hash,
    sync::{Arc, Mutex},
};

use crate::{Editor, Result, tracing::ResultExt as _};

use super::{KeyEditor, KeySequence, map::Keymap};

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
pub trait Mode: Eq + Hash + Clone + Send + Sync + 'static {}

/// Runtime state shared between a [`ModalKeymap`] and all [`ModeController`]
/// clones that were derived from it.
struct ModalState<M: Mode> {
    mode: M,
    seq: KeySequence,
}

impl<M: Mode> ModalState<M> {
    fn clear_seq(&mut self) {
        self.seq.clear();
    }
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
    /// If `mode` differs from the current mode, also clears the accumulated
    /// key-press sequence so that a partial prefix started in the old mode can
    /// never complete in the new one.  Calling with the current mode is a no-op.
    pub fn set_mode(&self, mode: M) {
        let mut s = self.state.lock().unwrap();
        if s.mode != mode {
            s.clear_seq();
        }
        s.mode = mode;
    }
}

/// A keymap that dispatches key presses according to the current mode.
///
/// Each mode has its own global and buffer-local bindings managed through the
/// standard [`Keymap`] API, accessible via [`ModalKeymap::keymap_for_mode`].
///
/// Mode transitions are performed through a [`ModeController`] obtained via
/// [`ModalKeymap::mode_controller`].  Calling [`ModeController::set_mode`]
/// atomically updates the mode and clears the accumulated key-press sequence,
/// so stale partial sequences can never fire across mode boundaries.
///
/// Unmatched key presses in any mode are silently dropped.
pub struct ModalKeymap<E: Editor, M: Mode> {
    state: Arc<Mutex<ModalState<M>>>,
    bindings: HashMap<M, Keymap<E>>,
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
                seq: Vec::new(),
            })),
            bindings: HashMap::new(),
        }
    }

    /// Returns a [`ModeController`] that can be cloned and captured in actions
    /// to read or change the active mode.
    pub fn mode_controller(&self) -> ModeController<M> {
        ModeController {
            state: Arc::clone(&self.state),
        }
    }

    /// Returns a mutable reference to the [`Keymap`] for `mode`, creating an
    /// empty one if it does not yet exist.
    ///
    /// Use this to add or remove bindings for a specific mode:
    ///
    /// ```ignore
    /// km.keymap_for_mode(Mode::Normal).add_global(&[kp('i')], action);
    /// km.keymap_for_mode(Mode::Insert).add_local(buf, &[kp('a')], action);
    /// ```
    pub fn keymap_for_mode(&mut self, mode: M) -> &mut Keymap<E> {
        self.bindings.entry(mode).or_default()
    }
}

impl<E: Editor, M: Mode> Clone for ModalKeymap<E, M> {
    /// Creates an independent clone of this keymap.
    ///
    /// The clone has:
    /// - the same bindings (deep-copied via [`Keymap`]'s own `Clone` impl),
    /// - a fresh key-press sequence accumulator (empty),
    /// - a snapshotted copy of the current mode.
    ///
    /// The original and the clone have **independent** runtime state: a
    /// [`ModeController`] obtained from one will not affect the other.
    fn clone(&self) -> Self {
        Self {
            bindings: self.bindings.clone(),
            state: Arc::new(Mutex::new(ModalState {
                mode: self.state.lock().unwrap().mode.clone(),
                seq: Vec::new(),
            })),
        }
    }
}

impl<E, M> ModalKeymap<E, M>
where
    E: KeyEditor + 'static,
    M: Mode,
    E::BufferHandle: Hash,
{
    /// Activate this modal keymap on `editor`.
    ///
    /// Installs a single [`KeyEditor::capture_keys`] handler that:
    ///
    /// - Reads the current mode and accumulated sequence from the shared state.
    /// - Dispatches to the mode's bindings using prefix-trie matching.
    /// - On exact match: fires the action and clears the accumulator.
    /// - On partial match: keeps accumulating.
    /// - On no match or unknown mode: silently drops the accumulator.
    ///
    /// Mode changes via [`ModeController::set_mode`] atomically clear the
    /// accumulator, so no mid-sequence pollution can cross mode boundaries.
    pub fn activate(self, editor: Arc<E>) -> Result<()> {
        let bindings = Arc::new(self.bindings);
        let state = self.state;
        let editor_for_cb = Arc::clone(&editor);

        editor.capture_keys(move |key_press| {
            // Push key and snapshot mode under a single lock acquisition.
            let current_mode = {
                let mut s = state.lock().unwrap();
                s.seq.push(key_press.clone());
                s.mode.clone()
            };

            let current_buf = editor_for_cb.current_buffer().ok();

            let result = {
                let s = state.lock().unwrap();
                match bindings.get(&current_mode) {
                    Some(km) => km.match_sequence(current_buf.as_ref(), &s.seq),
                    None => {
                        drop(s);
                        state.lock().unwrap().clear_seq();
                        return;
                    }
                }
            };

            use crate::keymap::map::MatchResult;
            match result {
                MatchResult::ExactMatch(action) => {
                    state.lock().unwrap().clear_seq();
                    _ = action
                        .call(&editor_for_cb)
                        .log_err_msg("ModalKeymap action failed");
                }
                MatchResult::PartialMatch => { /* keep accumulating */ }
                MatchResult::NoMatch => state.lock().unwrap().clear_seq(),
            }
        })
    }
}

#[cfg(feature = "tests")]
pub mod tests {
    use std::sync::{Arc, mpsc};

    use super::ModalKeymap;
    use crate::keymap::KeyPress;
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

        let mut km: ModalKeymap<E, TestMode> = ModalKeymap::new(TestMode::A);
        let mc = km.mode_controller();

        // Binding only exists in mode B.
        km.keymap_for_mode(TestMode::B)
            .add_global(&[kp('a')], move |_: &E| {
                tx.send('x').unwrap();
                Ok(())
            });

        km.activate(Arc::clone(&editor)).unwrap();

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

        let mut km: ModalKeymap<E, TestMode> = ModalKeymap::new(TestMode::A);
        let mc = km.mode_controller();

        // 'i' in A switches to B.
        {
            let mc = mc.clone();
            km.keymap_for_mode(TestMode::A)
                .add_global(&[kp('i')], move |_: &E| {
                    mc.set_mode(TestMode::B);
                    Ok(())
                });
        }
        // 'a' in A sends 'y'; 'a' in B sends 'x'.
        {
            let tx_a = tx.clone();
            km.keymap_for_mode(TestMode::A)
                .add_global(&[kp('a')], move |_: &E| {
                    tx_a.send('y').unwrap();
                    Ok(())
                });
        }
        {
            let tx_b = tx.clone();
            km.keymap_for_mode(TestMode::B)
                .add_global(&[kp('a')], move |_: &E| {
                    tx_b.send('x').unwrap();
                    Ok(())
                });
        }

        km.activate(Arc::clone(&editor)).unwrap();

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

        let mut km: ModalKeymap<E, TestMode> = ModalKeymap::new(TestMode::A);
        km.keymap_for_mode(TestMode::A)
            .add_global(&[kp('a')], move |_: &E| {
                tx.send('x').unwrap();
                Ok(())
            });

        km.activate(Arc::clone(&editor)).unwrap();

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

        let mut km: ModalKeymap<E, TestMode> = ModalKeymap::new(TestMode::A);
        km.keymap_for_mode(TestMode::A)
            .add_global(&[kp('a')], move |_: &E| {
                tx_global.send('g').unwrap();
                Ok(())
            });
        km.keymap_for_mode(TestMode::A)
            .add_local(current_buf, &[kp('a')], move |_: &E| {
                tx_local.send('l').unwrap();
                Ok(())
            });

        km.activate(Arc::clone(&editor)).unwrap();
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

        let mut km: ModalKeymap<E, TestMode> = ModalKeymap::new(TestMode::A);
        km.keymap_for_mode(TestMode::A)
            .add_global(&[kp('a'), kp('b')], move |_: &E| {
                tx.send('x').unwrap();
                Ok(())
            });

        km.activate(Arc::clone(&editor)).unwrap();

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

        let mut km: ModalKeymap<E, TestMode> = ModalKeymap::new(TestMode::A);
        let mc = km.mode_controller();

        // Two-key sequence in mode A.
        km.keymap_for_mode(TestMode::A)
            .add_global(&[kp('a'), kp('b')], move |_: &E| {
                tx.send('x').unwrap();
                Ok(())
            });

        km.activate(Arc::clone(&editor)).unwrap();

        // Start accumulating in A.
        editor.send_test_key(&kp('a'));
        assert_eq!(collect(&rx), vec![]);

        // Switch to B mid-sequence — clears accumulator.
        mc.set_mode(TestMode::B);

        // 'b' would complete the A sequence, but accumulator was cleared.
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
