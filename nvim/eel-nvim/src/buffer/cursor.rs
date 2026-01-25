use eel::{
    Position, Result,
    buffer::ReadBuffer,
    cursor::{CursorReadBuffer, CursorWriteBuffer},
};

use crate::{
    error::{Error as NvimError, IntoNvimResult as _},
    window::NvimWindow,
};

use super::{NativePosition, NvimBuffer};

impl NvimBuffer {
    fn get_window(&self) -> Result<Option<NvimWindow>> {
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

        Ok(nvim_window.map(|w| NvimWindow::wrap(w, self.dispatcher.clone())))
    }
}

impl CursorReadBuffer for NvimBuffer {
    fn get_cursor(&self) -> Result<Position> {
        let position: Position = match self.get_window()? {
            Some(w) => w.get_cursor()?,
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

        match &mut self.get_window()? {
            Some(w) => w.set_cursor(position)?,
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
