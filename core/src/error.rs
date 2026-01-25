use std::sync::Arc;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Buffer error: {0}")]
    Buffer(#[from] crate::buffer::Error),

    #[error("Platform error: {0}")]
    Platform(Arc<dyn PlatformError>),
}

pub type Result<R> = std::result::Result<R, Error>;

pub trait PlatformError
where
    Self: std::error::Error + Send + Sync + 'static,
{
}

impl<P: PlatformError> From<P> for Error {
    fn from(value: P) -> Self {
        Error::Platform(Arc::new(value))
    }
}
