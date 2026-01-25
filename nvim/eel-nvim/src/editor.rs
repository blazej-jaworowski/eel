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
    buffers: RwLock<HashMap<i32, NvimBufferHandle>>,
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

        if let Some(h) = self.buffers.read().get(&key) {
            trace!("Buffer handle exists already");
            return h.clone();
        }

        self.buffers
            .write()
            .entry(key)
            .or_insert_with(|| {
                trace!("Creating new buffer handle");
                NvimBufferHandle::new(NvimBuffer::new(buffer, self.dispatcher.clone()))
            })
            .clone()
    }
}

#[derive(Debug)]
pub struct NvimEditor {
    buffer_store: BufferStore,
    dispatcher: Arc<Dispatcher>,
}

impl NvimEditor {
    pub fn new(nvim_thread_id: ThreadId) -> Result<Self> {
        let dispatcher = Arc::new(Dispatcher::new(nvim_thread_id)?);

        Ok(NvimEditor {
            buffer_store: BufferStore::new(dispatcher.clone()),
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

    fn set_current_buffer(
        &self,
        buffer: &mut <Self::BufferHandle as BufferHandle>::WriteBuffer,
    ) -> Result<()> {
        let buf = buffer.inner_buf();

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
}

#[allow(unused)]
pub(crate) fn get_eel_namespace() -> u32 {
    nvim_oxi::api::create_namespace("eel")
}
