pub mod error;
pub use error::{Error, Result};

pub mod tracing;

pub mod editor;
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

#[cfg(feature = "window")]
pub mod window;

#[cfg(feature = "keymap")]
pub mod keymap;

#[cfg(feature = "tests")]
pub mod test_utils;

#[cfg(test)]
pub(crate) mod mock;

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
    #[cfg(not(feature = "window"))]
    macro_rules! eel_window_tests {
        ($test_tag:path, $editor_factory:expr $(, $_:tt)?) => {};
    }

    #[macro_export]
    macro_rules! eel_full_tests {
        ($test_tag:path, $editor_factory:expr) => {
            $crate::eel_buffer_tests!($test_tag, $editor_factory);
            $crate::eel_cursor_tests!($test_tag, $editor_factory);
            $crate::eel_mark_tests!($test_tag, $editor_factory);
            $crate::eel_region_tests!($test_tag, $editor_factory);
            $crate::eel_editor_tests!($test_tag, $editor_factory);
            $crate::eel_window_tests!($test_tag, $editor_factory);
        };
    }
}
