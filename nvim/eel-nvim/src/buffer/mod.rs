use std::{
    ops::RangeBounds,
    sync::{Arc, Weak},
};

use parking_lot::{ArcRwLockReadGuard, ArcRwLockWriteGuard, RwLock};
use tracing::trace;

use crate::{dispatcher::Dispatcher, error::Error as NvimError};

use eel::{
    Position, Result,
    buffer::{BufferHandle, Error as BufferError, ReadBuffer, WriteBuffer},
};

/// Represents a coordinate location within a Neovim buffer.
///
/// # Coordinate System
///
/// * **(1, 1)**: Represents the top-left corner of the buffer (first character of the first line).
/// * **Row**: Increases moving downwards.
/// * **Col**: Increases moving to the right.
///
/// # Bounds
///
/// Bounds depend on the editor state.
///
/// * **Normal Mode**: The `col` index typically ranges from `1` to `row_length` (if the line is not empty).
/// * **Insert Mode**: The `col` index may extend to `row_length + 1` to represent a cursor position
///   located immediately after the last character of the line.
///
/// `col` on an empty line will always be 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativePosition {
    pub row: usize,
    pub col: usize,
}

impl From<(usize, usize)> for NativePosition {
    fn from((row, col): (usize, usize)) -> Self {
        NativePosition { row, col }
    }
}

impl From<Position> for NativePosition {
    fn from(position: Position) -> Self {
        NativePosition {
            row: position.row + 1,
            col: position.col + 1,
        }
    }
}

impl From<NativePosition> for Position {
    fn from(position: NativePosition) -> Self {
        Self::new(
            position.row.saturating_sub(1),
            position.col.saturating_sub(1),
        )
    }
}

#[derive(Debug)]
pub struct NvimBuffer {
    handle: i32,
    dispatcher: Arc<Dispatcher>,
}

impl NvimBuffer {
    pub(crate) fn new(buffer: nvim_oxi::api::Buffer, dispatcher: Arc<Dispatcher>) -> Self {
        NvimBuffer {
            handle: buffer.handle(),
            dispatcher,
        }
    }

    pub(crate) fn inner_buf(&self) -> nvim_oxi::api::Buffer {
        self.handle.into()
    }
}

impl ReadBuffer for NvimBuffer {
    fn line_count(&self) -> Result<usize> {
        Ok(self.inner_buf().line_count().map_err(NvimError::from)?)
    }

    fn get_lines<R: RangeBounds<usize> + Send + 'static>(
        &self,
        range: R,
    ) -> Result<impl Iterator<Item = String> + Send> {
        let buf = self.inner_buf();

        let lines = self.dispatcher.dispatch(move || {
            let lines = buf
                .get_lines(range, true)
                .map_err(NvimError::from)?
                .map(|s| s.to_string())
                .collect::<Vec<String>>();

            Ok::<_, NvimError>(lines)
        })??;

        Ok(lines.into_iter())
    }
}

impl WriteBuffer for NvimBuffer {
    fn set_text(&mut self, start: &Position, end: &Position, text: &str) -> Result<()> {
        self.validate_pos(start)?;
        self.validate_pos(end)?;

        let mut buf = self.inner_buf();
        let text = text.to_string();
        let native_start: NativePosition = start.clone().into();
        let native_end: NativePosition = end.clone().into();

        self.dispatcher.dispatch(move || {
            nvim_oxi::api::set_option_value(
                "modified",
                true,
                &nvim_oxi::api::opts::OptionOpts::builder()
                    .buf(buf.clone())
                    .build(),
            )?;

            buf.set_text(
                (native_start.row - 1)..(native_end.row - 1),
                native_start.col - 1,
                native_end.col - 1,
                text.split("\n"),
            )?;

            // We only have to redraw if the buffer is visible, not sure if checking buffer
            // visibility would be faster though.
            nvim_oxi::api::command("redraw")?;

            Ok::<_, NvimError>(())
        })??;

        Ok(())
    }
}

#[derive(Clone, derivative::Derivative)]
#[derivative(Debug, Eq, PartialEq)]
pub struct NvimBufferHandle {
    pub(crate) id: i32,
    #[derivative(Debug = "ignore", PartialEq = "ignore")]
    buffer_lock: Weak<RwLock<NvimBuffer>>,
}

impl std::hash::Hash for NvimBufferHandle {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl NvimBufferHandle {
    pub(crate) fn new(id: i32, arc: &Arc<RwLock<NvimBuffer>>) -> Self {
        Self {
            id,
            buffer_lock: Arc::downgrade(arc),
        }
    }
}

impl BufferHandle for NvimBufferHandle {
    type ReadBuffer = NvimBuffer;
    type WriteBuffer = NvimBuffer;
    type ReadBufferLock = ArcRwLockReadGuard<parking_lot::RawRwLock, Self::ReadBuffer>;
    type WriteBufferLock = ArcRwLockWriteGuard<parking_lot::RawRwLock, Self::WriteBuffer>;

    fn read(&self) -> Result<Self::ReadBufferLock> {
        let arc = self.buffer_lock.upgrade().ok_or(BufferError::Dropped)?;
        let id = self.id;

        trace!(buffer_id = id, "Read-locking buffer");

        let lock = arc.read_arc();

        trace!(buffer_id = id, "Buffer read-locked");

        Ok(lock)
    }

    fn write(&self) -> Result<Self::WriteBufferLock> {
        let arc = self.buffer_lock.upgrade().ok_or(BufferError::Dropped)?;
        let id = self.id;

        trace!(buffer_id = id, "Write-locking buffer");

        let lock = arc.write_arc();

        trace!(buffer_id = id, "Buffer write-locked");

        Ok(lock)
    }
}

#[cfg(feature = "cursor")]
pub mod cursor;

#[cfg(feature = "mark")]
pub mod mark;

#[cfg(feature = "nvim-tests")]
mod tests {
    use std::{
        sync::{Arc, mpsc},
        time::Duration,
    };

    use eel::{
        Editor, Position,
        buffer::BufferHandle,
        eel_full_tests,
        mark::{MarkReadBuffer, MarkWriteBuffer},
        test_utils::new_buffer_with_content,
    };
    use eel_nvim_macros::nvim_test;

    #[nvim_test(editor_factory = crate::test_utils::nvim_editor_factory)]
    fn basic_test(_editor: impl Editor) {
        let var_key = "test_value";
        let original_value = String::from("Hello!");

        nvim_oxi::api::set_var(var_key, original_value.clone()).expect("Failed to set var");
        let value = nvim_oxi::api::get_var::<String>(var_key).expect("Failed to get var");

        assert_eq!(value, original_value);
    }

    #[nvim_test(editor_factory = crate::test_utils::nvim_editor_factory)]
    fn destroy_mark_does_not_wait_for_main_thread(editor: crate::editor::NvimEditor) {
        const WAIT: Duration = Duration::from_millis(250);

        let editor = Arc::new(editor);
        let buffer = new_buffer_with_content(editor.as_ref(), "test");
        let mark_id = {
            let mut buffer_lock = buffer.write().expect("buffer dropped");
            buffer_lock
                .create_mark(&Position::new(0, 0))
                .expect("Failed to create mark")
        };

        let (occupied_tx, occupied_rx) = mpsc::sync_channel(1);
        let (start_destroy_tx, start_destroy_rx) = mpsc::sync_channel(1);
        let (destroy_for_main_tx, destroy_for_main_rx) = mpsc::sync_channel(1);
        let (destroy_done_tx, destroy_done_rx) = mpsc::sync_channel(1);
        let (early_return_tx, early_return_rx) = mpsc::sync_channel(1);
        let (main_read_tx, main_read_rx) = mpsc::sync_channel(1);
        let (release_main_tx, release_main_rx) = mpsc::sync_channel(1);
        let (occupier_done_tx, occupier_done_rx) = mpsc::sync_channel(1);

        let destroy_worker = {
            let buffer = buffer.clone();

            std::thread::spawn(move || {
                if start_destroy_rx.recv_timeout(WAIT).is_err() {
                    let _ = destroy_for_main_tx.send(false);
                    let _ = destroy_done_tx.send(Err("Timed out waiting to destroy mark".into()));
                    return;
                }

                let (result, returned) = match buffer.write() {
                    Ok(mut buffer_lock) => {
                        let result = buffer_lock
                            .destroy_mark(mark_id)
                            .map_err(|error| error.to_string());
                        drop(buffer_lock);
                        (result, true)
                    }
                    Err(error) => (Err(error.to_string()), false),
                };

                let _ = destroy_for_main_tx.send(returned);
                let _ = destroy_done_tx.send(result);
            })
        };

        let occupier = {
            let editor = Arc::clone(&editor);
            let buffer = buffer.clone();

            std::thread::spawn(move || {
                let result = editor.dispatch(move || {
                    let _ = occupied_tx.send(());
                    let _ = start_destroy_tx.send(());

                    let returned = destroy_for_main_rx.recv_timeout(WAIT).unwrap_or(false);
                    let main_read_ok = returned
                        && buffer
                            .read()
                            .and_then(|buffer_lock| buffer_lock.get_mark_position(mark_id))
                            .is_ok();

                    let _ = early_return_tx.send(returned);
                    let _ = main_read_tx.send(main_read_ok);
                    let _ = release_main_rx.recv_timeout(WAIT);
                });

                let _ = occupier_done_tx.send(result.is_ok());
            })
        };

        let occupied = occupied_rx.recv_timeout(WAIT).is_ok();
        let returned_while_occupied = early_return_rx.recv_timeout(WAIT).unwrap_or(false);
        let main_read_ok = main_read_rx.recv_timeout(WAIT).unwrap_or(false);

        let _ = release_main_tx.send(());

        let barrier_ok = editor.dispatch(|| ()).is_ok();

        let worker_result = destroy_done_rx.recv_timeout(WAIT).ok();
        let occupier_result = occupier_done_rx.recv_timeout(WAIT).ok();

        if worker_result.is_some() {
            destroy_worker.join().expect("Mark destroy worker panicked");
        }
        if occupier_result.is_some() {
            occupier.join().expect("Main occupier panicked");
        }

        let mark_gone_after_barrier = worker_result.is_some()
            && buffer
                .read()
                .and_then(|buffer_lock| buffer_lock.get_mark_position(mark_id))
                .is_err();

        assert!(occupied, "Neovim main thread was not occupied");
        assert!(
            returned_while_occupied,
            "destroy_mark waited for Neovim main thread"
        );
        assert!(main_read_ok, "Neovim main thread could not read the buffer");
        assert!(
            worker_result.is_some_and(|result| result.is_ok()),
            "destroy_mark failed"
        );
        assert!(barrier_ok, "FIFO barrier did not complete");
        assert_eq!(
            occupier_result,
            Some(true),
            "Main-thread occupier did not complete"
        );
        assert!(
            mark_gone_after_barrier,
            "extmark was not gone after the FIFO barrier"
        );
    }

    eel_full_tests!(
        ::eel_nvim_macros::nvim_test,
        crate::test_utils::nvim_editor_factory
    );
}
