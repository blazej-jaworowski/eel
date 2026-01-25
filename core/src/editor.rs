use crate::{Result, buffer::BufferHandle};

pub trait Editor: Sized + Sync + Send + 'static {
    type BufferHandle: BufferHandle;

    fn current_buffer(&self) -> Result<Self::BufferHandle>;
    fn new_buffer(&self) -> Result<Self::BufferHandle>;
    fn set_current_buffer(
        &self,
        buffer: &mut <Self::BufferHandle as BufferHandle>::WriteBuffer,
    ) -> Result<()>;
}
