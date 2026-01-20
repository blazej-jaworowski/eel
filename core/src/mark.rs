use std::{marker::PhantomData, sync::Arc};

use async_trait::async_trait;
use tracing::debug;

use crate::{
    Position, Result, async_runtime,
    buffer::{BufferHandle, ReadBuffer, ReadBufferLock, WriteBuffer, WriteBufferLock},
    tracing::ResultExt,
};

pub trait MarkId: std::fmt::Debug + Clone + Copy + Eq + Sync + Send {}

#[derive(Debug, PartialEq, Eq)]
pub enum Gravity {
    Left,
    Right,
}

#[async_trait]
pub trait MarkReadBuffer: ReadBuffer {
    type MarkId: MarkId;

    async fn get_mark_position(&self, id: Self::MarkId) -> Result<Position>;
}

#[async_trait]
pub trait MarkWriteBuffer: MarkReadBuffer + WriteBuffer {
    async fn create_mark(&mut self, pos: &Position) -> Result<Self::MarkId>;
    async fn destroy_mark(&mut self, id: Self::MarkId) -> Result<()>;

    async fn set_mark_position(&mut self, id: Self::MarkId, pos: &Position) -> Result<()>;
    async fn set_mark_gravity(&mut self, id: Self::MarkId, gravity: Gravity) -> Result<()>;
}

pub trait MarkBufferHandle:
    BufferHandle<ReadBuffer = Self::MReadBuffer, WriteBuffer = Self::MWriteBuffer>
{
    type MarkId: MarkId;
    type MReadBuffer: MarkReadBuffer<MarkId = Self::MarkId>;
    type MWriteBuffer: MarkWriteBuffer<MarkId = Self::MarkId>;
}

impl<B, I> MarkBufferHandle for B
where
    B: BufferHandle,
    I: MarkId,
    B::ReadBuffer: MarkReadBuffer<MarkId = I>,
    B::WriteBuffer: MarkWriteBuffer<MarkId = I>,
{
    type MarkId = I;
    type MReadBuffer = B::ReadBuffer;
    type MWriteBuffer = B::WriteBuffer;
}

#[derive(Debug)]
pub struct MarkAccess<'a, L>
where
    L: ReadBufferLock + 'a,
    L::ReadBuffer: MarkReadBuffer,
{
    id: <L::ReadBuffer as MarkReadBuffer>::MarkId,
    buffer_lock: L,
    _marker: PhantomData<&'a ()>,
}

impl<'a, L> MarkAccess<'a, L>
where
    L: ReadBufferLock + 'a,
    L::ReadBuffer: MarkReadBuffer,
{
    pub async fn get_position(&self) -> Result<Position> {
        self.buffer_lock.get_mark_position(self.id).await
    }
}

impl<'a, L> MarkAccess<'a, L>
where
    L: WriteBufferLock + 'a,
    L::WriteBuffer: MarkWriteBuffer,
{
    pub async fn set_position(&mut self, position: &Position) -> Result<()> {
        self.buffer_lock.set_mark_position(self.id, position).await
    }

    pub async fn set_gravity(&mut self, gravity: Gravity) -> Result<()> {
        self.buffer_lock.set_mark_gravity(self.id, gravity).await
    }
}

struct InnerMark<B: MarkBufferHandle> {
    id: B::MarkId,
    buffer: B,
}

impl<B: MarkBufferHandle> Eq for InnerMark<B> {}

impl<B: MarkBufferHandle> PartialEq for InnerMark<B> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.buffer == other.buffer
    }
}

impl<B: MarkBufferHandle> std::fmt::Debug for InnerMark<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InnerMark").field("id", &self.id).finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark<B: MarkBufferHandle> {
    inner: Arc<InnerMark<B>>,
}

impl<B: MarkBufferHandle> Mark<B> {
    pub async fn new<Buf, L>(buffer: &B, position: &Position, mut buffer_lock: L) -> Result<Self>
    where
        Buf: MarkWriteBuffer<MarkId = B::MarkId>,
        L: WriteBufferLock<WriteBuffer = Buf>,
    {
        // TODO: We should find a way to verify if we have a lock to the right buffer.
        //       The same applies to below methods.
        let id = buffer_lock.create_mark(position).await?;

        Ok(Self {
            inner: Arc::new(InnerMark {
                id,
                buffer: buffer.clone(),
            }),
        })
    }

    pub async fn lock_new(buffer: &B, position: &Position) -> Result<Self> {
        let lock = buffer.write().await;
        Self::new(buffer, position, lock).await
    }

    pub fn read<'a, Buf, L>(&self, buffer_lock: L) -> MarkAccess<'a, L>
    where
        Buf: MarkReadBuffer<MarkId = B::MarkId>,
        L: ReadBufferLock<ReadBuffer = Buf> + 'a,
    {
        MarkAccess {
            id: self.inner.id,
            buffer_lock,
            _marker: Default::default(),
        }
    }

    pub async fn lock_read(
        &self,
    ) -> MarkAccess<'static, impl ReadBufferLock<ReadBuffer = B::ReadBuffer> + 'static> {
        let lock = self.inner.buffer.read().await;

        MarkAccess {
            id: self.inner.id,
            buffer_lock: lock,
            _marker: Default::default(),
        }
    }

    pub fn write<'a, Buf, L>(&self, buffer_lock: L) -> MarkAccess<'a, L>
    where
        Buf: MarkWriteBuffer<MarkId = B::MarkId>,
        L: WriteBufferLock<WriteBuffer = Buf> + 'a,
    {
        MarkAccess {
            id: self.inner.id,
            buffer_lock,
            _marker: Default::default(),
        }
    }

    pub async fn lock_write(
        &self,
    ) -> MarkAccess<'static, impl WriteBufferLock<WriteBuffer = B::WriteBuffer> + 'static> {
        let lock = self.inner.buffer.write().await;

        MarkAccess {
            id: self.inner.id,
            buffer_lock: lock,
            _marker: Default::default(),
        }
    }
}

impl<B: MarkBufferHandle> Drop for InnerMark<B> {
    fn drop(&mut self) {
        debug!("Destroying mark ({:?})", self.id);

        let buffer = self.buffer.clone();
        let id = self.id;
        async_runtime::spawn(async move {
            _ = buffer
                .write()
                .await
                .destroy_mark(id)
                .await
                .log_err_msg("Failed to destroy mark");
        });
    }
}

#[cfg(feature = "tests")]
pub mod tests {
    use std::ops::Deref;

    use crate::{Editor, test_utils::new_buffer_with_content};

    use super::*;

    pub async fn test_mark_basic<E>(editor: E)
    where
        E: Editor,
        E::BufferHandle: MarkBufferHandle,
    {
        let buffer = new_buffer_with_content(&editor, "test\ntest2").await;

        let mark = Mark::lock_new(&buffer, &Position::new(0, 1))
            .await
            .expect("Failed to create mark");

        let position = mark
            .lock_read()
            .await
            .get_position()
            .await
            .expect("Failed to get position");

        assert_eq!(position, Position::new(0, 1));

        mark.lock_write()
            .await
            .set_position(&Position::new(1, 0))
            .await
            .expect("Failed to set position");

        let position = mark
            .lock_read()
            .await
            .get_position()
            .await
            .expect("Failed to get position");

        assert_eq!(position, Position::new(1, 0));
    }

    pub async fn test_mark_set_text<E>(editor: E)
    where
        E: Editor,
        E::BufferHandle: MarkBufferHandle,
    {
        let buffer = new_buffer_with_content(&editor, "First line").await;
        let mut buffer_lock = buffer.write().await;

        let mark = Mark::new(&buffer, &Position::new(0, 6), &mut *buffer_lock)
            .await
            .expect("Failed to create mark");

        buffer_lock
            .set_text(
                &Position::new(0, 6),
                &Position::new(0, 6),
                "(actually) line\nSecond ",
            )
            .await
            .expect("Failed to set text");

        let position = mark
            .read(&*buffer_lock)
            .get_position()
            .await
            .expect("Failed to get position");

        assert_eq!(position, Position::new(1, 7));
    }

    pub async fn test_mark_gravity_right<E>(editor: E)
    where
        E: Editor,
        E::BufferHandle: MarkBufferHandle,
    {
        let buffer = new_buffer_with_content(&editor, "First line").await;
        let mut buffer_lock = buffer.write().await;

        let mark = Mark::new(&buffer, &Position::new(0, 5), &mut *buffer_lock)
            .await
            .expect("Failed to create mark");

        assert_eq!(
            mark.read(buffer_lock.deref())
                .get_position()
                .await
                .expect("Failed to get mark position"),
            Position::new(0, 5),
        );

        buffer_lock
            .set_text(&Position::new(0, 1), &Position::new(0, 9), "ir")
            .await
            .expect("Failed to set text");

        assert_eq!(
            mark.read(&mut *buffer_lock)
                .get_position()
                .await
                .expect("Failed to get mark position"),
            Position::new(0, 3),
        );

        buffer_lock
            .set_text(&Position::new(0, 3), &Position::new(0, 3), "...")
            .await
            .expect("Failed to set text");

        assert_eq!(
            mark.read(buffer_lock)
                .get_position()
                .await
                .expect("Failed to get mark position"),
            Position::new(0, 6),
        );
    }

    pub async fn test_mark_gravity_left<E>(editor: E)
    where
        E: Editor,
        E::BufferHandle: MarkBufferHandle,
    {
        let buffer = new_buffer_with_content(&editor, "First line").await;
        let mut buffer_lock = buffer.write().await;

        let mark = Mark::new(&buffer, &Position::new(0, 5), &mut *buffer_lock)
            .await
            .expect("Failed to create mark");

        mark.write(&mut *buffer_lock)
            .set_gravity(Gravity::Left)
            .await
            .expect("Failed to set gravity");

        assert_eq!(
            mark.write(&mut *buffer_lock)
                .get_position()
                .await
                .expect("Failed to get mark position"),
            Position::new(0, 5),
        );

        buffer_lock
            .set_text(&Position::new(0, 1), &Position::new(0, 9), "ir")
            .await
            .expect("Failed to set text");

        assert_eq!(
            mark.read(&mut *buffer_lock)
                .get_position()
                .await
                .expect("Failed to get mark position"),
            Position::new(0, 1),
        );

        buffer_lock
            .set_text(&Position::new(0, 1), &Position::new(0, 3), "...")
            .await
            .expect("Failed to set text");

        assert_eq!(
            mark.read(buffer_lock)
                .get_position()
                .await
                .expect("Failed to get mark position"),
            Position::new(0, 1),
        );
    }

    #[macro_export]
    macro_rules! eel_mark_tests {
        ($test_tag:path, $editor_factory:expr, $prefix:literal) => {
            $crate::eel_tests!(
                test_tag: $test_tag,
                editor_factory: $editor_factory,
                editor_bounds: { E::BufferHandle: $crate::mark::MarkBufferHandle },
                module_path: $crate::mark::tests,
                prefix: $prefix,
                tests: [
                    test_mark_basic,
                    test_mark_set_text,
                    test_mark_gravity_right,
                    test_mark_gravity_left,
                ],
            );
        };

        ($test_tag:path, $editor_factory:expr) => {
            $crate::eel_mark_tests!($test_tag, $editor_factory, "");
        };
    }
}
