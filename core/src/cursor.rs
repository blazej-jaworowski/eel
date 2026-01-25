use crate::{
    Position, Result,
    buffer::{BufferHandle, ReadBuffer, WriteBuffer},
};

pub trait CursorReadBuffer: ReadBuffer {
    fn get_cursor(&self) -> Result<Position>;
}

pub trait CursorWriteBuffer: CursorReadBuffer + WriteBuffer {
    fn set_cursor(&mut self, position: &Position) -> Result<()>;

    fn append_at_cursor(&mut self, text: &str) -> Result<()> {
        self.append_at_position(&self.get_cursor()?, text)
    }

    fn prepend_at_cursor(&mut self, text: &str) -> Result<()> {
        self.prepend_at_position(&self.get_cursor()?, text)
    }

    fn type_text(&mut self, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }

        let position = self.get_cursor()?;
        let max_text_pos = Position::max_text_pos(text);

        let next_position = position.clone().next_col();

        let position = if self.validate_pos(&next_position).is_ok() {
            next_position
        } else {
            position
        };

        self.prepend_at_position(&position, text)?;

        self.set_cursor(&position.offset(&max_text_pos).prev_col())
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

    pub fn test_cursor<E>(editor: E)
    where
        E: Editor,
        E::BufferHandle: CursorBufferHandle,
    {
        let buffer = new_buffer_with_state(&editor, "|");

        assert_cursor_pos!(buffer, Position::new(0, 0));

        let buffer = new_buffer_with_state(
            &editor,
            r#"|First line
Second line"#,
        );

        assert_cursor_pos!(buffer, Position::new(0, 0));

        buffer
            .write()
            .set_cursor(&Position::new(1, 4))
            .expect("Failed to set cursor");

        assert_cursor_pos!(buffer, Position::new(1, 4));

        buffer
            .write()
            .set_cursor(&Position::new(0, 0))
            .expect("Failed to set cursor");

        assert_cursor_pos!(buffer, Position::new(0, 0));

        buffer
            .write()
            .set_cursor(&Position::new(1, 11))
            .expect("Failed to set cursor");

        assert_cursor_pos!(buffer, Position::new(1, 11));

        assert_buffer_error!(
            buffer.write().set_cursor(&Position::new(2, 0)),
            crate::Error::Buffer(crate::buffer::Error::RowOutOfBounds { row: 2, limit: 1 })
        );

        assert_buffer_error!(
            buffer.write().set_cursor(&Position::new(1, 12)),
            crate::Error::Buffer(crate::buffer::Error::ColOutOfBounds { col: 12, limit: 11 })
        );

        assert_buffer_error!(
            buffer.write().set_cursor(&Position::new(0, 12)),
            crate::Error::Buffer(crate::buffer::Error::ColOutOfBounds { col: 12, limit: 10 })
        );
    }

    pub fn test_cursor_append<E>(editor: E)
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
        );

        buffer
            .write()
            .append_at_cursor("test ")
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
            .set_cursor(&Position::new(2, 6))
            .expect("Failed to set cursor");

        buffer
            .write()
            .prepend_at_cursor("(3rd) ")
            .expect("Failed to prepend at cursor");

        assert_buffer_content!(
            buffer,
            r#"First line
Second line
Third (3rd) test line
"#
        );
    }

    pub fn test_cursor_type_text<E>(editor: E)
    where
        E: Editor,
        E::BufferHandle: CursorBufferHandle,
    {
        let buffer = new_buffer_with_state(
            &editor,
            r#"First line
Second| line
Third line!"#,
        );

        buffer
            .write()
            .type_text("test ")
            .expect("Failed to type text");

        assert_buffer_state!(
            buffer,
            r#"First line
Second test| line
Third line!"#
        );

        buffer
            .write()
            .type_text("test\n")
            .expect("Failed to type text");

        assert_buffer_state!(
            buffer,
            r#"First line
Second test test
|line
Third line!"#
        );
    }

    pub fn test_cursor_type_text_empty<E>(editor: E)
    where
        E: Editor,
        E::BufferHandle: CursorBufferHandle,
    {
        let buffer = new_buffer_with_state(&editor, "|");

        buffer
            .write()
            .type_text("test")
            .expect("Failed to type text");

        assert_buffer_state!(buffer, r#"tes|t"#);
    }

    #[macro_export]
    macro_rules! eel_cursor_tests {
        ($test_tag:path, $editor_factory:expr, $prefix:tt) => {
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
