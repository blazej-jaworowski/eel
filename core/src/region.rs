use std::ops::{Bound, RangeBounds};

use async_trait::async_trait;

use crate::{
    Position, Result,
    buffer::{Buffer, BufferHandle, BufferReadLock, BufferWriteLock},
    mark::{Gravity, Mark, MarkBuffer},
};

pub struct BufferRegion<B>
where
    B: BufferHandle,
    B::Buffer: MarkBuffer,
{
    start: Mark<B>,
    end: Mark<B>,
}

impl<B> BufferRegion<B>
where
    B: BufferHandle,
    B::Buffer: MarkBuffer,
{
    pub async fn new(buffer: &B, start: &Position, end: &Position) -> Result<Self> {
        let start = Mark::new(buffer, start).await?;
        let end = Mark::new(buffer, end).await?;

        start.set_gravity(Gravity::Left).await?;
        end.set_gravity(Gravity::Right).await?;

        Ok(BufferRegion { start, end })
    }

    pub async fn new_locked(
        buffer: &B,
        start: &Position,
        end: &Position,
        buffer_lock: &mut impl BufferWriteLock<B::Buffer>,
    ) -> Result<Self> {
        let start = Mark::new_locked(buffer, start, buffer_lock).await?;
        let end = Mark::new_locked(buffer, end, buffer_lock).await?;

        start.set_gravity(Gravity::Left).await?;
        end.set_gravity(Gravity::Right).await?;

        Ok(BufferRegion { start, end })
    }

    pub async fn translate_position(&self, pos: &Position) -> Result<Position> {
        let lock = self.get_buffer().read().await;
        self.translate_position_locked(pos, &lock).await
    }

    pub async fn translate_position_locked(
        &self,
        pos: &Position,
        buffer_lock: &impl BufferReadLock<B::Buffer>,
    ) -> Result<Position> {
        let start_pos = self.start.get_position_locked(buffer_lock).await?;

        Ok(Position {
            row: start_pos.row + pos.row,
            col: if pos.row == 0 {
                start_pos.col + pos.col
            } else {
                pos.col
            },
        })
    }

    pub fn get_buffer(&self) -> &B {
        self.start.get_buffer()
    }
}

#[async_trait]
impl<B> Buffer for BufferRegion<B>
where
    B: BufferHandle,
    B::Buffer: MarkBuffer,
{
    async fn line_count(&self) -> Result<usize> {
        let buffer = self.get_buffer().read().await;

        let start = self.start.get_position_locked(&buffer).await?;
        let end = self.end.get_position_locked(&buffer).await?;

        Ok(end.row - start.row + 1)
    }

    async fn get_lines<R: RangeBounds<usize> + Send + 'static>(
        &self,
        range: R,
    ) -> Result<impl Iterator<Item = String> + Send> {
        let buffer = self.get_buffer().read().await;

        let line_count = self.line_count().await?;

        let start_pos = self.start.get_position_locked(&buffer).await?;
        let end_pos = self.end.get_position_locked(&buffer).await?;

        let start_bound = match range.start_bound() {
            Bound::Included(i) => *i,
            Bound::Excluded(i) => i + 1,
            Bound::Unbounded => 0,
        };
        let end_bound = match range.end_bound() {
            Bound::Included(i) => i + 1,
            Bound::Excluded(i) => *i,
            Bound::Unbounded => line_count,
        };

        let partial_first_line = start_bound == 0;
        let partial_last_line = end_bound == line_count;

        let start_bound = start_bound + start_pos.row;
        let end_bound = end_bound + start_pos.row;

        let mut lines: Vec<String> = buffer.get_lines(start_bound..end_bound).await?.collect();

        if partial_last_line && let Some(l) = lines.last_mut() {
            l.truncate(end_pos.col);
        }

        if partial_first_line && let Some(l) = lines.first_mut() {
            *l = l.split_off(start_pos.col);
        }

        Ok(lines.into_iter())
    }

    async fn set_text(&mut self, start: &Position, end: &Position, text: &str) -> Result<()> {
        let mut buffer = self.get_buffer().write().await;

        let abs_start = self.translate_position_locked(start, &buffer).await?;
        let abs_end = self.translate_position_locked(end, &buffer).await?;

        buffer.set_text(&abs_start, &abs_end, text).await
    }
}

// TODO: Implement MarkBuffer and CursorBuffer traits

#[cfg(feature = "tests")]
pub mod tests {
    use crate::{Editor, test_utils::new_buffer_with_content};

    use super::*;

    async fn init_test_region<E>(editor: &E) -> (E::BufferHandle, BufferRegion<E::BufferHandle>)
    where
        E: Editor,
        E::Buffer: MarkBuffer,
    {
        let buffer = new_buffer_with_content(
            editor,
            r#"First line
Second line
Third line
Fourth line"#,
        )
        .await;

        let region = BufferRegion::new(&buffer, &Position::new(1, 2), &Position::new(2, 5))
            .await
            .expect("Failed to create region");

        (buffer, region)
    }

    pub async fn test_region_line_count<E>(editor: E)
    where
        E: Editor,
        E::Buffer: MarkBuffer,
    {
        let (_, region) = init_test_region(&editor).await;

        assert_eq!(
            region.line_count().await.expect("Failed to get line count"),
            2
        );
    }

    pub async fn test_region_get_lines<E>(editor: E)
    where
        E: Editor,
        E::Buffer: MarkBuffer,
    {
        let (_, region) = init_test_region(&editor).await;

        assert_eq!(
            region
                .get_lines(0..=1)
                .await
                .expect("Failed to get lines")
                .collect::<Vec<_>>(),
            ["cond line", "Third"],
        );

        assert_eq!(
            region.get_line(0).await.expect("Failed to get line"),
            "cond line"
        );

        assert_eq!(
            region.get_line(1).await.expect("Failed to get line"),
            "Third"
        );

        assert_eq!(
            region.get_content().await.expect("Failed to get content"),
            "cond line\nThird"
        );
    }

    pub async fn test_region_set_text<E>(editor: E)
    where
        E: Editor,
        E::Buffer: MarkBuffer,
    {
        let (buffer, mut region) = init_test_region(&editor).await;

        region
            .append(" line\nFourth line\nFifth")
            .await
            .expect("Failed to append");

        assert_eq!(
            region.get_content().await.expect("Failed to get content"),
            "cond line\nThird line\nFourth line\nFifth"
        );

        assert_eq!(
            buffer
                .read()
                .await
                .get_content()
                .await
                .expect("Failed to get content"),
            r#"First line
Second line
Third line
Fourth line
Fifth line
Fourth line"#
        );

        region
            .prepend("ll me on it\n")
            .await
            .expect("Failed to append");

        assert_eq!(
            region.get_content().await.expect("Failed to get content"),
            "ll me on it\ncond line\nThird line\nFourth line\nFifth"
        );

        assert_eq!(
            buffer
                .read()
                .await
                .get_content()
                .await
                .expect("Failed to get content"),
            r#"First line
Sell me on it
cond line
Third line
Fourth line
Fifth line
Fourth line"#
        );

        region
            .set_line(1, "Second line")
            .await
            .expect("Failed to set line");

        assert_eq!(
            region.get_content().await.expect("Failed to get content"),
            "ll me on it\nSecond line\nThird line\nFourth line\nFifth"
        );

        assert_eq!(
            buffer
                .read()
                .await
                .get_content()
                .await
                .expect("Failed to get content"),
            r#"First line
Sell me on it
Second line
Third line
Fourth line
Fifth line
Fourth line"#
        );
    }

    pub async fn test_region_empty<E>(editor: E)
    where
        E: Editor,
        E::Buffer: MarkBuffer,
    {
        let buffer = new_buffer_with_content(
            &editor,
            r#"First line
Second line
Third line
Fourth line"#,
        )
        .await;

        let mut region = BufferRegion::new(&buffer, &Position::new(1, 11), &Position::new(1, 11))
            .await
            .expect("Failed to create region");

        assert_eq!(
            region.line_count().await.expect("Failed to get line count"),
            1
        );

        assert_eq!(
            region.get_content().await.expect("Failed to get content"),
            ""
        );

        region
            .set_content("\nActual third line")
            .await
            .expect("Failed to set content");

        assert_eq!(
            region.get_content().await.expect("Failed to get content"),
            "\nActual third line"
        );

        assert_eq!(
            region.line_count().await.expect("Failed to get line count"),
            2
        );

        assert_eq!(
            buffer
                .read()
                .await
                .get_content()
                .await
                .expect("Failed to get content"),
            r#"First line
Second line
Actual third line
Third line
Fourth line"#
        );
    }

    #[macro_export]
    macro_rules! eel_region_tests {
        ($test_tag:path, $editor_factory:expr, $prefix:literal) => {
            $crate::eel_tests!(
                test_tag: $test_tag,
                editor_factory: $editor_factory,
                editor_bounds: {},
                buffer_bounds: { $crate::mark::MarkBuffer },
                module_path: $crate::region::tests,
                prefix: $prefix,
                tests: [
                    test_region_line_count,
                    test_region_get_lines,
                    test_region_set_text,
                    test_region_empty,
                ],
            );
        };

        ($test_tag:path, $editor_factory:expr) => {
            $crate::eel_region_tests!($test_tag, $editor_factory, "");
        };
    }
}
