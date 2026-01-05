use std::{ops::RangeBounds, sync::Arc};

use async_trait::async_trait;
use tokio::sync::RwLock;
use tracing::trace;

use crate::{
    async_dispatch::Dispatcher,
    error::{Error as NvimError, IntoNvimResult as _},
    window::NvimWindow,
};

use eel::{
    Position, Result,
    buffer::{Buffer, BufferHandle},
    cursor::CursorBuffer,
};

/// Represents a loordinate location within a Neovim buffer.
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

    async fn get_window(&self) -> Result<Option<NvimWindow>> {
        let handle = self.handle;

        let nvim_window = self
            .dispatcher
            .dispatch(move || {
                nvim_oxi::api::list_wins().find(|win| {
                    if let Ok(buf) = win.get_buf() {
                        buf.handle() == handle
                    } else {
                        false
                    }
                })
            })
            .await?;

        Ok(nvim_window.map(|w| NvimWindow::wrap(w, self.dispatcher.clone())))
    }
}

#[async_trait]
impl Buffer for NvimBuffer {
    async fn line_count(&self) -> Result<usize> {
        Ok(self.inner_buf().line_count().map_err(NvimError::from)?)
    }

    async fn get_lines<R: RangeBounds<usize> + Send + 'static>(
        &self,
        range: R,
    ) -> Result<impl Iterator<Item = String> + Send> {
        let buf = self.inner_buf();

        let lines = self
            .dispatcher
            .dispatch(move || {
                let lines = buf
                    .get_lines(range, true)
                    .map_err(NvimError::from)?
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>();

                Ok::<_, NvimError>(lines)
            })
            .await??;

        Ok(lines.into_iter())
    }

    async fn set_text(&mut self, start: &Position, end: &Position, text: &str) -> Result<()> {
        self.validate_pos(start).await?;
        self.validate_pos(end).await?;

        let mut buf = self.inner_buf();
        let text = text.to_string();
        let native_start: NativePosition = start.clone().into();
        let native_end: NativePosition = end.clone().into();

        self.dispatcher
            .dispatch(move || {
                nvim_oxi::api::set_option_value(
                    "modified",
                    true,
                    &nvim_oxi::api::opts::OptionOpts::builder()
                        .buffer(buf.clone())
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
            })
            .await??;

        Ok(())
    }
}

#[async_trait]
impl CursorBuffer for NvimBuffer {
    async fn get_cursor(&self) -> Result<Position> {
        let position: Position = match self.get_window().await? {
            Some(w) => w.get_cursor().await?,
            None => {
                let native: NativePosition = self.inner_buf().get_mark('\"').into_nvim()?.into();
                native.into()
            }
        };

        if self.get_line(position.row).await?.is_empty() {
            Ok(Position::new(position.row, 0))
        } else {
            Ok(position)
        }
    }

    async fn set_cursor(&mut self, position: &Position) -> Result<()> {
        self.validate_pos(position).await?;

        match &mut self.get_window().await? {
            Some(w) => w.set_cursor(position).await?,
            None => {
                let native: NativePosition = position.clone().into();
                self.inner_buf()
                    .set_mark('\"', native.row, native.col, &Default::default())
                    .map_err(NvimError::from)?
            }
        };

        Ok(())
    }
}

#[derive(Clone, derivative::Derivative)]
#[derivative(Debug)]
pub struct NvimBufferHandle {
    id: i32,
    #[derivative(Debug = "ignore")]
    buffer_lock: Arc<RwLock<NvimBuffer>>,
}

impl NvimBufferHandle {
    pub(crate) fn new(buffer: NvimBuffer) -> Self {
        Self {
            id: buffer.inner_buf().handle(),
            buffer_lock: Arc::new(RwLock::new(buffer)),
        }
    }
}

impl BufferHandle<NvimBuffer> for NvimBufferHandle {
    fn read(
        &self,
    ) -> impl Future<Output = impl std::ops::Deref<Target = NvimBuffer> + Sync + Send + 'static>
    + Send
    + 'static {
        let lock = self.buffer_lock.clone();
        let id = self.id;

        async move {
            trace!(buffer_id = id, "Read-locking buffer");

            let lock = lock.read_owned().await;

            trace!(buffer_id = id, "Buffer read-locked");

            lock
        }
    }

    fn write(
        &self,
    ) -> impl Future<Output = impl std::ops::DerefMut<Target = NvimBuffer> + Send + 'static>
    + Send
    + 'static {
        let lock = self.buffer_lock.clone();
        let id = self.id;

        async move {
            trace!(buffer_id = id, "Write-locking buffer");

            let lock = lock.write_owned().await;

            trace!(buffer_id = id, "Buffer write-locked");

            lock
        }
    }
}

#[cfg(feature = "nvim_tests")]
mod tests {
    use crate::{editor::NvimEditor, test_utils::run_nvim_async_test};
    use eel::{eel_buffer_tests, eel_cursor_buffer_tests};
    use eel_nvim_macros::nvim_test;

    #[nvim_test]
    async fn basic_test(_editor: NvimEditor) {
        let var_key = "test_value";
        let original_value = String::from("Hello!");

        nvim_oxi::api::set_var(var_key, original_value.clone()).expect("Failed to set var");
        let value = nvim_oxi::api::get_var::<String>(var_key).expect("Failed to get var");

        assert_eq!(value, original_value);
    }

    eel_buffer_tests!(nvim_test);
    eel_cursor_buffer_tests!(nvim_test);
}
