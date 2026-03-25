use eel::{
    Position, Result,
    buffer::ReadBuffer,
    cursor::{CursorReadBuffer, CursorWriteBuffer},
};

use crate::{
    buffer::NativePosition,
    error::{Error as NvimError, IntoNvimResult as _},
};

use super::NvimBuffer;

impl NvimBuffer {
    fn get_window(&self) -> Result<Option<nvim_oxi::api::Window>> {
        let handle = self.handle;

        let nvim_window = self.dispatcher.dispatch(move || {
            nvim_oxi::api::list_wins().find(|win| {
                if let Ok(buf) = win.get_buf() {
                    buf.handle() == handle
                } else {
                    false
                }
            })
        })?;

        Ok(nvim_window)
    }
}

impl CursorReadBuffer for NvimBuffer {
    fn get_cursor(&self) -> Result<Position> {
        let position: Position = match self.get_window()? {
            Some(win) => {
                let native: NativePosition = self
                    .dispatcher
                    .dispatch(move || win.get_cursor().into_nvim())??
                    .into();
                native.into()
            }
            None => {
                let native: NativePosition = self.inner_buf().get_mark('\"').into_nvim()?.into();
                native.into()
            }
        };

        if self.get_line(position.row)?.is_empty() {
            Ok(Position::new(position.row, 0))
        } else {
            Ok(position)
        }
    }
}

impl CursorWriteBuffer for NvimBuffer {
    fn set_cursor(&mut self, position: &Position) -> Result<()> {
        self.validate_pos(position)?;

        match self.get_window()? {
            Some(mut win) => {
                let native: NativePosition = position.clone().into();
                self.dispatcher.dispatch(move || {
                    win.set_cursor(native.row, native.col).into_nvim()?;
                    nvim_oxi::api::command("redraw").into_nvim()
                })??;
            }
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
