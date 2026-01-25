use crate::{
    Position, Result,
    buffer::{ReadBuffer, ReadBufferLock, WriteBufferLock},
    cursor::{CursorReadBuffer, CursorWriteBuffer},
    mark::{MarkBufferHandle, MarkReadBuffer, MarkWriteBuffer},
    region::BufferRegionAccess,
};

impl<'a, B, Buf, L> CursorReadBuffer for BufferRegionAccess<'a, B, Buf, L>
where
    B: MarkBufferHandle,
    Buf: MarkReadBuffer<MarkId = B::MarkId>,
    Buf: CursorReadBuffer,
    L: ReadBufferLock<ReadBuffer = Buf> + 'a,
{
    fn get_cursor(&self) -> Result<Position> {
        let pos = self.buffer_lock.get_cursor()?;

        self.region_position(&pos)
    }
}

impl<'a, B, Buf, L> CursorWriteBuffer for BufferRegionAccess<'a, B, Buf, L>
where
    B: MarkBufferHandle,
    Buf: MarkWriteBuffer<MarkId = B::MarkId>,
    Buf: CursorWriteBuffer,
    L: WriteBufferLock<WriteBuffer = Buf> + 'a,
{
    fn set_cursor(&mut self, position: &Position) -> Result<()> {
        self.validate_pos(position)?;

        let pos = self.real_position(position)?;

        self.buffer_lock.set_cursor(&pos)
    }
}
