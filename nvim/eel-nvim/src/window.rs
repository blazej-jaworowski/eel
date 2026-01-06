use std::sync::Arc;

use eel::{Position, Result};

use crate::{async_dispatch::Dispatcher, buffer::NativePosition, error::IntoNvimResult};

pub struct NvimWindow {
    inner: nvim_oxi::api::Window,
    dispatcher: Arc<Dispatcher>,
}

impl NvimWindow {
    pub fn wrap(window: nvim_oxi::api::Window, dispatcher: Arc<Dispatcher>) -> Self {
        NvimWindow {
            inner: window,
            dispatcher,
        }
    }
}

impl NvimWindow {
    pub async fn get_cursor(&self) -> Result<Position> {
        let window = self.inner.clone();

        let native: NativePosition = self
            .dispatcher
            .dispatch(move || window.get_cursor().into_nvim())
            .await??
            .into();

        Ok(native.into())
    }

    pub async fn set_cursor(&mut self, position: &Position) -> Result<()> {
        let native: NativePosition = position.clone().into();

        let mut window = self.inner.clone();

        self.dispatcher
            .dispatch(move || {
                window.set_cursor(native.row, native.col).into_nvim()?;

                nvim_oxi::api::command("redraw").into_nvim()
            })
            .await??;

        Ok(())
    }
}
