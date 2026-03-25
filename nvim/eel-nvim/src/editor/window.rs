use std::{collections::HashMap, sync::Arc};

use eel::window::{
    ReadWindowLock, WindowDimensions, WindowEditor, WindowId, WindowOpenConfig, WindowPosition,
    WindowStoreHandle, WriteWindowLock,
};
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::{buffer::NvimBufferHandle, dispatcher::Dispatcher, error::IntoNvimResult};

use super::NvimEditor;

struct NvimWindowEntry {
    buffer: Option<NvimBufferHandle>,
    is_floating: bool,
}

pub struct NvimWindowStore {
    windows: HashMap<i32, NvimWindowEntry>,
    dispatcher: Arc<Dispatcher>,
}

impl NvimWindowStore {
    fn new(dispatcher: Arc<Dispatcher>) -> Self {
        Self {
            windows: HashMap::new(),
            dispatcher,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct NvimWindowId(i32);

impl WindowId for NvimWindowId {}

#[derive(Clone)]
pub struct NvimWindowStoreHandle(Arc<RwLock<NvimWindowStore>>);

impl NvimWindowStoreHandle {
    pub fn new(dispatcher: Arc<Dispatcher>) -> Self {
        Self(Arc::new(RwLock::new(NvimWindowStore::new(dispatcher))))
    }
}

impl std::fmt::Debug for NvimWindowStoreHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("NvimWindowStoreHandle").finish()
    }
}

impl PartialEq for NvimWindowStoreHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for NvimWindowStoreHandle {}

impl ReadWindowLock for NvimWindowStore {
    type WindowId = NvimWindowId;
    type BufferHandle = NvimBufferHandle;

    fn current_window_id(&self) -> eel::Result<NvimWindowId> {
        let dispatcher = self.dispatcher.clone();
        let win = dispatcher.dispatch(nvim_oxi::api::get_current_win)?;
        Ok(NvimWindowId(win.handle()))
    }

    fn list_window_ids(&self) -> eel::Result<Vec<NvimWindowId>> {
        Ok(self.windows.keys().map(|&id| NvimWindowId(id)).collect())
    }

    fn get_buffer(&self, id: NvimWindowId) -> eel::Result<Option<NvimBufferHandle>> {
        self.windows
            .get(&id.0)
            .map(|e| e.buffer.clone())
            .ok_or_else(|| eel::Error::Window(eel::window::Error::InvalidWindow))
    }

    fn get_dimensions(&self, id: NvimWindowId) -> eel::Result<WindowDimensions> {
        let dispatcher = self.dispatcher.clone();
        let width = dispatcher
            .dispatch(move || nvim_oxi::api::Window::from(id.0).get_width().into_nvim())??;
        let height = dispatcher
            .dispatch(move || nvim_oxi::api::Window::from(id.0).get_height().into_nvim())??;
        Ok(WindowDimensions {
            width: width as usize,
            height: height as usize,
        })
    }

    fn get_position(&self, id: NvimWindowId) -> eel::Result<WindowPosition> {
        let dispatcher = self.dispatcher.clone();
        let (row, col) = dispatcher
            .dispatch(move || nvim_oxi::api::Window::from(id.0).get_position().into_nvim())??;
        Ok(WindowPosition { row, col })
    }

    fn is_floating(&self, id: NvimWindowId) -> eel::Result<bool> {
        self.windows
            .get(&id.0)
            .map(|e| e.is_floating)
            .ok_or_else(|| eel::Error::Window(eel::window::Error::InvalidWindow))
    }
}

impl WriteWindowLock for NvimWindowStore {
    fn new_window(
        &mut self,
        buffer: Option<&NvimBufferHandle>,
        config: WindowOpenConfig<NvimWindowId>,
    ) -> eel::Result<NvimWindowId> {
        if let WindowOpenConfig::Split(ref split) = config
            && self
                .windows
                .get(&split.window.0)
                .map(|e| e.is_floating)
                .unwrap_or(false)
        {
            return Err(eel::window::Error::CannotSplitFloat.into());
        }

        let is_floating = matches!(config, WindowOpenConfig::Float(_));
        let dispatcher = self.dispatcher.clone();
        let buf_id: Option<i32> = buffer.map(|h| h.id);

        let win = dispatcher.dispatch(move || -> eel::Result<nvim_oxi::api::Window> {
            let buf: nvim_oxi::api::Buffer = if let Some(id) = buf_id {
                id.into()
            } else {
                nvim_oxi::api::create_buf(false, true).into_nvim()?
            };

            let mut nvim_config = nvim_oxi::api::types::WindowConfig::default();

            match config {
                WindowOpenConfig::Split(split) => {
                    // Validate split_at: must be 1 <= split_at < dim so both
                    // resulting windows are non-zero.
                    let target_win = nvim_oxi::api::Window::from(split.window.0);
                    let dim = match split.direction {
                        eel::window::SplitDirection::Left | eel::window::SplitDirection::Right => {
                            target_win.get_width().into_nvim()? as usize
                        }
                        eel::window::SplitDirection::Above | eel::window::SplitDirection::Below => {
                            target_win.get_height().into_nvim()? as usize
                        }
                    };
                    if split.split_at == 0 || split.split_at >= dim {
                        return Err(eel::window::Error::InvalidSplitAt {
                            split_at: split.split_at,
                            limit: dim.saturating_sub(1),
                        }
                        .into());
                    }

                    nvim_config.split = Some(match split.direction {
                        eel::window::SplitDirection::Above => {
                            nvim_oxi::api::types::SplitDirection::Above
                        }
                        eel::window::SplitDirection::Below => {
                            nvim_oxi::api::types::SplitDirection::Below
                        }
                        eel::window::SplitDirection::Left => {
                            nvim_oxi::api::types::SplitDirection::Left
                        }
                        eel::window::SplitDirection::Right => {
                            nvim_oxi::api::types::SplitDirection::Right
                        }
                    });
                    nvim_config.win = Some(nvim_oxi::api::Window::from(split.window.0));
                    match split.direction {
                        eel::window::SplitDirection::Left | eel::window::SplitDirection::Right => {
                            nvim_config.width = Some(split.split_at as u32);
                        }
                        eel::window::SplitDirection::Above | eel::window::SplitDirection::Below => {
                            nvim_config.height = Some(split.split_at as u32);
                        }
                    }
                }
                WindowOpenConfig::Float(float) => {
                    let cols = nvim_oxi::api::get_option_value::<u32>(
                        "columns",
                        &nvim_oxi::api::opts::OptionOpts::default(),
                    )
                    .into_nvim()? as usize;
                    let rows = nvim_oxi::api::get_option_value::<u32>(
                        "lines",
                        &nvim_oxi::api::opts::OptionOpts::default(),
                    )
                    .into_nvim()? as usize;
                    if float.position.col + float.dimensions.width > cols
                        || float.position.row + float.dimensions.height > rows
                    {
                        return Err(eel::window::Error::FloatOutOfBounds.into());
                    }

                    nvim_config.relative = Some(nvim_oxi::api::types::WindowRelativeTo::Editor);
                    nvim_config.anchor = Some(nvim_oxi::api::types::WindowAnchor::NorthWest);
                    nvim_config.row = Some(float.position.row as f64);
                    nvim_config.col = Some(float.position.col as f64);
                    nvim_config.width = Some(float.dimensions.width as u32);
                    nvim_config.height = Some(float.dimensions.height as u32);
                    nvim_config.focusable = Some(float.focusable);
                    nvim_config.zindex = Some(float.z_index);
                }
            }

            Ok(nvim_oxi::api::open_win(&buf, false, &nvim_config).into_nvim()?)
        })??;

        let id = NvimWindowId(win.handle());
        self.windows.insert(
            id.0,
            NvimWindowEntry {
                buffer: buffer.cloned(),
                is_floating,
            },
        );

        Ok(id)
    }

    fn close_window(&mut self, id: NvimWindowId) -> eel::Result<()> {
        let dispatcher = self.dispatcher.clone();
        dispatcher
            .dispatch(move || nvim_oxi::api::Window::from(id.0).close(true).into_nvim())??;
        self.windows.remove(&id.0);
        Ok(())
    }

    fn set_current(&mut self, id: NvimWindowId) -> eel::Result<()> {
        let dispatcher = self.dispatcher.clone();
        dispatcher.dispatch(move || {
            let win = nvim_oxi::api::Window::from(id.0);
            nvim_oxi::api::set_current_win(&win).into_nvim()
        })??;
        Ok(())
    }

    fn set_buffer(
        &mut self,
        id: NvimWindowId,
        buffer: Option<&NvimBufferHandle>,
    ) -> eel::Result<()> {
        let Some(buf_handle) = buffer else {
            return Ok(());
        };

        let dispatcher = self.dispatcher.clone();
        let buf_id = buf_handle.id;

        dispatcher.dispatch(move || {
            let mut win = nvim_oxi::api::Window::from(id.0);
            let buf: nvim_oxi::api::Buffer = buf_id.into();
            win.set_buf(&buf).into_nvim()
        })??;

        self.windows
            .get_mut(&id.0)
            .ok_or_else(|| eel::Error::Window(eel::window::Error::InvalidWindow))?
            .buffer = Some(buf_handle.clone());

        Ok(())
    }
}

impl WindowStoreHandle for NvimWindowStoreHandle {
    type WindowId = NvimWindowId;
    type BufferHandle = NvimBufferHandle;
    type ReadLock<'lock>
        = RwLockReadGuard<'lock, NvimWindowStore>
    where
        Self: 'lock;
    type WriteLock<'lock>
        = RwLockWriteGuard<'lock, NvimWindowStore>
    where
        Self: 'lock;

    fn windows_read(&self) -> Self::ReadLock<'_> {
        self.0.read()
    }

    fn windows_write(&self) -> Self::WriteLock<'_> {
        self.0.write()
    }
}

impl WindowEditor for NvimEditor {
    type WindowStoreHandle = NvimWindowStoreHandle;

    fn get_window_store(&self) -> NvimWindowStoreHandle {
        self.window_store.clone()
    }
}
