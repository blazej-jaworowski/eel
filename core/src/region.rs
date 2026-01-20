use std::{
    marker::PhantomData,
    ops::{Bound, RangeBounds},
};

use async_trait::async_trait;

use crate::{
    Position, Result,
    buffer::{BufferHandle, ReadBuffer, ReadBufferLock, WriteBuffer, WriteBufferLock},
    mark::{Gravity, Mark, MarkBufferHandle, MarkReadBuffer, MarkWriteBuffer},
};

pub struct BufferRegionAccess<'a, B, Buf, L>
where
    B: MarkBufferHandle,
    Buf: MarkReadBuffer<MarkId = B::MarkId>,
    L: ReadBufferLock<ReadBuffer = Buf> + 'a,
{
    start: Mark<B>,
    end: Mark<B>,
    buffer_lock: L,
    _mark: PhantomData<&'a ()>,
}

impl<'a, B, Buf, L> BufferRegionAccess<'a, B, Buf, L>
where
    B: MarkBufferHandle,
    Buf: MarkReadBuffer<MarkId = B::MarkId>,
    L: ReadBufferLock<ReadBuffer = Buf> + 'a,
{
    pub async fn translate_position(&self, pos: &Position) -> Result<Position> {
        let start_pos = self.start.read(&*self.buffer_lock).get_position().await?;

        Ok(Position {
            row: start_pos.row + pos.row,
            col: if pos.row == 0 {
                start_pos.col + pos.col
            } else {
                pos.col
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferRegion<B: MarkBufferHandle> {
    start: Mark<B>,
    end: Mark<B>,
    buffer: B,
}

impl<B: MarkBufferHandle> BufferRegion<B> {
    pub async fn new(
        buffer: &B,
        start: &Position,
        end: &Position,
        mut buffer_lock: impl WriteBufferLock<WriteBuffer = B::WriteBuffer>,
    ) -> Result<Self> {
        let start = Mark::new(buffer, start, &mut *buffer_lock).await?;
        let end = Mark::new(buffer, end, &mut *buffer_lock).await?;

        start
            .write(&mut *buffer_lock)
            .set_gravity(Gravity::Left)
            .await?;

        end.write(&mut *buffer_lock)
            .set_gravity(Gravity::Right)
            .await?;

        Ok(BufferRegion {
            start,
            end,
            buffer: buffer.clone(),
        })
    }

    pub async fn lock_new(buffer: &B, start: &Position, end: &Position) -> Result<Self> {
        let lock = buffer.write().await;

        Self::new(buffer, start, end, lock).await
    }
}

#[async_trait]
impl<'a, B, Buf, L> ReadBuffer for BufferRegionAccess<'a, B, Buf, L>
where
    B: MarkBufferHandle,
    Buf: MarkReadBuffer<MarkId = B::MarkId>,
    L: ReadBufferLock<ReadBuffer = Buf> + 'a,
{
    async fn line_count(&self) -> Result<usize> {
        let start = self.start.read(&*self.buffer_lock).get_position().await?;

        let end = self.end.read(&*self.buffer_lock).get_position().await?;

        Ok(end.row - start.row + 1)
    }

    async fn get_lines<R: RangeBounds<usize> + Send + 'static>(
        &self,
        range: R,
    ) -> Result<impl Iterator<Item = String> + Send> {
        let line_count = self.line_count().await?;

        let start_pos = self.start.read(&*self.buffer_lock).get_position().await?;

        let end_pos = self.end.read(&*self.buffer_lock).get_position().await?;

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

        let mut lines: Vec<String> = self
            .buffer_lock
            .get_lines(start_bound..end_bound)
            .await?
            .collect();

        if partial_last_line && let Some(l) = lines.last_mut() {
            l.truncate(end_pos.col);
        }

        if partial_first_line && let Some(l) = lines.first_mut() {
            *l = l.split_off(start_pos.col);
        }

        Ok(lines.into_iter())
    }
}

#[async_trait]
impl<'a, B, Buf, L> WriteBuffer for BufferRegionAccess<'a, B, Buf, L>
where
    B: MarkBufferHandle,
    Buf: MarkWriteBuffer<MarkId = B::MarkId>,
    L: WriteBufferLock<WriteBuffer = Buf> + 'a,
{
    async fn set_text(&mut self, start: &Position, end: &Position, text: &str) -> Result<()> {
        self.validate_pos(start).await?;
        self.validate_pos(end).await?;

        let abs_start = self.translate_position(start).await?;
        let abs_end = self.translate_position(end).await?;

        self.buffer_lock.set_text(&abs_start, &abs_end, text).await
    }
}

#[async_trait]
impl<B: MarkBufferHandle> BufferHandle for BufferRegion<B> {
    type ReadBuffer = BufferRegionAccess<'static, B, B::ReadBuffer, B::ReadBufferLock>;
    type WriteBuffer = BufferRegionAccess<'static, B, B::WriteBuffer, B::WriteBufferLock>;
    type ReadBufferLock = Box<Self::ReadBuffer>;
    type WriteBufferLock = Box<Self::WriteBuffer>;

    fn read(&self) -> impl Future<Output = Self::ReadBufferLock> + Send + 'static {
        let buffer = self.buffer.clone();
        let start = self.start.clone();
        let end = self.end.clone();

        async move {
            Box::new(BufferRegionAccess {
                start,
                end,
                buffer_lock: buffer.read().await,
                _mark: Default::default(),
            })
        }
    }

    fn write(&self) -> impl Future<Output = Self::WriteBufferLock> + Send + 'static {
        let buffer = self.buffer.clone();
        let start = self.start.clone();
        let end = self.end.clone();

        async move {
            Box::new(BufferRegionAccess {
                start,
                end,
                buffer_lock: buffer.write().await,
                _mark: Default::default(),
            })
        }
    }
}

// TODO: Implement MarkBuffer and CursorBuffer traits

#[cfg(feature = "tests")]
pub mod tests {
    use crate::{
        Editor,
        test_utils::{EditorFactory, new_buffer_with_content},
    };

    use super::*;

    async fn init_test_region<E>(editor: &E) -> (E::BufferHandle, BufferRegion<E::BufferHandle>)
    where
        E: Editor,
        E::BufferHandle: MarkBufferHandle,
    {
        let buffer = new_buffer_with_content(
            editor,
            r#"First line
Second line
Third line
Fourth line"#,
        )
        .await;

        let region = BufferRegion::lock_new(&buffer, &Position::new(1, 2), &Position::new(2, 5))
            .await
            .expect("Failed to create region");

        (buffer, region)
    }

    pub async fn test_region_line_count<E>(editor: E)
    where
        E: Editor,
        E::BufferHandle: MarkBufferHandle,
    {
        let (_, region) = init_test_region(&editor).await;

        assert_eq!(
            region
                .read()
                .await
                .line_count()
                .await
                .expect("Failed to get line count"),
            2
        );
    }

    pub async fn test_region_get_lines<E>(editor: E)
    where
        E: Editor,
        E::BufferHandle: MarkBufferHandle,
    {
        let (_, region) = init_test_region(&editor).await;

        let region = region.read().await;

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
        E::BufferHandle: MarkBufferHandle,
    {
        let (buffer, region) = init_test_region(&editor).await;

        region
            .write()
            .await
            .append(" line\nFourth line\nFifth")
            .await
            .expect("Failed to append");

        assert_eq!(
            region
                .read()
                .await
                .get_content()
                .await
                .expect("Failed to get content"),
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
            .write()
            .await
            .prepend("ll me on it\n")
            .await
            .expect("Failed to append");

        assert_eq!(
            region
                .read()
                .await
                .get_content()
                .await
                .expect("Failed to get content"),
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
            .write()
            .await
            .set_line(1, "Second line")
            .await
            .expect("Failed to set line");

        assert_eq!(
            region
                .read()
                .await
                .get_content()
                .await
                .expect("Failed to get content"),
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
        E::BufferHandle: MarkBufferHandle,
    {
        let buffer = new_buffer_with_content(
            &editor,
            r#"First line
Second line
Third line
Fourth line"#,
        )
        .await;

        let mut region =
            BufferRegion::lock_new(&buffer, &Position::new(1, 11), &Position::new(1, 11))
                .await
                .expect("Failed to create region")
                .write()
                .await;

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

        drop(region);

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

    pub struct RegionEditor<E: Editor> {
        editor: E,
    }

    #[async_trait]
    impl<E> Editor for RegionEditor<E>
    where
        E: Editor,
        E::BufferHandle: MarkBufferHandle,
    {
        type BufferHandle = BufferRegion<E::BufferHandle>;

        async fn new_buffer(&self) -> Result<Self::BufferHandle> {
            let (_, region) = init_test_region(&self.editor).await;

            region.write().await.set_content("").await?;

            Ok(region)
        }

        // Not required for buffer tests

        async fn current_buffer(&self) -> Result<Self::BufferHandle> {
            unimplemented!()
        }

        async fn set_current_buffer(
            &self,
            _buffer: &mut <Self::BufferHandle as BufferHandle>::WriteBuffer,
        ) -> Result<()> {
            unimplemented!()
        }
    }

    pub fn region_editor_factory<E: EditorFactory + 'static>(
        editor_factory: E,
    ) -> impl EditorFactory<Editor = RegionEditor<E::Editor>>
    where
        <E::Editor as Editor>::BufferHandle: MarkBufferHandle,
    {
        move || RegionEditor {
            editor: editor_factory.create_editor(),
        }
    }

    #[macro_export]
    macro_rules! eel_region_tests {
        ($test_tag:path, $editor_factory:expr, $prefix:tt) => {
            $crate::eel_tests!(
                test_tag: $test_tag,
                editor_factory: $editor_factory,
                editor_bounds: { E::BufferHandle: $crate::mark::MarkBufferHandle },
                module_path: $crate::region::tests,
                prefix: $prefix,
                tests: [
                    test_region_line_count,
                    test_region_get_lines,
                    test_region_set_text,
                    test_region_empty,
                ],
            );

            $crate::test_utils::paste! {
                $crate::eel_buffer_tests!(
                    $test_tag,
                    $crate::region::tests::region_editor_factory($editor_factory),
                    [< $prefix test_region_ >]
                );
            }
        };

        ($test_tag:path, $editor_factory:expr) => {
            $crate::eel_region_tests!($test_tag, $editor_factory, "");
        };
    }
}
