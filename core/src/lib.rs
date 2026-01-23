pub mod error;
pub use error::{Error, Result};

pub mod async_runtime;
pub mod tracing;

mod editor;
mod position;

pub use editor::Editor;
pub use position::Position;

pub mod buffer;

mod complete_buffer;
pub use complete_buffer::CompleteBufferHandle;

#[cfg(feature = "cursor")]
pub mod cursor;

#[cfg(feature = "mark")]
pub mod mark;

#[cfg(feature = "region")]
pub mod region;

#[cfg(feature = "tests")]
pub mod test_utils;

#[cfg(feature = "tests")]
mod tests {
    #[macro_export]
    #[cfg(not(feature = "cursor"))]
    macro_rules! eel_cursor_tests {
        ($test_tag:path, $editor_factory:expr $(, $_:tt)?) => {};
    }

    #[macro_export]
    #[cfg(not(feature = "mark"))]
    macro_rules! eel_mark_tests {
        ($test_tag:path, $editor_factory:expr $(, $_:tt)?) => {};
    }

    #[macro_export]
    #[cfg(not(feature = "region"))]
    macro_rules! eel_region_tests {
        ($test_tag:path, $editor_factory:expr $(, $_:tt)?) => {};
    }

    #[macro_export]
    macro_rules! eel_full_tests {
        ($test_tag:path, $editor_factory:expr) => {
            $crate::eel_buffer_tests!($test_tag, $editor_factory);
            $crate::eel_cursor_tests!($test_tag, $editor_factory);
            $crate::eel_mark_tests!($test_tag, $editor_factory);
            $crate::eel_region_tests!($test_tag, $editor_factory);
        };
    }
}
