use async_trait::async_trait;

use crate::{
    Position, Result,
    buffer::{Buffer, BufferHandle},
};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Mark destroyed")]
    Destroyed,
}

pub trait MarkId: std::fmt::Debug + Clone + Copy + Sync + Send {}

#[async_trait]
pub trait MarksBuffer: Buffer {
    type MarkId: MarkId;

    async fn create_mark(&mut self, pos: &Position) -> Result<Self::MarkId>;
    async fn destroy_mark(&mut self, id: Self::MarkId) -> Result<()>;

    async fn get_mark_position(&self, id: Self::MarkId) -> Result<Position>;
    async fn set_mark_position(&mut self, id: Self::MarkId, pos: &Position) -> Result<()>;
}

#[derive(Debug)]
pub struct MarkHandle<B>
where
    B: BufferHandle,
    B::Buffer: MarksBuffer,
{
    id: <B::Buffer as MarksBuffer>::MarkId,
    buffer: B,
}

impl<B: Clone> Clone for MarkHandle<B>
where
    B: BufferHandle,
    B::Buffer: MarksBuffer,
{
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            buffer: self.buffer.clone(),
        }
    }
}

impl<B> MarkHandle<B>
where
    B: BufferHandle,
    B::Buffer: MarksBuffer,
{
    pub async fn new(buffer: &B, position: &Position) -> Result<Self> {
        let id = buffer.write().await.create_mark(position).await?;

        Ok(Self {
            id,
            buffer: buffer.clone(),
        })
    }

    pub async fn destroy(self) -> Result<()> {
        self.buffer.write().await.destroy_mark(self.id).await
    }

    pub async fn get_position(&self) -> Result<Position> {
        self.buffer.read().await.get_mark_position(self.id).await
    }

    pub async fn set_position(&self, position: &Position) -> Result<()> {
        self.buffer
            .write()
            .await
            .set_mark_position(self.id, position)
            .await
    }
}

#[cfg(feature = "tests")]
pub mod tests {
    use crate::{Editor, test_utils::new_buffer_with_content};

    use super::*;

    #[doc(hidden)]
    pub use paste::paste;

    pub async fn _test_buffer_marks_basic<E>(editor: E)
    where
        E: Editor,
        E::Buffer: MarksBuffer,
    {
        let buffer = new_buffer_with_content(&editor, "test\ntest2").await;

        let mark = MarkHandle::new(&buffer, &Position::new(0, 1))
            .await
            .expect("Failed to create mark");

        let position = mark.get_position().await.expect("Failed to get position");

        assert_eq!(position, Position::new(0, 1));

        mark.set_position(&Position::new(1, 0))
            .await
            .expect("Failed to set position");

        let position = mark.get_position().await.expect("Failed to get position");

        assert_eq!(position, Position::new(1, 0));

        {
            let mark = mark.clone();
            mark.destroy().await.expect("Failed to destroy mark");
        }

        // TODO: Verify specific error
        assert!(
            mark.get_position().await.is_err(),
            "Operation on a destroyed mark should error"
        );
    }

    pub async fn _test_buffer_marks_set_text<E>(editor: E)
    where
        E: Editor,
        E::Buffer: MarksBuffer,
    {
        let buffer = new_buffer_with_content(&editor, "First line").await;

        let mark = MarkHandle::new(&buffer, &Position::new(0, 6))
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

    // TODO: More tests. This has many edge cases that need to have defined behaviour.

    #[macro_export]
    macro_rules! eel_marks_buffer_tests {
        (@test $test_name:ident, $test_tag:path) => {
            $crate::marks::tests::paste! {
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

        ($test_tag:path) => {
            eel_marks_buffer_tests!(@test test_buffer_marks_basic, $test_tag);
            eel_marks_buffer_tests!(@test test_buffer_marks_set_text, $test_tag);
        };
    }
}
