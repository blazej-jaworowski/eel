use async_trait::async_trait;
use eel::{Position, Result, buffer::Buffer, cursor::CursorBuffer};

use crate::{
    error::{Error as NvimError, IntoNvimResult as _},
    window::NvimWindow,
};

use super::{NativePosition, NvimBuffer};

impl NvimBuffer {
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
