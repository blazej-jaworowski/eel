use std::{collections::HashMap, sync::Arc, thread::ThreadId};

use parking_lot::RwLock;
use tracing::trace;

use eel::{Editor, Result, buffer::BufferHandle};

use crate::{
    buffer::{NvimBuffer, NvimBufferHandle},
    dispatcher::Dispatcher,
    error::{Error as NvimError, IntoNvimResult},
};

#[derive(Debug)]
struct BufferStore {
    buffers: RwLock<HashMap<i32, Arc<RwLock<NvimBuffer>>>>,
    dispatcher: Arc<Dispatcher>,
}

impl BufferStore {
    fn new(dispatcher: Arc<Dispatcher>) -> Self {
        Self {
            buffers: RwLock::default(),
            dispatcher,
        }
    }
}

impl BufferStore {
    fn get_buffer_handle(&self, buffer: nvim_oxi::api::Buffer) -> NvimBufferHandle {
        let key = buffer.handle();

        if let Some(arc) = self.buffers.read().get(&key) {
            trace!("Buffer handle exists already");
            return NvimBufferHandle::new(key, arc);
        }

        let mut buffers = self.buffers.write();
        let arc = buffers.entry(key).or_insert_with(|| {
            trace!("Creating new buffer handle");
            Arc::new(RwLock::new(NvimBuffer::new(
                buffer,
                self.dispatcher.clone(),
            )))
        });

        NvimBufferHandle::new(key, arc)
    }

    fn remove_buffer(&self, id: i32) {
        self.buffers.write().remove(&id);
    }
}

#[derive(Debug)]
pub struct NvimEditor {
    buffer_store: BufferStore,
    dispatcher: Arc<Dispatcher>,
    #[cfg(feature = "window")]
    window_store: window_editor::NvimWindowStoreHandle,
}

impl NvimEditor {
    pub fn new(nvim_thread_id: ThreadId) -> Result<Self> {
        let dispatcher = Arc::new(Dispatcher::new(nvim_thread_id)?);

        Ok(NvimEditor {
            buffer_store: BufferStore::new(dispatcher.clone()),
            #[cfg(feature = "window")]
            window_store: window_editor::NvimWindowStoreHandle::new(dispatcher.clone()),
            dispatcher,
        })
    }

    pub fn new_on_current() -> Result<Self> {
        Self::new(std::thread::current().id())
    }

    pub fn dispatch<F, R>(&self, func: F) -> Result<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        self.dispatcher.dispatch(func)
    }
}

impl Editor for NvimEditor {
    type BufferHandle = NvimBufferHandle;

    fn current_buffer(&self) -> Result<NvimBufferHandle> {
        let buf = self.dispatch(nvim_oxi::api::get_current_buf)?;

        Ok(self.buffer_store.get_buffer_handle(buf))
    }

    fn set_current_buffer(&self, buffer: &Self::BufferHandle) -> Result<()> {
        let buf = buffer.read()?.inner_buf();

        Ok(self.dispatch(move || nvim_oxi::api::set_current_buf(&buf).into_nvim())??)
    }

    fn new_buffer(&self) -> Result<NvimBufferHandle> {
        let buf = self.dispatch(|| {
            let buf = nvim_oxi::api::create_buf(true, true)?;
            let opts = nvim_oxi::api::opts::OptionOpts::builder()
                .buffer(buf.clone())
                .build();

            nvim_oxi::api::set_option_value("buftype", "nofile", &opts)?;
            nvim_oxi::api::set_option_value("bufhidden", "hide", &opts)?;
            nvim_oxi::api::set_option_value("swapfile", false, &opts)?;

            Ok::<_, NvimError>(buf)
        })??;

        Ok(self.buffer_store.get_buffer_handle(buf))
    }

    fn kill_buffer(&self, buffer: &NvimBufferHandle) -> Result<()> {
        let id = buffer.id;

        self.dispatch(move || {
            let buf: nvim_oxi::api::Buffer = id.into();
            buf.delete(
                &nvim_oxi::api::opts::BufDeleteOpts::builder()
                    .force(true)
                    .build(),
            )
            .map_err(NvimError::from)
        })??;

        self.buffer_store.remove_buffer(id);

        Ok(())
    }
}

#[allow(unused)]
pub(crate) fn get_eel_namespace() -> u32 {
    nvim_oxi::api::create_namespace("eel")
}

#[cfg(feature = "window")]
mod window_editor {
    use std::{collections::HashMap, sync::Arc};

    use eel::window::{ReadWindowLock, WindowEditor, WindowId, WindowStoreHandle, WriteWindowLock};
    use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};

    use crate::{
        buffer::NvimBufferHandle,
        dispatcher::Dispatcher,
        editor::NvimEditor,
        error::{Error as NvimError, IntoNvimResult},
    };

    struct NvimWindowEntry {
        buffer: Option<NvimBufferHandle>,
    }

    struct NvimWindowStore {
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

    pub struct NvimReadWindowLock<'lock>(RwLockReadGuard<'lock, NvimWindowStore>);
    pub struct NvimWriteWindowLock<'lock>(RwLockWriteGuard<'lock, NvimWindowStore>);

    impl ReadWindowLock for NvimReadWindowLock<'_> {
        type WindowId = NvimWindowId;
        type BufferHandle = NvimBufferHandle;

        fn current_window_id(&self) -> eel::Result<NvimWindowId> {
            let dispatcher = self.0.dispatcher.clone();
            let win = dispatcher.dispatch(nvim_oxi::api::get_current_win)?;
            Ok(NvimWindowId(win.handle()))
        }

        fn get_buffer(&self, id: NvimWindowId) -> eel::Result<Option<NvimBufferHandle>> {
            Ok(self.0.windows.get(&id.0).and_then(|e| e.buffer.clone()))
        }

        fn get_width(&self, id: NvimWindowId) -> eel::Result<usize> {
            let dispatcher = self.0.dispatcher.clone();
            let width = dispatcher
                .dispatch(move || nvim_oxi::api::Window::from(id.0).get_width().into_nvim())??;
            Ok(width as usize)
        }

        fn get_height(&self, id: NvimWindowId) -> eel::Result<usize> {
            let dispatcher = self.0.dispatcher.clone();
            let height = dispatcher
                .dispatch(move || nvim_oxi::api::Window::from(id.0).get_height().into_nvim())??;
            Ok(height as usize)
        }
    }

    impl ReadWindowLock for NvimWriteWindowLock<'_> {
        type WindowId = NvimWindowId;
        type BufferHandle = NvimBufferHandle;

        fn current_window_id(&self) -> eel::Result<NvimWindowId> {
            let dispatcher = self.0.dispatcher.clone();
            let win = dispatcher.dispatch(nvim_oxi::api::get_current_win)?;
            Ok(NvimWindowId(win.handle()))
        }

        fn get_buffer(&self, id: NvimWindowId) -> eel::Result<Option<NvimBufferHandle>> {
            Ok(self.0.windows.get(&id.0).and_then(|e| e.buffer.clone()))
        }

        fn get_width(&self, id: NvimWindowId) -> eel::Result<usize> {
            let dispatcher = self.0.dispatcher.clone();
            let width = dispatcher
                .dispatch(move || nvim_oxi::api::Window::from(id.0).get_width().into_nvim())??;
            Ok(width as usize)
        }

        fn get_height(&self, id: NvimWindowId) -> eel::Result<usize> {
            let dispatcher = self.0.dispatcher.clone();
            let height = dispatcher
                .dispatch(move || nvim_oxi::api::Window::from(id.0).get_height().into_nvim())??;
            Ok(height as usize)
        }
    }

    impl WriteWindowLock for NvimWriteWindowLock<'_> {
        fn new_window(&mut self, buffer: Option<&NvimBufferHandle>) -> eel::Result<NvimWindowId> {
            let dispatcher = self.0.dispatcher.clone();
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
            self.0.windows.insert(
                id.0,
                NvimWindowEntry {
                    buffer: buffer.cloned(),
                },
            );

            Ok(id)
        }

        fn close_window(&mut self, id: NvimWindowId) -> eel::Result<()> {
            let dispatcher = self.0.dispatcher.clone();
            dispatcher
                .dispatch(move || nvim_oxi::api::Window::from(id.0).close(true).into_nvim())??;
            self.0.windows.remove(&id.0);
            Ok(())
        }

        fn set_current(&mut self, id: NvimWindowId) -> eel::Result<()> {
            let dispatcher = self.0.dispatcher.clone();
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

            let dispatcher = self.0.dispatcher.clone();
            let buf_id = buf_handle.id;

            dispatcher.dispatch(move || {
                let mut win = nvim_oxi::api::Window::from(id.0);
                let buf: nvim_oxi::api::Buffer = buf_id.into();
                win.set_buf(&buf).into_nvim()
            })??;

            self.0
                .windows
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
            = NvimReadWindowLock<'lock>
        where
            Self: 'lock;
        type WriteLock<'lock>
            = NvimWriteWindowLock<'lock>
        where
            Self: 'lock;

        fn windows_read(&self) -> NvimReadWindowLock<'_> {
            NvimReadWindowLock(self.0.read())
        }

        fn windows_write(&self) -> NvimWriteWindowLock<'_> {
            NvimWriteWindowLock(self.0.write())
        }
    }

    impl WindowEditor for NvimEditor {
        type WindowStoreHandle = NvimWindowStoreHandle;

        fn get_window_store(&self) -> NvimWindowStoreHandle {
            self.window_store.clone()
        }
    }
}
