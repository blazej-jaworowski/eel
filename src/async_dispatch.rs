use std::{sync::mpsc, thread::ThreadId};

use futures::{TryFutureExt, future::Either};
use tokio::sync::oneshot;
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
    ResultRecv(#[from] oneshot::error::RecvError),

    #[error("Tokio task join error: {0}")]
    TaskJoin(#[from] tokio::task::JoinError),
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

        let async_handle = AsyncHandle::new(move || {
            trace!("Async handle called");

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
        })
        .map_err(|e| NvimError::from(Error::from(e)))?;

        Ok(Dispatcher {
            nvim_thread_id,
            async_handle,
            func_tx: tx,
        })
    }

    fn inner_dispatch<F, R>(&self, func: F) -> impl Future<Output = std::result::Result<R, Error>>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        if std::thread::current().id() == self.nvim_thread_id {
            trace!("Dispatch called from nvim thread");

            return Either::Left(std::future::ready(Ok(func())));
        }

        let (result_tx, result_rx) = oneshot::channel::<R>();

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
            return Either::Left(std::future::ready(Err(Error::FuncSend)));
        }

        trace!("Calling async handle");

        if let Err(e) = self.async_handle.send() {
            return Either::Left(std::future::ready(Err(e.into())));
        }

        Either::Right(async {
            trace!("Awaiting result");

            let result = result_rx.await?;

            trace!("Result received");

            Ok::<_, Error>(result)
        })
    }

    pub fn dispatch<F, R>(&self, func: F) -> impl Future<Output = Result<R>>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        self.inner_dispatch(func)
            .map_err(|e| EelError::from(NvimError::from(e)))
    }
}
