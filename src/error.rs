use eel::error::PlatformError;

use crate::async_dispatch;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Nvim API error: {0}")]
    Api(#[from] nvim_oxi::api::Error),

    #[error("Nvim Lua error: {0}")]
    Lua(#[from] nvim_oxi::lua::Error),

    #[error("Nvim MLua error: {0}")]
    MLua(String),

    #[error("Async dispatch error: {0}")]
    AsyncDispatch(#[from] async_dispatch::Error),
}

impl From<nvim_oxi::mlua::Error> for Error {
    fn from(value: nvim_oxi::mlua::Error) -> Self {
        Self::MLua(value.to_string())
    }
}

pub trait IntoNvimResult<T> {
    fn into_nvim(self) -> std::result::Result<T, Error>;
}

impl<T, E> IntoNvimResult<T> for std::result::Result<T, E>
where
    Error: From<E>,
{
    fn into_nvim(self) -> std::result::Result<T, Error> {
        self.map_err(Error::from)
    }
}

impl PlatformError for Error {}
