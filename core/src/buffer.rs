use std::ops::RangeBounds;

use crate::{Position, Result};

use itertools::Itertools;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Row out of bounds: {row} (limit {limit})")]
    RowOutOfBounds { row: isize, limit: usize },

    #[error("Col out of bounds: {col} (limit {limit})")]
    ColOutOfBounds { col: isize, limit: usize },

    #[error("Error: {0}")]
    Custom(Box<dyn std::error::Error + Sync + Send>),
}

pub trait ReadBuffer: Send + Sync {
    fn line_count(&self) -> Result<usize>;
    fn get_lines<R: RangeBounds<usize> + Send + 'static>(
        &self,
        range: R,
    ) -> Result<impl Iterator<Item = String> + Send>;

    fn max_row(&self) -> Result<usize> {
        Ok(self.line_count()? - 1)
    }

    fn max_pos(&self) -> Result<Position> {
        self.max_row_pos(self.max_row()?)
    }

    fn max_row_pos(&self, row: usize) -> Result<Position> {
        let row_len = self.get_line(row)?.len();
        Ok(Position::new(row, row_len))
    }

    fn validate_pos(&self, position: &Position) -> Result<()> {
        let max_row = self.max_row()?;

        if position.row > max_row {
            Err(Error::RowOutOfBounds {
                row: position.row as isize,
                limit: max_row,
            })?;
        }

        let max_col = self.max_row_pos(position.row)?.col;

        if position.col > max_col {
            Err(Error::ColOutOfBounds {
                col: position.col as isize,
                limit: max_col,
            })?;
        }

        Ok(())
    }

    fn get_line(&self, row: usize) -> Result<String> {
        let max_row = self.max_row()?;

        if row > max_row {
            Err(Error::RowOutOfBounds {
                row: row as isize,
                limit: max_row,
            })?;
        }

        let line = self
            .get_lines(row..(row + 1))?
            .next()
            .ok_or(Error::RowOutOfBounds {
                row: row as isize,
                limit: max_row,
            })?;

        Ok(line)
    }

    fn get_all_lines(&self) -> Result<impl Iterator<Item = String>> {
        self.get_lines(0..self.line_count()?)
    }

    fn get_content(&self) -> Result<String> {
        Ok(self.get_all_lines()?.join("\n"))
    }
}

pub trait WriteBuffer: ReadBuffer {
    fn set_text(&mut self, start: &Position, end: &Position, text: &str) -> Result<()>;

    fn set_content(&mut self, text: &str) -> Result<()> {
        self.set_text(&Position::origin(), &self.max_pos()?, text)
    }

    fn set_line(&mut self, row: usize, line: &str) -> Result<()> {
        let row_end = self.max_row_pos(row)?;

        self.set_text(&Position::new(row, 0), &row_end, line)
    }

    fn append_at_position(&mut self, position: &Position, text: &str) -> Result<()> {
        let next_position = position.clone().next_col();

        let position = if self.validate_pos(&next_position).is_ok() {
            &next_position
        } else {
            position
        };

        self.set_text(position, position, text)?;

        Ok(())
    }

    fn prepend_at_position(&mut self, position: &Position, text: &str) -> Result<()> {
        self.set_text(position, position, text)
    }

    fn append(&mut self, text: &str) -> Result<()> {
        let mut max_pos = self.max_pos()?;

        if max_pos.col > 0 {
            max_pos = max_pos.prev_col();
        }

        self.append_at_position(&max_pos, text)
    }

    fn prepend(&mut self, text: &str) -> Result<()> {
        self.prepend_at_position(&Position::origin(), text)
    }
}

pub trait ReadBufferLock: std::ops::Deref<Target = Self::ReadBuffer> + Sync + Send {
    type ReadBuffer: ReadBuffer;
}
pub trait WriteBufferLock:
    ReadBufferLock<ReadBuffer = Self::WriteBuffer> + std::ops::DerefMut<Target = Self::WriteBuffer>
{
    type WriteBuffer: WriteBuffer;
}

impl<D, B> ReadBufferLock for D
where
    B: ReadBuffer,
    D: std::ops::Deref<Target = B> + Sync + Send,
{
    type ReadBuffer = B;
}

impl<B, D> WriteBufferLock for D
where
    B: WriteBuffer + ReadBuffer,
    D: ReadBufferLock<ReadBuffer = B>,
    D: std::ops::DerefMut<Target = B>,
{
    type WriteBuffer = B;
}

pub trait BufferHandle: Eq + Clone + Send + Sync + 'static {
    type ReadBuffer: ReadBuffer;
    type WriteBuffer: WriteBuffer;
    type ReadBufferLock: ReadBufferLock<ReadBuffer = Self::ReadBuffer> + 'static;
    type WriteBufferLock: WriteBufferLock<WriteBuffer = Self::WriteBuffer> + 'static;

    fn read(&self) -> Self::ReadBufferLock;
    fn write(&self) -> Self::WriteBufferLock;
}

#[cfg(feature = "tests")]
pub mod tests {
    use super::*;

    use rayon::iter::{IntoParallelIterator, ParallelIterator};

    use crate::{
        assert_buffer_content, assert_buffer_error, editor::Editor,
        test_utils::new_buffer_with_content,
    };

    pub fn test_buffer_pos(editor: impl Editor) {
        let buffer = new_buffer_with_content(
            &editor,
            r#"First line
Second line
Third line!"#,
        );

        assert_eq!(buffer.read().max_row().expect("Failed to get max row"), 2);

        assert_eq!(
            buffer
                .read()
                .max_row_pos(0)
                .expect("Failed to get max row pos"),
            Position::new(0, 10)
        );

        assert_eq!(
            buffer
                .read()
                .max_row_pos(2)
                .expect("Failed to get max row pos"),
            Position::new(2, 11)
        );

        assert_eq!(
            buffer.read().max_pos().expect("Failed to get max pos"),
            Position::new(2, 11)
        );

        let buffer = new_buffer_with_content(&editor, "");

        assert_eq!(buffer.read().max_row().expect("Failed to get max row"), 0);

        assert_eq!(
            buffer.read().max_pos().expect("Failed to get max pos"),
            Position::new(0, 0)
        );

        let buffer = new_buffer_with_content(
            &editor,
            r#"First line
Second line
Third line!
"#,
        );

        assert_eq!(buffer.read().max_row().expect("Failed to get max row"), 3);

        assert_eq!(
            buffer.read().max_pos().expect("Failed to get max pos"),
            Position::new(3, 0)
        );
    }

    pub fn test_buffer_set_text(editor: impl Editor) {
        let buffer = new_buffer_with_content(
            &editor,
            r#"First line
Second line
Third line!"#,
        );

        buffer
            .write()
            .set_text(&Position::new(0, 6), &Position::new(2, 5), ":)")
            .expect("Failed to set text");

        assert_buffer_content!(buffer, r#"First :) line!"#);

        buffer
            .write()
            .set_text(&Position::new(0, 6), &Position::new(0, 9), "")
            .expect("Failed to set text");

        assert_buffer_content!(buffer, r#"First line!"#);

        buffer
            .write()
            .set_text(&Position::new(0, 11), &Position::new(0, 11), " (wow)")
            .expect("Failed to set text");

        assert_buffer_content!(buffer, r#"First line! (wow)"#);

        let buffer = new_buffer_with_content(
            &editor,
            r#"

Some line
"#,
        );

        buffer
            .write()
            .set_text(&Position::new(2, 0), &Position::new(2, 9), "")
            .expect("Failed to set text");

        assert_buffer_content!(
            buffer, r#"


"#
        );

        buffer
            .write()
            .set_text(&Position::new(2, 0), &Position::new(2, 0), "This was empty")
            .expect("Failed to set text");

        assert_buffer_content!(
            buffer,
            r#"

This was empty
"#
        );

        buffer
            .write()
            .set_text(&Position::new(0, 0), &Position::new(2, 0), "New line\n")
            .expect("Failed to set text");

        assert_buffer_content!(
            buffer,
            r#"New line
This was empty
"#
        );

        buffer
            .write()
            .set_text(&Position::new(1, 0), &Position::new(1, 0), "Hey, ")
            .expect("Failed to set text");

        assert_buffer_content!(
            buffer,
            r#"New line
Hey, This was empty
"#
        );
    }

    pub fn test_buffer_append(editor: impl Editor) {
        let buffer = new_buffer_with_content(&editor, "");

        buffer
            .write()
            .append("First line")
            .expect("Failed to append");

        assert_buffer_content!(buffer, "First line");

        buffer
            .write()
            .append("\nSecond line")
            .expect("Failed to append");

        assert_buffer_content!(buffer, "First line\nSecond line");
    }

    pub fn test_buffer_prepend(editor: impl Editor) {
        let buffer = new_buffer_with_content(&editor, "");

        buffer
            .write()
            .prepend("Second line")
            .expect("Failed to prepend");

        assert_buffer_content!(buffer, "Second line");

        buffer
            .write()
            .prepend("First line\n")
            .expect("Failed to prepend");

        assert_buffer_content!(buffer, "First line\nSecond line");
    }

    pub fn test_buffer_pos_append(editor: impl Editor) {
        let buffer = new_buffer_with_content(
            &editor,
            r#"First line
Second line
Third line!"#,
        );

        buffer
            .write()
            .append_at_position(&Position::new(1, 6), "test ")
            .expect("Failed to append at position");

        assert_buffer_content!(
            buffer,
            r#"First line
Second test line
Third line!"#
        );

        buffer
            .write()
            .append_at_position(&Position::new(2, 10), " :)")
            .expect("Failed to append at position");

        assert_buffer_content!(
            buffer,
            r#"First line
Second test line
Third line! :)"#
        );

        assert_buffer_error!(
            buffer
                .write()
                .append_at_position(&Position::new(3, 0), ":("),
            crate::Error::Buffer(Error::RowOutOfBounds { row: 3, limit: 2 })
        );
        assert_buffer_error!(
            buffer
                .write()
                .append_at_position(&Position::new(1, 17), ":("),
            crate::Error::Buffer(Error::ColOutOfBounds { col: 17, limit: 16 })
        );

        buffer
            .write()
            .prepend_at_position(&Position::new(1, 16), " ;)")
            .expect("Failed to prepend at position");

        assert_buffer_content!(
            buffer,
            r#"First line
Second test line ;)
Third line! :)"#
        );

        buffer
            .write()
            .prepend_at_position(&Position::new(0, 0), "Actual first line\n")
            .expect("Failed to prepend at position");

        assert_buffer_content!(
            buffer,
            r#"Actual first line
First line
Second test line ;)
Third line! :)"#
        );

        assert_buffer_error!(
            buffer
                .write()
                .prepend_at_position(&Position::new(4, 0), ":("),
            crate::Error::Buffer(Error::RowOutOfBounds { row: 4, limit: 3 })
        );
    }

    pub fn test_buffer_append_many(editor: impl Editor) {
        let buffer = new_buffer_with_content(&editor, "");

        let mut data = String::new();

        for i in 0..1000 {
            let line = format!("{i}\n");
            buffer.write().append(&line).expect("Failed to append");

            data.push_str(&line);
        }

        let content = buffer.read().get_content().expect("Failed to get content");

        assert!(content == data, "Content should be the same");
    }

    pub fn test_buffer_set_text_parallel(editor: impl Editor + 'static) {
        let buffer = new_buffer_with_content(&editor, "");

        let mut nums = (0..1000).map(|i| i.to_string()).collect::<Vec<_>>();

        nums.clone()
            .into_par_iter()
            .map(|i| {
                let buffer = buffer.clone();

                buffer.write().append(&format!("{i}\n"))
            })
            .for_each(|r| {
                r.expect("Failed to append");
            });

        let mut values = buffer
            .read()
            .get_all_lines()
            .expect("Failed to get all lines")
            .collect::<Vec<_>>();

        values.sort();

        nums.push(String::new());
        nums.sort();

        assert!(values == nums, "Lists should be the same");
    }

    #[macro_export]
    macro_rules! eel_buffer_tests {
        ($test_tag:path, $editor_factory:expr, $prefix:tt) => {
            $crate::eel_tests!(
                test_tag: $test_tag,
                editor_factory: $editor_factory,
                editor_bounds: {},
                module_path: $crate::buffer::tests,
                prefix: $prefix,
                tests: [
                    test_buffer_pos,
                    test_buffer_set_text,
                    test_buffer_append,
                    test_buffer_prepend,
                    test_buffer_pos_append,
                    test_buffer_append_many,
                    test_buffer_set_text_parallel,
                ],
            );
        };

        ($test_tag:path, $editor_factory:expr) => {
            $crate::eel_buffer_tests!($test_tag, $editor_factory, "");
        };
    }
}
