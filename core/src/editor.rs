use async_trait::async_trait;

use crate::{Result, buffer::BufferHandle};

#[async_trait]
pub trait Editor: Sized + Sync + Send + 'static {
    type BufferHandle: BufferHandle;

    async fn current_buffer(&self) -> Result<Self::BufferHandle>;
    async fn new_buffer(&self) -> Result<Self::BufferHandle>;
    async fn set_current_buffer(
        &self,
        buffer: &mut <Self::BufferHandle as BufferHandle>::WriteBuffer,
    ) -> Result<()>;
}
