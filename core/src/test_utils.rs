use crate::{
    buffer::{BufferHandle, WriteBuffer},
    editor::Editor,
};

#[doc(hidden)]
pub use paste::paste;

#[macro_export]
macro_rules! assert_buffer_content {
    ($buffer:expr, $content:expr) => {{
        use $crate::buffer::ReadBuffer as _;

        let buffer = $buffer.read().expect("buffer dropped");
        let content = buffer.get_content().expect("Failed to get buffer content");
        assert_eq!(content, $content)
    }};
}

#[macro_export]
macro_rules! assert_buffer_error {
    ($value:expr, $error:pat) => {
        if let Err(e) = $value {
            assert!(matches!(e, $error))
        } else {
            assert!(false, "Expected buffer error, got: {:?}", $value)
        }
    };
}

pub fn new_buffer_with_content<E: Editor>(editor: &E, content: &str) -> E::BufferHandle {
    let buffer = editor.new_buffer().expect("Failed to create test buffer");

    assert_buffer_content!(buffer, "");

    buffer
        .write()
        .expect("buffer dropped")
        .set_content(content)
        .expect("Failed to set buffer content");

    assert_buffer_content!(buffer, content);

    buffer
}

#[cfg(feature = "cursor")]
mod cursor {
    use itertools::Itertools as _;

    use super::*;
    use crate::{
        Position,
        cursor::{CursorReadBuffer, CursorWriteBuffer},
    };

    #[macro_export]
    macro_rules! assert_cursor_pos {
        ($buffer:expr, $position:expr) => {{
            use $crate::cursor::CursorReadBuffer as _;

            let buffer = $buffer.read().expect("buffer dropped");
            let actual_pos = buffer.get_cursor().expect("Failed to get cursor");
            assert_eq!(actual_pos, $position, "Invalid cursor position");
        }};
    }

    #[macro_export]
    macro_rules! assert_buffer_state {
        ($buffer:expr, $state: expr) => {{
            let (content, position) = $crate::test_utils::parse_buffer_state($state);
            $crate::assert_buffer_content!($buffer, content);
            $crate::assert_cursor_pos!($buffer, position);
        }};
    }

    pub fn new_buffer_with_state<E>(editor: &E, state: &str) -> E::BufferHandle
    where
        E: Editor,
        <E::BufferHandle as BufferHandle>::ReadBuffer: CursorReadBuffer,
        <E::BufferHandle as BufferHandle>::WriteBuffer: CursorWriteBuffer,
    {
        let buffer = editor.new_buffer().expect("Failed to create test buffer");

        set_buffer_state(&buffer, state);

        assert_buffer_state!(buffer, state);

        buffer
    }

    pub fn set_buffer_state<B>(buffer: &B, state: &str)
    where
        B: BufferHandle,
        B::ReadBuffer: CursorReadBuffer,
        B::WriteBuffer: CursorWriteBuffer,
    {
        let (content, position) = parse_buffer_state(state);

        {
            let mut buffer_lock = buffer.write().expect("buffer dropped");

            buffer_lock
                .set_content(&content)
                .expect("Failed to set content");

            buffer_lock
                .set_cursor(&position)
                .expect("Failed to set position");
        }

        assert_buffer_state!(buffer, state)
    }

    pub fn parse_buffer_state(state: &str) -> (String, Position) {
        let lines = state.lines();
        let mut cursor_pos: Option<Position> = None;

        let mut content: String = lines
            .enumerate()
            .map(|(i, line)| {
                let parts = line.split("|").collect_vec();

                let (l, r) = match parts.as_slice() {
                    [s] => return s.to_string(),
                    [l, r] => (*l, *r),
                    _ => panic!("State string can only contain a single '|' cursor marker"),
                };

                if cursor_pos.is_some() {
                    panic!("State string can only contain a single '|' cursor marker");
                }

                cursor_pos = Some(Position::new(i, l.len()));

                format!("{l}{r}")
            })
            .join("\n");

        // str::lines() removes the last newline if it's present, we want to preserve it
        if state.ends_with("\n") {
            content.push('\n');
        }

        let cursor_pos = cursor_pos.expect("State string should contain a '|' cursor marker");

        (content, cursor_pos)
    }
}

#[cfg(feature = "cursor")]
pub use cursor::*;

pub trait EditorFactory {
    type Editor: Editor;

    fn create_editor(&self) -> Self::Editor;
}

impl<F, E> EditorFactory for F
where
    E: Editor,
    F: Fn() -> E,
{
    type Editor = E;

    fn create_editor(&self) -> Self::Editor {
        self()
    }
}

pub trait EditorTest<E, R>: Send + 'static {
    fn run(self, editor: E) -> R;
}

impl<F, E, R> EditorTest<E, R> for F
where
    F: FnOnce(E) -> R,
    F: Send + 'static,
    E: Editor,
    R: Send + 'static,
{
    fn run(self, editor: E) -> R {
        self(editor)
    }
}

#[macro_export]
macro_rules! eel_tests {
    (@test
        test_tag: $test_tag:path,
        editor_factory: $editor_factory:expr,
        editor_bounds: { $( $editor_bounds:tt )* },
        module_path: $module_path:path,
        prefix: $prefix:tt,
        test: $test_name:ident$(,)?
    ) => {
        $crate::test_utils::paste! {
            #[$test_tag(editor_factory = $editor_factory)]
            fn [< $prefix $test_name >]<E>(editor: E)
            where
                E: $crate::Editor,
                $( $editor_bounds )*
            {
                $module_path::$test_name(editor);
            }
        }
    };

    (
        test_tag: $test_tag:path,
        editor_factory: $editor_factory:expr,
        editor_bounds: $editor_bounds:tt,
        module_path: $module_path:path,
        prefix: $prefix:tt,
        tests: [ $( $test_name:ident ),* $(,)? ],
    ) => {
        $(
            $crate::eel_tests!(@test
                test_tag: $test_tag,
                editor_factory: $editor_factory,
                editor_bounds: $editor_bounds,
                module_path: $module_path,
                prefix: $prefix,
                test: $test_name,
            );
        )*
    };
}

#[cfg(test)]
pub(crate) mod mock {
    use std::ops::{Deref, DerefMut};

    use crate::{
        Position,
        buffer::{self, ReadBuffer, WriteBuffer},
    };

    pub struct MockEditor;

    #[derive(Clone, PartialEq, Eq, Hash, Debug)]
    pub struct MockBuf;

    pub struct MockBufData;

    impl ReadBuffer for MockBufData {
        fn line_count(&self) -> crate::Result<usize> {
            unimplemented!()
        }
        fn get_lines<R: std::ops::RangeBounds<usize> + Send + 'static>(
            &self,
            _: R,
        ) -> crate::Result<impl Iterator<Item = String> + Send> {
            Ok(std::iter::empty::<String>())
        }
    }

    impl WriteBuffer for MockBufData {
        fn set_text(&mut self, _: &Position, _: &Position, _: &str) -> crate::Result<()> {
            unimplemented!()
        }
    }

    pub struct MockReadLock(pub MockBufData);
    impl Deref for MockReadLock {
        type Target = MockBufData;
        fn deref(&self) -> &MockBufData {
            &self.0
        }
    }
    unsafe impl Send for MockReadLock {}
    unsafe impl Sync for MockReadLock {}

    pub struct MockWriteLock(pub MockBufData);
    impl Deref for MockWriteLock {
        type Target = MockBufData;
        fn deref(&self) -> &MockBufData {
            &self.0
        }
    }
    impl DerefMut for MockWriteLock {
        fn deref_mut(&mut self) -> &mut MockBufData {
            &mut self.0
        }
    }
    unsafe impl Send for MockWriteLock {}
    unsafe impl Sync for MockWriteLock {}

    impl buffer::BufferHandle for MockBuf {
        type ReadBuffer = MockBufData;
        type WriteBuffer = MockBufData;
        type ReadBufferLock = MockReadLock;
        type WriteBufferLock = MockWriteLock;
        fn read(&self) -> crate::Result<MockReadLock> {
            unimplemented!()
        }
        fn write(&self) -> crate::Result<MockWriteLock> {
            unimplemented!()
        }
    }

    impl crate::Editor for MockEditor {
        type BufferHandle = MockBuf;
        fn current_buffer(&self) -> crate::Result<MockBuf> {
            unimplemented!()
        }
        fn set_current_buffer(&self, _: &MockBuf) -> crate::Result<()> {
            unimplemented!()
        }
        fn new_buffer(&self) -> crate::Result<MockBuf> {
            unimplemented!()
        }
        fn kill_buffer(&self, _: &MockBuf) -> crate::Result<()> {
            unimplemented!()
        }
    }

    /// A no-op key action for use in keymap equality tests.
    ///
    /// The `u32` tag makes instances distinguishable by `PartialEq`, so tests
    /// can verify that `KeyMapping::PartialEq` compares actions, not just sequences.
    #[cfg(feature = "keymap")]
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct MockAction(pub u32);

    #[cfg(feature = "keymap")]
    impl crate::keymap::KeyAction<MockEditor> for MockAction {
        fn call(&self, _: &MockEditor) -> crate::Result<()> {
            Ok(())
        }
    }
}
