use crate::{Editor, Result};

/// An action bound to a key sequence. Receives a reference to the editor.
pub trait KeyAction<E: Editor>: Send + Sync + 'static {
    fn call(&self, editor: &E) -> Result<()>;
}

impl<E, F> KeyAction<E> for F
where
    E: Editor,
    F: Fn(&E) -> Result<()> + Send + Sync + 'static,
{
    fn call(&self, editor: &E) -> Result<()> {
        (self)(editor)
    }
}
