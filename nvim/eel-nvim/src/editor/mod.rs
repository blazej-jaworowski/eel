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
mod window_editor;
