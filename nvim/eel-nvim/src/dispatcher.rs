use std::{rc::Rc, sync::mpsc, thread::ThreadId};

use tracing::{error, trace};

use crate::error::Error as NvimError;
use eel::{Error as EelError, Result};

use nvim_oxi::{self, libuv::AsyncHandle};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Nvim LibUV error: {0}")]
    NvimLibUV(#[from] nvim_oxi::libuv::Error),

    #[error("Dispatch function send error")]
    FuncSend,

    #[error("Result receive error: {0}")]
    ResultRecv(#[from] mpsc::RecvError),
}

pub struct Dispatcher {
    nvim_thread_id: ThreadId,
    async_handle: AsyncHandle,
    func_tx: mpsc::Sender<Box<dyn FnOnce() + Send>>,
}

impl std::fmt::Debug for Dispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dispatcher")
            .field("nvim_thread_id", &self.nvim_thread_id)
            .field("func_tx", &self.func_tx)
            .finish()
    }
}

impl Dispatcher {
    pub fn new(nvim_thread_id: ThreadId) -> Result<Dispatcher> {
        let (tx, rx) = mpsc::channel::<Box<dyn FnOnce() + Send>>();

        // In theory this function can be called on a different thread than the inner AsyncHandle
        // function, and Rc is not Send. But we don't clone it and we pass it straight into the
        // AsyncHandle, so using Rc should be fine.
        let rx = Rc::new(rx);

        let async_handle = AsyncHandle::new(move || {
            trace!("Async handle called, scheduling call on the main neovim thread");

            let rx = rx.clone();

            // We have to call vim.schedule because of libuv recursion issues causing crashes.
            nvim_oxi::schedule(move |()| {
                trace!("Dispatched function called on the main neovim thread");

                loop {
                    match rx.try_recv() {
                        Ok(f) => {
                            trace!("Function received by async handle");
                            f();
                        }
                        Err(mpsc::TryRecvError::Empty) => {
                            trace!("Func channel empty");
                            return;
                        }
                        Err(mpsc::TryRecvError::Disconnected) => {
                            error!("Func channel disconnected");
                            return;
                        }
                    }
                }
            });
        })
        .map_err(|e| NvimError::from(Error::from(e)))?;

        Ok(Dispatcher {
            nvim_thread_id,
            async_handle,
            func_tx: tx,
        })
    }

    fn inner_dispatch<F, R>(&self, func: F) -> std::result::Result<R, Error>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        if std::thread::current().id() == self.nvim_thread_id {
            trace!("Dispatch called from nvim thread");

            return Ok(func());
        }

        let (result_tx, result_rx) = mpsc::sync_channel::<R>(1);

        let nvim_tid = self.nvim_thread_id;
        let dispatch_func = Box::new(move || {
            if nvim_tid != std::thread::current().id() {
                error!("Dispatched function called on non-nvim thread");
                return;
            }

            trace!("Calling function on neovim thread");

            let result = func();

            trace!("Sending function result");

            if result_tx.send(result).is_err() {
                error!("Error while sending dispatch result");
            }
        });

        trace!("Sending function to dispatch");

        if self.func_tx.send(dispatch_func).is_err() {
            return Err(Error::FuncSend);
        }

        trace!("Calling async handle");

        if let Err(e) = self.async_handle.send() {
            return Err(e.into());
        }

        trace!("Awaiting result");

        let result = result_rx.recv()?;

        trace!("Result received");

        Ok::<_, Error>(result)
    }

    pub fn dispatch<F, R>(&self, func: F) -> Result<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        self.inner_dispatch(func)
            .map_err(|e| EelError::from(NvimError::from(e)))
    }
}
