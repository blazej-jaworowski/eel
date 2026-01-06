use async_trait::async_trait;

use crate::{Position, Result, buffer::Buffer};

#[async_trait]
pub trait CursorBuffer: Buffer {
    async fn get_cursor(&self) -> Result<Position>;
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

#[cfg(feature = "tests")]
pub mod tests {

    use crate::{
        Editor, assert_buffer_content, assert_buffer_error, assert_buffer_state, assert_cursor_pos,
        buffer::BufferHandle, test_utils::new_buffer_with_state,
    };

    use super::*;

    #[doc(hidden)]
    pub use paste::paste;

    pub async fn _test_buffer_cursor<E>(editor: E)
    where
        E: Editor,
        E::Buffer: CursorBuffer,
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

    pub async fn _test_buffer_cursor_append<E>(editor: E)
    where
        E: Editor,
        E::Buffer: CursorBuffer,
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

    pub async fn _test_buffer_type_text<E>(editor: E)
    where
        E: Editor,
        E::Buffer: CursorBuffer,
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

    pub async fn _test_buffer_type_text_empty<E>(editor: E)
    where
        E: Editor,
        E::Buffer: CursorBuffer,
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
    macro_rules! eel_cursor_buffer_tests {
        (@test $test_name:ident, $test_tag:path) => {
            $crate::buffer::tests::paste! {
                #[$test_tag]
                async fn $test_name<E>(editor: E)
                where
                    E: $crate::Editor + 'static,
                    E::Buffer: $crate::cursor::CursorBuffer,
                {
                    $crate::cursor::tests::[< _ $test_name >](editor).await;
                }
            }
        };

        ($test_tag:path) => {
            eel_cursor_buffer_tests!(@test test_buffer_cursor, $test_tag);
            eel_cursor_buffer_tests!(@test test_buffer_cursor_append, $test_tag);
            eel_cursor_buffer_tests!(@test test_buffer_type_text, $test_tag);
            eel_cursor_buffer_tests!(@test test_buffer_type_text_empty, $test_tag);
        };
    }
}
