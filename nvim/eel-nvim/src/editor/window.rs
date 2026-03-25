use std::{collections::HashMap, sync::Arc};

use eel::window::{ReadWindowLock, WindowEditor, WindowId, WindowStoreHandle, WriteWindowLock};
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::{
    buffer::NvimBufferHandle,
    dispatcher::Dispatcher,
    error::{Error as NvimError, IntoNvimResult},
};

use super::NvimEditor;

struct NvimWindowEntry {
    buffer: Option<NvimBufferHandle>,
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
        Ok(self.windows.get(&id.0).and_then(|e| e.buffer.clone()))
    }

    fn get_width(&self, id: NvimWindowId) -> eel::Result<usize> {
        let dispatcher = self.dispatcher.clone();
        let width = dispatcher
            .dispatch(move || nvim_oxi::api::Window::from(id.0).get_width().into_nvim())??;
        Ok(width as usize)
    }

    fn get_height(&self, id: NvimWindowId) -> eel::Result<usize> {
        let dispatcher = self.dispatcher.clone();
        let height = dispatcher
            .dispatch(move || nvim_oxi::api::Window::from(id.0).get_height().into_nvim())??;
        Ok(height as usize)
    }
}

impl WriteWindowLock for NvimWindowStore {
    fn new_window(&mut self, buffer: Option<&NvimBufferHandle>) -> eel::Result<NvimWindowId> {
        let dispatcher = self.dispatcher.clone();
        let buf_id: Option<i32> = buffer.map(|h| h.id);

        let win = dispatcher.dispatch(
            move || -> std::result::Result<nvim_oxi::api::Window, NvimError> {
                let buf: nvim_oxi::api::Buffer = if let Some(id) = buf_id {
                    id.into()
                } else {
                    nvim_oxi::api::create_buf(false, true).into_nvim()?
                };

                let config = nvim_oxi::api::types::WindowConfig::builder()
                    .split(nvim_oxi::api::types::SplitDirection::Below)
                    .build();

                nvim_oxi::api::open_win(&buf, false, &config).into_nvim()
            },
        )??;

        let id = NvimWindowId(win.handle());
        self.windows.insert(
            id.0,
            NvimWindowEntry {
                buffer: buffer.cloned(),
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
            .entry(id.0)
            .and_modify(|e| e.buffer = Some(buf_handle.clone()))
            .or_insert_with(|| NvimWindowEntry {
                buffer: Some(buf_handle.clone()),
            });

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
