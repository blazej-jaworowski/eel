use async_trait::async_trait;

use crate::{
    Position, Result,
    buffer::{ReadBuffer, ReadBufferLock, WriteBufferLock},
    cursor::{CursorReadBuffer, CursorWriteBuffer},
    mark::{MarkBufferHandle, MarkReadBuffer, MarkWriteBuffer},
    region::BufferRegionAccess,
};

#[async_trait]
impl<'a, B, Buf, L> CursorReadBuffer for BufferRegionAccess<'a, B, Buf, L>
where
    B: MarkBufferHandle,
    Buf: MarkReadBuffer<MarkId = B::MarkId>,
    Buf: CursorReadBuffer,
    L: ReadBufferLock<ReadBuffer = Buf> + 'a,
{
    async fn get_cursor(&self) -> Result<Position> {
        let pos = self.buffer_lock.get_cursor().await?;

        self.region_position(&pos).await
    }
}

#[async_trait]
impl<'a, B, Buf, L> CursorWriteBuffer for BufferRegionAccess<'a, B, Buf, L>
where
    B: MarkBufferHandle,
    Buf: MarkWriteBuffer<MarkId = B::MarkId>,
    Buf: CursorWriteBuffer,
    L: WriteBufferLock<WriteBuffer = Buf> + 'a,
{
    async fn set_cursor(&mut self, position: &Position) -> Result<()> {
        self.validate_pos(position).await?;

        let pos = self.real_position(position).await?;

        self.buffer_lock.set_cursor(&pos).await
    }
}
