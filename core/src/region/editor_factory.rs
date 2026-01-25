use crate::{
    Editor, Position, Result,
    buffer::{BufferHandle, WriteBuffer},
    mark::MarkBufferHandle,
    region::BufferRegion,
    test_utils::{EditorFactory, new_buffer_with_content},
};

pub struct RegionEditor<E: Editor> {
    editor: E,
    empty: bool,
}

impl<E> Editor for RegionEditor<E>
where
    E: Editor,
    E::BufferHandle: MarkBufferHandle,
{
    type BufferHandle = BufferRegion<E::BufferHandle>;

    fn new_buffer(&self) -> Result<Self::BufferHandle> {
        let buffer = new_buffer_with_content(
            &self.editor,
            if self.empty {
                ""
            } else {
                r#"First line
Second line
Third line
Fourth line"#
            },
        );

        let region = if self.empty {
            BufferRegion::lock_new(&buffer, &Position::new(0, 0), &Position::new(0, 0))?
        } else {
            BufferRegion::lock_new(&buffer, &Position::new(1, 2), &Position::new(2, 5))?
        };

        region.write().set_content("")?;

        Ok(region)
    }

    // Not required for buffer tests

    fn current_buffer(&self) -> Result<Self::BufferHandle> {
        unimplemented!()
    }

    fn set_current_buffer(
        &self,
        _buffer: &mut <Self::BufferHandle as BufferHandle>::WriteBuffer,
    ) -> Result<()> {
        unimplemented!()
    }
}

pub fn region_editor_factory<E: EditorFactory + 'static>(
    editor_factory: E,
    empty: bool,
) -> impl EditorFactory<Editor = RegionEditor<E::Editor>>
where
    <E::Editor as Editor>::BufferHandle: MarkBufferHandle,
{
    move || RegionEditor {
        editor: editor_factory.create_editor(),
        empty,
    }
}
