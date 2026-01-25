use crate::{
    Position, Result,
    buffer::{ReadBufferLock, WriteBufferLock},
    mark::{Gravity, MarkBufferHandle, MarkReadBuffer, MarkWriteBuffer},
    region::BufferRegionAccess,
};

impl<'a, B, Buf, L> MarkReadBuffer for BufferRegionAccess<'a, B, Buf, L>
where
    B: MarkBufferHandle,
    Buf: MarkReadBuffer<MarkId = B::MarkId>,
    L: ReadBufferLock<ReadBuffer = Buf> + 'a,
{
    type MarkId = B::MarkId;

    fn get_mark_position(&self, id: Self::MarkId) -> Result<Position> {
        let pos = self.buffer_lock.get_mark_position(id)?;

        self.region_position(&pos)
    }
}

impl<'a, B, Buf, L> MarkWriteBuffer for BufferRegionAccess<'a, B, Buf, L>
where
    B: MarkBufferHandle,
    Buf: MarkWriteBuffer<MarkId = B::MarkId>,
    L: WriteBufferLock<WriteBuffer = Buf> + 'a,
{
    fn create_mark(&mut self, pos: &Position) -> Result<Self::MarkId> {
        let pos = self.real_position(pos)?;

        self.buffer_lock.create_mark(&pos)
    }

    fn destroy_mark(&mut self, id: Self::MarkId) -> Result<()> {
        self.buffer_lock.destroy_mark(id)
    }

    fn set_mark_position(&mut self, id: Self::MarkId, pos: &Position) -> Result<()> {
        let pos = self.real_position(pos)?;

        self.buffer_lock.set_mark_position(id, &pos)
    }

    fn set_mark_gravity(&mut self, id: Self::MarkId, gravity: Gravity) -> Result<()> {
        self.buffer_lock.set_mark_gravity(id, gravity)
    }
}
