use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use async_trait::async_trait;
use tracing::debug;

use crate::{
    Position, Result, async_runtime,
    buffer::{Buffer, BufferHandle, BufferReadLock, BufferWriteLock},
    tracing::ResultExt,
};

pub trait MarkId: std::fmt::Debug + Clone + Copy + Sync + Send {}

#[derive(Debug, PartialEq, Eq)]
pub enum Gravity {
    Left,
    Right,
}

#[async_trait]
pub trait MarksBuffer: Buffer {
    type MarkId: MarkId;

    async fn create_mark(&mut self, pos: &Position) -> Result<Self::MarkId>;
    async fn destroy_mark(&mut self, id: Self::MarkId) -> Result<()>;

    async fn get_mark_position(&self, id: Self::MarkId) -> Result<Position>;
    async fn set_mark_position(&mut self, id: Self::MarkId, pos: &Position) -> Result<()>;

    async fn set_mark_gravity(&mut self, id: Self::MarkId, gravity: Gravity) -> Result<()>;
}

#[derive(Debug)]
pub struct Mark<B>
where
    B: BufferHandle,
    B::Buffer: MarksBuffer,
{
    id: <B::Buffer as MarksBuffer>::MarkId,
    buffer: B,
    ref_count: Arc<AtomicU64>,
}

impl<B> Clone for Mark<B>
where
    B: BufferHandle,
    B::Buffer: MarksBuffer,
{
    fn clone(&self) -> Self {
        let prev_count = self.ref_count.fetch_add(1, Ordering::Relaxed);

        debug!({ ref_count = prev_count }, "Cloning mark ({:?})", self.id);

        Self {
            id: self.id,
            buffer: self.buffer.clone(),
            ref_count: self.ref_count.clone(),
        }
    }
}

impl<B> Drop for Mark<B>
where
    B: BufferHandle,
    B::Buffer: MarksBuffer,
{
    fn drop(&mut self) {
        let prev_count = self.ref_count.fetch_sub(1, Ordering::Relaxed);

        debug!({ ref_count = prev_count }, "Dropping mark ({:?})", self.id);

        if prev_count == 1 {
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
}

impl<B> Mark<B>
where
    B: BufferHandle,
    B::Buffer: MarksBuffer,
{
    pub async fn new(buffer: &B, position: &Position) -> Result<Self> {
        let mut lock = buffer.write().await;
        Self::new_locked(buffer, position, &mut lock).await
    }

    pub async fn new_locked(
        buffer: &B,
        position: &Position,
        buffer_lock: &mut impl BufferWriteLock<B::Buffer>,
    ) -> Result<Self> {
        // TODO: We should find a way to verify if we have a lock to the right buffer.
        //       The same applies to below methods.
        let id = buffer_lock.create_mark(position).await?;

        Ok(Self {
            id,
            buffer: buffer.clone(),
            ref_count: Arc::new(AtomicU64::new(1)),
        })
    }

    pub async fn get_position(&self) -> Result<Position> {
        let lock = self.buffer.read().await;
        self.get_position_locked(&lock).await
    }

    pub async fn get_position_locked(
        &self,
        buffer_lock: &impl BufferReadLock<B::Buffer>,
    ) -> Result<Position> {
        buffer_lock.get_mark_position(self.id).await
    }

    pub async fn set_position(&self, position: &Position) -> Result<()> {
        let mut lock = self.buffer.write().await;
        self.set_position_locked(position, &mut lock).await
    }

    pub async fn set_position_locked(
        &self,
        position: &Position,
        buffer_lock: &mut impl BufferWriteLock<B::Buffer>,
    ) -> Result<()> {
        buffer_lock.set_mark_position(self.id, position).await
    }

    pub async fn set_gravity(&self, gravity: Gravity) -> Result<()> {
        let mut lock = self.buffer.write().await;
        self.set_gravity_locked(gravity, &mut lock).await
    }
    pub async fn set_gravity_locked(
        &self,
        gravity: Gravity,
        buffer_lock: &mut impl BufferWriteLock<B::Buffer>,
    ) -> Result<()> {
        buffer_lock.set_mark_gravity(self.id, gravity).await
    }

    pub fn get_buffer(&self) -> &B {
        &self.buffer
    }
}

#[cfg(feature = "tests")]
pub mod tests {
    use crate::{Editor, test_utils::new_buffer_with_content};

    use super::*;

    pub async fn _test_marks_basic<E>(editor: E)
    where
        E: Editor,
        E::Buffer: MarksBuffer,
    {
        let buffer = new_buffer_with_content(&editor, "test\ntest2").await;

        let mark = Mark::new(&buffer, &Position::new(0, 1))
            .await
            .expect("Failed to create mark");

        let position = mark.get_position().await.expect("Failed to get position");

        assert_eq!(position, Position::new(0, 1));

        mark.set_position(&Position::new(1, 0))
            .await
            .expect("Failed to set position");

        let position = mark.get_position().await.expect("Failed to get position");

        assert_eq!(position, Position::new(1, 0));
    }

    pub async fn _test_marks_set_text<E>(editor: E)
    where
        E: Editor,
        E::Buffer: MarksBuffer,
    {
        let buffer = new_buffer_with_content(&editor, "First line").await;

        let mark = Mark::new(&buffer, &Position::new(0, 6))
            .await
            .expect("Failed to create mark");

        buffer
            .write()
            .await
            .set_text(
                &Position::new(0, 6),
                &Position::new(0, 6),
                "(actually) line\nSecond ",
            )
            .await
            .expect("Failed to set text");

        let position = mark.get_position().await.expect("Failed to get position");

        assert_eq!(position, Position::new(1, 7));
    }

    pub async fn _test_marks_gravity_right<E>(editor: E)
    where
        E: Editor,
        E::Buffer: MarksBuffer,
    {
        let buffer = new_buffer_with_content(&editor, "First line").await;

        let mark = Mark::new(&buffer, &Position::new(0, 5))
            .await
            .expect("Failed to create mark");

        assert_eq!(
            mark.get_position()
                .await
                .expect("Failed to get mark position"),
            Position::new(0, 5),
        );

        buffer
            .write()
            .await
            .set_text(&Position::new(0, 1), &Position::new(0, 9), "ir")
            .await
            .expect("Failed to set text");

        assert_eq!(
            mark.get_position()
                .await
                .expect("Failed to get mark position"),
            Position::new(0, 3),
        );

        buffer
            .write()
            .await
            .set_text(&Position::new(0, 3), &Position::new(0, 3), "...")
            .await
            .expect("Failed to set text");

        assert_eq!(
            mark.get_position()
                .await
                .expect("Failed to get mark position"),
            Position::new(0, 6),
        );
    }

    pub async fn _test_marks_gravity_left<E>(editor: E)
    where
        E: Editor,
        E::Buffer: MarksBuffer,
    {
        let buffer = new_buffer_with_content(&editor, "First line").await;

        let mark = Mark::new(&buffer, &Position::new(0, 5))
            .await
            .expect("Failed to create mark");

        mark.set_gravity(Gravity::Left)
            .await
            .expect("Failed to set gravity");

        assert_eq!(
            mark.get_position()
                .await
                .expect("Failed to get mark position"),
            Position::new(0, 5),
        );

        buffer
            .write()
            .await
            .set_text(&Position::new(0, 1), &Position::new(0, 9), "ir")
            .await
            .expect("Failed to set text");

        assert_eq!(
            mark.get_position()
                .await
                .expect("Failed to get mark position"),
            Position::new(0, 1),
        );

        buffer
            .write()
            .await
            .set_text(&Position::new(0, 1), &Position::new(0, 3), "...")
            .await
            .expect("Failed to set text");

        assert_eq!(
            mark.get_position()
                .await
                .expect("Failed to get mark position"),
            Position::new(0, 1),
        );
    }

    // TODO: Test reference counting and cleanup.

    #[macro_export]
    macro_rules! eel_marks_tests {
        (@test $test_name:ident, $test_tag:meta) => {
            $crate::test_utils::paste! {
                #[$test_tag]
                async fn $test_name<E>(editor: E)
                where
                    E: $crate::Editor,
                    E::Buffer: $crate::marks::MarksBuffer,
                {
                    $crate::marks::tests::[< _ $test_name >](editor).await;
                }
            }
        };

        ($test_tag:meta) => {
            $crate::eel_marks_tests!(@test test_marks_basic, $test_tag);
            $crate::eel_marks_tests!(@test test_marks_set_text, $test_tag);
            $crate::eel_marks_tests!(@test test_marks_gravity_right, $test_tag);
            $crate::eel_marks_tests!(@test test_marks_gravity_left, $test_tag);
        };
    }
}
