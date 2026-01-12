pub mod error;
pub use error::{Error, Result};

pub mod async_runtime;
pub mod tracing;

mod editor;
mod position;

pub use editor::Editor;
pub use position::Position;

pub mod buffer;
pub mod cursor;
pub mod marks;

#[cfg(feature = "tests")]
pub mod test_utils;

#[cfg(feature = "tests")]
#[macro_export]
macro_rules! eel_full_tests {
    ($test_tag:path) => {
        $crate::eel_buffer_tests!($test_tag);
        $crate::eel_cursor_buffer_tests!($test_tag);
        $crate::eel_marks_tests!($test_tag);
    };
}
