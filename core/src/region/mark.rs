use async_trait::async_trait;

use crate::{
    Position, Result,
    buffer::{ReadBufferLock, WriteBufferLock},
    mark::{Gravity, MarkBufferHandle, MarkReadBuffer, MarkWriteBuffer},
    region::BufferRegionAccess,
};

#[async_trait]
impl<'a, B, Buf, L> MarkReadBuffer for BufferRegionAccess<'a, B, Buf, L>
where
    B: MarkBufferHandle,
    Buf: MarkReadBuffer<MarkId = B::MarkId>,
    L: ReadBufferLock<ReadBuffer = Buf> + 'a,
{
    type MarkId = B::MarkId;

    async fn get_mark_position(&self, id: Self::MarkId) -> Result<Position> {
        let pos = self.buffer_lock.get_mark_position(id).await?;

        self.region_position(&pos).await
    }
}

#[async_trait]
impl<'a, B, Buf, L> MarkWriteBuffer for BufferRegionAccess<'a, B, Buf, L>
where
    B: MarkBufferHandle,
    Buf: MarkWriteBuffer<MarkId = B::MarkId>,
    L: WriteBufferLock<WriteBuffer = Buf> + 'a,
{
    async fn create_mark(&mut self, pos: &Position) -> Result<Self::MarkId> {
        let pos = self.real_position(pos).await?;

        self.buffer_lock.create_mark(&pos).await
    }

    async fn destroy_mark(&mut self, id: Self::MarkId) -> Result<()> {
        self.buffer_lock.destroy_mark(id).await
    }

    async fn set_mark_position(&mut self, id: Self::MarkId, pos: &Position) -> Result<()> {
        let pos = self.real_position(pos).await?;

        self.buffer_lock.set_mark_position(id, &pos).await
    }

    async fn set_mark_gravity(&mut self, id: Self::MarkId, gravity: Gravity) -> Result<()> {
        self.buffer_lock.set_mark_gravity(id, gravity).await
    }
}
