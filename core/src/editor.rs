use crate::{Result, buffer::BufferHandle};

pub trait Editor: Sized + Sync + Send + 'static {
    type BufferHandle: BufferHandle;

    fn current_buffer(&self) -> Result<Self::BufferHandle>;
    fn set_current_buffer(&self, buffer: &Self::BufferHandle) -> Result<()>;

    fn new_buffer(&self) -> Result<Self::BufferHandle>;
    fn kill_buffer(&self, buffer: &Self::BufferHandle) -> Result<()>;
}

#[cfg(feature = "tests")]
pub mod tests {
    use super::*;
    use crate::buffer::Error as BufferError;

    pub fn test_editor_new_buffer(editor: impl Editor) {
        let buffer = editor.new_buffer().expect("Failed to create buffer");

        assert!(
            buffer.read().is_ok(),
            "read() should succeed on a new buffer"
        );
        assert!(
            buffer.write().is_ok(),
            "write() should succeed on a new buffer"
        );
    }

    pub fn test_editor_kill_buffer(editor: impl Editor) {
        let buffer = editor.new_buffer().expect("Failed to create buffer");

        editor.kill_buffer(&buffer).expect("Failed to kill buffer");

        assert!(
            matches!(
                buffer.read(),
                Err(crate::Error::Buffer(BufferError::Dropped))
            ),
            "Expected Dropped error after kill_buffer"
        );
        assert!(
            matches!(
                buffer.write(),
                Err(crate::Error::Buffer(BufferError::Dropped))
            ),
            "Expected Dropped error after kill_buffer"
        );
    }

    #[macro_export]
    macro_rules! eel_editor_tests {
        ($test_tag:path, $editor_factory:expr, $prefix:tt) => {
            $crate::eel_tests!(
                test_tag: $test_tag,
                editor_factory: $editor_factory,
                editor_bounds: {},
                module_path: $crate::editor::tests,
                prefix: $prefix,
                tests: [test_editor_new_buffer, test_editor_kill_buffer],
            );
        };

        ($test_tag:path, $editor_factory:expr) => {
            $crate::eel_editor_tests!($test_tag, $editor_factory, "");
        };
    }
}
