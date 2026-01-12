use std::ops::RangeBounds;

use crate::{Position, Result};

use async_trait::async_trait;
use itertools::Itertools;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Row out of bounds: {row} (max {max})")]
    RowOutOfBounds { row: usize, max: usize },

    #[error("Col out of bounds: {col} (max {max})")]
    ColOutOfBounds { col: usize, max: usize },

    #[error("Mark error: {0}")]
    Mark(#[from] crate::marks::Error),

    #[error("Error: {0}")]
    Custom(Box<dyn std::error::Error + Sync + Send>),
}

#[async_trait]
pub trait Buffer: Send + Sync {
    async fn line_count(&self) -> Result<usize>;
    async fn get_lines<R: RangeBounds<usize> + Send + 'static>(
        &self,
        range: R,
    ) -> Result<impl Iterator<Item = String> + Send>;

    async fn set_text(&mut self, start: &Position, end: &Position, text: &str) -> Result<()>;

    async fn max_row(&self) -> Result<usize> {
        Ok(self.line_count().await? - 1)
    }

    async fn max_pos(&self) -> Result<Position> {
        self.max_row_pos(self.max_row().await?).await
    }

    async fn max_row_pos(&self, row: usize) -> Result<Position> {
        let row_len = self.get_line(row).await?.len();
        Ok(Position::new(row, row_len))
    }

    async fn validate_pos(&self, position: &Position) -> Result<()> {
        let max_row = self.max_row().await?;

        if position.row > max_row {
            Err(Error::RowOutOfBounds {
                row: position.row,
                max: max_row,
            })?;
        }

        let max_col = self.max_row_pos(position.row).await?.col;

        if position.col > max_col {
            Err(Error::ColOutOfBounds {
                col: position.col,
                max: max_col,
            })?;
        }

        Ok(())
    }

    async fn get_line(&self, row: usize) -> Result<String> {
        let max_row = self.max_row().await?;

        if row > max_row {
            Err(Error::RowOutOfBounds { row, max: max_row })?;
        }

        let line = self
            .get_lines(row..(row + 1))
            .await?
            .next()
            .ok_or(Error::RowOutOfBounds { row, max: max_row })?;

        Ok(line)
    }

    async fn get_all_lines(&self) -> Result<impl Iterator<Item = String>> {
        self.get_lines(0..self.line_count().await?).await
    }

    async fn get_content(&self) -> Result<String> {
        Ok(self.get_all_lines().await?.join("\n"))
    }

    async fn set_content(&mut self, text: &str) -> Result<()> {
        self.set_text(&Position::origin(), &self.max_pos().await?, text)
            .await
    }

    async fn set_line(&mut self, row: usize, line: &str) -> Result<()> {
        let row_end = self.max_row_pos(row).await?;

        self.set_text(&Position::new(row, 0), &row_end, line).await
    }

    async fn append_at_position(&mut self, position: &Position, text: &str) -> Result<()> {
        let next_position = position.clone().next_col();

        let position = if self.validate_pos(&next_position).await.is_ok() {
            &next_position
        } else {
            position
        };

        self.set_text(position, position, text).await?;

        Ok(())
    }

    async fn prepend_at_position(&mut self, position: &Position, text: &str) -> Result<()> {
        self.set_text(position, position, text).await
    }

    async fn append(&mut self, text: &str) -> Result<()> {
        let mut max_pos = self.max_pos().await?;

        if max_pos.col > 0 {
            max_pos = max_pos.prev_col();
        }

        self.append_at_position(&max_pos, text).await
    }

    async fn prepend(&mut self, text: &str) -> Result<()> {
        self.prepend_at_position(&Position::origin(), text).await
    }
}

pub trait BufferReadLock<B: Buffer>: std::ops::Deref<Target = B> + Sync + Send + 'static {}
pub trait BufferWriteLock<B: Buffer>: std::ops::DerefMut<Target = B> + BufferReadLock<B> {}

impl<B, D> BufferReadLock<B> for D
where
    B: Buffer,
    D: std::ops::Deref<Target = B> + Sync + Send + 'static,
{
}

impl<B, D> BufferWriteLock<B> for D
where
    B: Buffer,
    D: std::ops::DerefMut<Target = B> + Sync + Send + 'static,
{
}

pub trait BufferHandle: Clone + Send + Sync + 'static {
    type Buffer: Buffer;

    fn read(&self) -> impl Future<Output = impl BufferReadLock<Self::Buffer>> + Send + 'static;

    fn write(&self) -> impl Future<Output = impl BufferWriteLock<Self::Buffer>> + Send + 'static;
}

#[cfg(feature = "tests")]
pub mod tests {
    use super::*;

    #[doc(hidden)]
    pub use paste::paste;

    use crate::{
        assert_buffer_content, assert_buffer_error, async_runtime, editor::Editor,
        test_utils::new_buffer_with_content,
    };

    pub async fn _test_buffer_pos(editor: impl Editor) {
        let buffer = new_buffer_with_content(
            &editor,
            r#"First line
Second line
Third line!"#,
        )
        .await;

        assert_eq!(
            buffer
                .read()
                .await
                .max_row()
                .await
                .expect("Failed to get max row"),
            2
        );

        assert_eq!(
            buffer
                .read()
                .await
                .max_row_pos(0)
                .await
                .expect("Failed to get max row pos"),
            Position::new(0, 10)
        );

        assert_eq!(
            buffer
                .read()
                .await
                .max_row_pos(2)
                .await
                .expect("Failed to get max row pos"),
            Position::new(2, 11)
        );

        assert_eq!(
            buffer
                .read()
                .await
                .max_pos()
                .await
                .expect("Failed to get max pos"),
            Position::new(2, 11)
        );

        let buffer = new_buffer_with_content(&editor, "").await;

        assert_eq!(
            buffer
                .read()
                .await
                .max_row()
                .await
                .expect("Failed to get max row"),
            0
        );

        assert_eq!(
            buffer
                .read()
                .await
                .max_pos()
                .await
                .expect("Failed to get max pos"),
            Position::new(0, 0)
        );

        let buffer = new_buffer_with_content(
            &editor,
            r#"First line
Second line
Third line!
"#,
        )
        .await;

        assert_eq!(
            buffer
                .read()
                .await
                .max_row()
                .await
                .expect("Failed to get max row"),
            3
        );

        assert_eq!(
            buffer
                .read()
                .await
                .max_pos()
                .await
                .expect("Failed to get max pos"),
            Position::new(3, 0)
        );
    }

    pub async fn _test_buffer_set_text(editor: impl Editor) {
        let buffer = new_buffer_with_content(
            &editor,
            r#"First line
Second line
Third line!"#,
        )
        .await;

        buffer
            .write()
            .await
            .set_text(&Position::new(0, 6), &Position::new(2, 5), ":)")
            .await
            .expect("Failed to set text");

        assert_buffer_content!(buffer, r#"First :) line!"#);

        buffer
            .write()
            .await
            .set_text(&Position::new(0, 6), &Position::new(0, 9), "")
            .await
            .expect("Failed to set text");

        assert_buffer_content!(buffer, r#"First line!"#);

        buffer
            .write()
            .await
            .set_text(&Position::new(0, 11), &Position::new(0, 11), " (wow)")
            .await
            .expect("Failed to set text");

        assert_buffer_content!(buffer, r#"First line! (wow)"#);

        let buffer = new_buffer_with_content(
            &editor,
            r#"

Some line
"#,
        )
        .await;

        buffer
            .write()
            .await
            .set_text(&Position::new(2, 0), &Position::new(2, 9), "")
            .await
            .expect("Failed to set text");

        assert_buffer_content!(
            buffer, r#"


"#
        );

        buffer
            .write()
            .await
            .set_text(&Position::new(2, 0), &Position::new(2, 0), "This was empty")
            .await
            .expect("Failed to set text");

        assert_buffer_content!(
            buffer,
            r#"

This was empty
"#
        );

        buffer
            .write()
            .await
            .set_text(&Position::new(0, 0), &Position::new(2, 0), "New line\n")
            .await
            .expect("Failed to set text");

        assert_buffer_content!(
            buffer,
            r#"New line
This was empty
"#
        );

        buffer
            .write()
            .await
            .set_text(&Position::new(1, 0), &Position::new(1, 0), "Hey, ")
            .await
            .expect("Failed to set text");

        assert_buffer_content!(
            buffer,
            r#"New line
Hey, This was empty
"#
        );
    }

    pub async fn _test_buffer_append(editor: impl Editor) {
        let buffer = new_buffer_with_content(&editor, "").await;

        buffer
            .write()
            .await
            .append("First line")
            .await
            .expect("Failed to append");

        assert_buffer_content!(buffer, "First line");

        buffer
            .write()
            .await
            .append("\nSecond line")
            .await
            .expect("Failed to append");

        assert_buffer_content!(buffer, "First line\nSecond line");
    }

    pub async fn _test_buffer_prepend(editor: impl Editor) {
        let buffer = new_buffer_with_content(&editor, "").await;

        buffer
            .write()
            .await
            .prepend("Second line")
            .await
            .expect("Failed to prepend");

        assert_buffer_content!(buffer, "Second line");

        buffer
            .write()
            .await
            .prepend("First line\n")
            .await
            .expect("Failed to prepend");

        assert_buffer_content!(buffer, "First line\nSecond line");
    }

    pub async fn _test_buffer_pos_append(editor: impl Editor) {
        let buffer = new_buffer_with_content(
            &editor,
            r#"First line
Second line
Third line!"#,
        )
        .await;

        buffer
            .write()
            .await
            .append_at_position(&Position::new(1, 6), "test ")
            .await
            .expect("Failed to append at position");

        assert_buffer_content!(
            buffer,
            r#"First line
Second test line
Third line!"#
        );

        buffer
            .write()
            .await
            .append_at_position(&Position::new(2, 10), " :)")
            .await
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
                .await
                .append_at_position(&Position::new(3, 0), ":(")
                .await,
            crate::Error::Buffer(Error::RowOutOfBounds { row: 3, max: 2 })
        );
        assert_buffer_error!(
            buffer
                .write()
                .await
                .append_at_position(&Position::new(1, 17), ":(")
                .await,
            crate::Error::Buffer(Error::ColOutOfBounds { col: 17, max: 16 })
        );

        buffer
            .write()
            .await
            .prepend_at_position(&Position::new(1, 16), " ;)")
            .await
            .expect("Failed to prepend at position");

        assert_buffer_content!(
            buffer,
            r#"First line
Second test line ;)
Third line! :)"#
        );

        buffer
            .write()
            .await
            .prepend_at_position(&Position::new(0, 0), "Actual first line\n")
            .await
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
                .await
                .prepend_at_position(&Position::new(4, 0), ":(")
                .await,
            crate::Error::Buffer(Error::RowOutOfBounds { row: 4, max: 3 })
        );
    }

    pub async fn _test_buffer_append_many(editor: impl Editor) {
        let buffer = new_buffer_with_content(&editor, "").await;

        let mut data = String::new();

        for i in 0..20000 {
            let line = format!("{i}\n");
            buffer
                .write()
                .await
                .append(&line)
                .await
                .expect("Failed to append");

            data.push_str(&line);
        }

        let content = buffer
            .read()
            .await
            .get_content()
            .await
            .expect("Failed to get content");

        assert!(content == data, "Content should be the same");
    }

    #[allow(clippy::manual_async_fn)]
    pub fn _test_buffer_set_text_parallel(
        editor: impl Editor + 'static,
    ) -> impl Future<Output = ()> + Send + 'static {
        async move {
            let buffer = new_buffer_with_content(&editor, "").await;

            let mut nums = (0..20000).map(|i| i.to_string()).collect::<Vec<_>>();

            let futures = nums
                .clone()
                .into_iter()
                .map(|i| {
                    let buffer = buffer.clone();

                    async_runtime::spawn(async move {
                        buffer.write().await.append(&format!("{i}\n")).await
                    })
                })
                .collect::<Vec<_>>();

            for future in futures {
                future
                    .await
                    .expect("Failed to join")
                    .expect("Failed to append");
            }

            let mut values = buffer
                .read()
                .await
                .get_all_lines()
                .await
                .expect("Failed to get all lines")
                .collect::<Vec<_>>();

            values.sort();

            nums.push(String::new());
            nums.sort();

            assert!(values == nums, "Lists should be the same");
        }
    }

    #[macro_export]
    macro_rules! eel_buffer_tests {
        (@test $test_name:ident, $test_tag:path) => {
            $crate::buffer::tests::paste! {
                #[$test_tag]
                async fn $test_name(editor: impl $crate::Editor + 'static) {
                    $crate::buffer::tests::[< _ $test_name >](editor).await;
                }
            }
        };

        ($test_tag:path) => {
            eel_buffer_tests!(@test test_buffer_pos, $test_tag);
            eel_buffer_tests!(@test test_buffer_set_text, $test_tag);
            eel_buffer_tests!(@test test_buffer_append, $test_tag);
            eel_buffer_tests!(@test test_buffer_prepend, $test_tag);
            eel_buffer_tests!(@test test_buffer_pos_append, $test_tag);
            eel_buffer_tests!(@test test_buffer_append_many, $test_tag);
            eel_buffer_tests!(@test test_buffer_set_text_parallel, $test_tag);
        };
    }
}
