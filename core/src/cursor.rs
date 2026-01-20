use async_trait::async_trait;

use crate::{
    Position, Result,
    buffer::{BufferHandle, ReadBuffer, WriteBuffer},
};

#[async_trait]
pub trait CursorReadBuffer: ReadBuffer {
    async fn get_cursor(&self) -> Result<Position>;
}

#[async_trait]
pub trait CursorWriteBuffer: CursorReadBuffer + WriteBuffer {
    async fn set_cursor(&mut self, position: &Position) -> Result<()>;

    async fn append_at_cursor(&mut self, text: &str) -> Result<()> {
        self.append_at_position(&self.get_cursor().await?, text)
            .await
    }

    async fn prepend_at_cursor(&mut self, text: &str) -> Result<()> {
        self.prepend_at_position(&self.get_cursor().await?, text)
            .await
    }

    async fn type_text(&mut self, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }

        let position = self.get_cursor().await?;
        let max_text_pos = Position::max_text_pos(text);

        let next_position = position.clone().next_col();

        let position = if self.validate_pos(&next_position).await.is_ok() {
            next_position
        } else {
            position
        };

        self.prepend_at_position(&position, text).await?;

        self.set_cursor(&position.offset(&max_text_pos).prev_col())
            .await
    }
}

pub trait CursorBufferHandle:
    BufferHandle<ReadBuffer = Self::CReadBuffer, WriteBuffer = Self::CWriteBuffer>
{
    type CReadBuffer: CursorReadBuffer;
    type CWriteBuffer: CursorWriteBuffer;
}

impl<B> CursorBufferHandle for B
where
    B: BufferHandle,
    B::ReadBuffer: CursorReadBuffer,
    B::WriteBuffer: CursorWriteBuffer,
{
    type CReadBuffer = B::ReadBuffer;
    type CWriteBuffer = B::WriteBuffer;
}

#[cfg(feature = "tests")]
pub mod tests {

    use crate::{
        Editor, assert_buffer_content, assert_buffer_error, assert_buffer_state, assert_cursor_pos,
        buffer::BufferHandle, test_utils::new_buffer_with_state,
    };

    use super::*;

    pub async fn test_cursor<E>(editor: E)
    where
        E: Editor,
        E::BufferHandle: CursorBufferHandle,
    {
        let buffer = new_buffer_with_state(&editor, "|").await;

        assert_cursor_pos!(buffer, Position::new(0, 0));

        let buffer = new_buffer_with_state(
            &editor,
            r#"|First line
Second line"#,
        )
        .await;

        assert_cursor_pos!(buffer, Position::new(0, 0));

        buffer
            .write()
            .await
            .set_cursor(&Position::new(1, 4))
            .await
            .expect("Failed to set cursor");

        assert_cursor_pos!(buffer, Position::new(1, 4));

        buffer
            .write()
            .await
            .set_cursor(&Position::new(0, 0))
            .await
            .expect("Failed to set cursor");

        assert_cursor_pos!(buffer, Position::new(0, 0));

        buffer
            .write()
            .await
            .set_cursor(&Position::new(1, 11))
            .await
            .expect("Failed to set cursor");

        assert_cursor_pos!(buffer, Position::new(1, 11));

        assert_buffer_error!(
            buffer.write().await.set_cursor(&Position::new(2, 0)).await,
            crate::Error::Buffer(crate::buffer::Error::RowOutOfBounds { row: 2, max: 1 })
        );

        assert_buffer_error!(
            buffer.write().await.set_cursor(&Position::new(1, 12)).await,
            crate::Error::Buffer(crate::buffer::Error::ColOutOfBounds { col: 12, max: 11 })
        );

        assert_buffer_error!(
            buffer.write().await.set_cursor(&Position::new(0, 12)).await,
            crate::Error::Buffer(crate::buffer::Error::ColOutOfBounds { col: 12, max: 10 })
        );
    }

    pub async fn test_cursor_append<E>(editor: E)
    where
        E: Editor,
        E::BufferHandle: CursorBufferHandle,
    {
        let buffer = new_buffer_with_state(
            &editor,
            r#"First line
Second line
Third| line
"#,
        )
        .await;

        buffer
            .write()
            .await
            .append_at_cursor("test ")
            .await
            .expect("Failed to append at cursor");

        assert_buffer_content!(
            buffer,
            r#"First line
Second line
Third test line
"#
        );

        buffer
            .write()
            .await
            .set_cursor(&Position::new(2, 6))
            .await
            .expect("Failed to set cursor");

        buffer
            .write()
            .await
            .prepend_at_cursor("(3rd) ")
            .await
            .expect("Failed to prepend at cursor");

        assert_buffer_content!(
            buffer,
            r#"First line
Second line
Third (3rd) test line
"#
        );
    }

    pub async fn test_cursor_type_text<E>(editor: E)
    where
        E: Editor,
        E::BufferHandle: CursorBufferHandle,
    {
        let buffer = new_buffer_with_state(
            &editor,
            r#"First line
Second| line
Third line!"#,
        )
        .await;

        buffer
            .write()
            .await
            .type_text("test ")
            .await
            .expect("Failed to type text");

        assert_buffer_state!(
            buffer,
            r#"First line
Second test| line
Third line!"#
        );

        buffer
            .write()
            .await
            .type_text("test\n")
            .await
            .expect("Failed to type text");

        assert_buffer_state!(
            buffer,
            r#"First line
Second test test
|line
Third line!"#
        );
    }

    pub async fn test_cursor_type_text_empty<E>(editor: E)
    where
        E: Editor,
        E::BufferHandle: CursorBufferHandle,
    {
        let buffer = new_buffer_with_state(&editor, "|").await;

        buffer
            .write()
            .await
            .type_text("test")
            .await
            .expect("Failed to type text");

        assert_buffer_state!(buffer, r#"tes|t"#);
    }

    #[macro_export]
    macro_rules! eel_cursor_tests {
        ($test_tag:path, $editor_factory:expr, $prefix:literal) => {
            $crate::eel_tests!(
                test_tag: $test_tag,
                editor_factory: $editor_factory,
                editor_bounds: {
                    <E::BufferHandle as $crate::buffer::BufferHandle>::ReadBuffer: $crate::cursor::CursorReadBuffer,
                    <E::BufferHandle as $crate::buffer::BufferHandle>::WriteBuffer: $crate::cursor::CursorWriteBuffer,
                },
                module_path: $crate::cursor::tests,
                prefix: $prefix,
                tests: [
                    test_cursor,
                    test_cursor_append,
                    test_cursor_type_text,
                    test_cursor_type_text_empty
                ],
            );
        };

        ($test_tag:path, $editor_factory:expr) => {
            $crate::eel_cursor_tests!($test_tag, $editor_factory, "");
        };
    }
}
