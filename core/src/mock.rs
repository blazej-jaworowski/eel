use std::ops::{Deref, DerefMut};

use crate::{
    Position,
    buffer::{self, ReadBuffer, WriteBuffer},
};

#[derive(Debug)]
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
