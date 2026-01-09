use crate::{
    Result,
    buffer::{Buffer, BufferHandle},
};

#[async_trait::async_trait]
pub trait Editor: Sized + Sync + Send {
    type Buffer: Buffer;
    type BufferHandle: BufferHandle<Buffer = Self::Buffer>;

    async fn current_buffer(&self) -> Result<Self::BufferHandle>;
    async fn new_buffer(&self) -> Result<Self::BufferHandle>;
    async fn set_current_buffer(&self, buffer: &mut Self::Buffer) -> Result<()>;
}
