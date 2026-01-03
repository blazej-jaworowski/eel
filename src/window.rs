use eel::{Position, Result};

use crate::{buffer::NativePosition, error::IntoNvimResult};

pub struct NvimWindow(nvim_oxi::api::Window);

impl NvimWindow {
    pub fn wrap(window: nvim_oxi::api::Window) -> Self {
        NvimWindow(window)
    }
}

impl NvimWindow {
    pub fn get_cursor(&self) -> Result<Position> {
        let native: NativePosition = self.0.get_cursor().into_nvim()?.into();
        Ok(native.into())
    }

    pub fn set_cursor(&mut self, position: &Position) -> Result<()> {
        let native: NativePosition = position.clone().into();
        self.0.set_cursor(native.row, native.col).into_nvim()?;
        Ok(())
    }
}
