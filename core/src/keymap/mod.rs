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
