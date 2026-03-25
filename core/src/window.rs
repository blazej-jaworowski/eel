use crate::{Result, buffer::BufferHandle};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Invalid window")]
    InvalidWindow,
}

pub trait WindowId: Copy + Eq + std::fmt::Debug + Send + Sync {}

pub trait ReadWindowLock {
    type WindowId: WindowId;
    type BufferHandle: BufferHandle;

    fn current_window_id(&self) -> Result<Self::WindowId>;
    fn list_window_ids(&self) -> Result<Vec<Self::WindowId>>;
    fn get_buffer(&self, id: Self::WindowId) -> Result<Option<Self::BufferHandle>>;

    fn get_width(&self, id: Self::WindowId) -> Result<usize>;
    fn get_height(&self, id: Self::WindowId) -> Result<usize>;
}

pub trait WriteWindowLock: ReadWindowLock {
    fn new_window(&mut self, buffer: Option<&Self::BufferHandle>) -> Result<Self::WindowId>;
    fn close_window(&mut self, id: Self::WindowId) -> Result<()>;

    fn set_current(&mut self, id: Self::WindowId) -> Result<()>;

    fn set_buffer(&mut self, id: Self::WindowId, buffer: Option<&Self::BufferHandle>)
    -> Result<()>;
}

impl<L: ReadWindowLock, D: std::ops::Deref<Target = L>> ReadWindowLock for D {
    type WindowId = L::WindowId;
    type BufferHandle = L::BufferHandle;

    fn current_window_id(&self) -> Result<Self::WindowId> {
        (**self).current_window_id()
    }

    fn list_window_ids(&self) -> Result<Vec<Self::WindowId>> {
        (**self).list_window_ids()
    }

    fn get_buffer(&self, id: Self::WindowId) -> Result<Option<Self::BufferHandle>> {
        (**self).get_buffer(id)
    }

    fn get_width(&self, id: Self::WindowId) -> Result<usize> {
        (**self).get_width(id)
    }

    fn get_height(&self, id: Self::WindowId) -> Result<usize> {
        (**self).get_height(id)
    }
}

impl<L: WriteWindowLock, D: std::ops::DerefMut<Target = L>> WriteWindowLock for D {
    fn new_window(&mut self, buffer: Option<&Self::BufferHandle>) -> Result<Self::WindowId> {
        (**self).new_window(buffer)
    }

    fn close_window(&mut self, id: Self::WindowId) -> Result<()> {
        (**self).close_window(id)
    }

    fn set_current(&mut self, id: Self::WindowId) -> Result<()> {
        (**self).set_current(id)
    }

    fn set_buffer(
        &mut self,
        id: Self::WindowId,
        buffer: Option<&Self::BufferHandle>,
    ) -> Result<()> {
        (**self).set_buffer(id, buffer)
    }
}

pub trait WindowStoreHandle: Clone + std::fmt::Debug + PartialEq + Eq + Send + Sync {
    type WindowId: WindowId;
    type BufferHandle: BufferHandle;
    type ReadLock<'lock>: ReadWindowLock<WindowId = Self::WindowId, BufferHandle = Self::BufferHandle>
        + 'lock
    where
        Self: 'lock;
    type WriteLock<'lock>: WriteWindowLock<WindowId = Self::WindowId, BufferHandle = Self::BufferHandle>
        + 'lock
    where
        Self: 'lock;

    fn windows_read(&self) -> Self::ReadLock<'_>;
    fn windows_write(&self) -> Self::WriteLock<'_>;
}

pub trait WindowEditor: crate::Editor {
    type WindowStoreHandle: WindowStoreHandle<BufferHandle = Self::BufferHandle>;

    fn get_window_store(&self) -> Self::WindowStoreHandle;

    fn new_window(
        &self,
        buffer: Option<&Self::BufferHandle>,
    ) -> Result<Window<Self::WindowStoreHandle>> {
        let store = self.get_window_store();
        let id = store.windows_write().new_window(buffer)?;
        Ok(Window { id, store })
    }

    fn current_window(&self) -> Result<Window<Self::WindowStoreHandle>> {
        let store = self.get_window_store();
        let id = store.windows_read().current_window_id()?;
        Ok(Window { id, store })
    }

    fn list_windows(&self) -> Result<Vec<Window<Self::WindowStoreHandle>>> {
        let store = self.get_window_store();
        let ids = store.windows_read().list_window_ids()?;
        Ok(ids.into_iter().map(|id| Window { id, store: store.clone() }).collect())
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Window<S: WindowStoreHandle> {
    id: S::WindowId,
    store: S,
}

impl<S: WindowStoreHandle> Window<S> {
    // TODO: add a bound ensuring `L` comes from the same store as this window
    // to prevent accidentally mixing locks from different stores.
    pub fn read<L>(&self, lock: L) -> WindowAccess<L>
    where
        L: ReadWindowLock<WindowId = S::WindowId, BufferHandle = S::BufferHandle>,
    {
        WindowAccess { id: self.id, lock }
    }

    // TODO: add a bound ensuring `L` comes from the same store as this window
    // to prevent accidentally mixing locks from different stores.
    pub fn write<L>(&self, lock: L) -> WindowAccess<L>
    where
        L: WriteWindowLock<WindowId = S::WindowId, BufferHandle = S::BufferHandle>,
    {
        WindowAccess { id: self.id, lock }
    }

    pub fn lock_read(&self) -> WindowAccess<S::ReadLock<'_>> {
        WindowAccess {
            id: self.id,
            lock: self.store.windows_read(),
        }
    }

    pub fn lock_write(&self) -> WindowAccess<S::WriteLock<'_>> {
        WindowAccess {
            id: self.id,
            lock: self.store.windows_write(),
        }
    }
}

pub struct WindowAccess<L: ReadWindowLock> {
    id: L::WindowId,
    lock: L,
}

impl<L: ReadWindowLock> WindowAccess<L> {
    pub fn get_buffer(&self) -> Result<Option<L::BufferHandle>> {
        self.lock.get_buffer(self.id)
    }

    pub fn get_width(&self) -> Result<usize> {
        self.lock.get_width(self.id)
    }

    pub fn get_height(&self) -> Result<usize> {
        self.lock.get_height(self.id)
    }
}

impl<L: WriteWindowLock> WindowAccess<L> {
    pub fn set_buffer(&mut self, buffer: Option<&L::BufferHandle>) -> Result<()> {
        self.lock.set_buffer(self.id, buffer)
    }

    pub fn set_as_current(&mut self) -> Result<()> {
        self.lock.set_current(self.id)
    }

    pub fn close(mut self) -> Result<()> {
        self.lock.close_window(self.id)
    }
}

#[cfg(feature = "tests")]
pub mod tests {
    use super::*;

    pub fn test_window_new_close<E: WindowEditor>(editor: E) {
        let buffer = editor.new_buffer().expect("Failed to create buffer");
        let window = editor
            .new_window(Some(&buffer))
            .expect("Failed to create window");

        assert!(
            window.lock_read().get_width().is_ok(),
            "get_width should succeed on new window"
        );
        assert!(
            window.lock_read().get_height().is_ok(),
            "get_height should succeed on new window"
        );

        window.lock_write().close().expect("Failed to close window");
    }

    pub fn test_window_get_buffer<E: WindowEditor>(editor: E)
    where
        E::BufferHandle: std::fmt::Debug,
    {
        let buffer = editor.new_buffer().expect("Failed to create buffer");
        let window = editor
            .new_window(Some(&buffer))
            .expect("Failed to create window");

        let got = window
            .lock_read()
            .get_buffer()
            .expect("get_buffer should succeed")
            .expect("window should have a buffer");

        assert_eq!(
            got, buffer,
            "Window should contain the buffer it was opened with"
        );

        window.lock_write().close().expect("Failed to close window");
    }

    pub fn test_window_set_buffer<E: WindowEditor>(editor: E)
    where
        E::BufferHandle: std::fmt::Debug,
    {
        let buf1 = editor.new_buffer().expect("Failed to create buffer 1");
        let buf2 = editor.new_buffer().expect("Failed to create buffer 2");
        let window = editor
            .new_window(Some(&buf1))
            .expect("Failed to create window");

        window
            .lock_write()
            .set_buffer(Some(&buf2))
            .expect("Failed to set buffer");

        let got = window
            .lock_read()
            .get_buffer()
            .expect("get_buffer should succeed after set_buffer")
            .expect("window should still have a buffer");

        assert_eq!(got, buf2, "Window should show the newly set buffer");

        window.lock_write().close().expect("Failed to close window");
    }

    pub fn test_window_dimensions<E: WindowEditor>(editor: E) {
        let window = editor.new_window(None).expect("Failed to create window");

        let width = window.lock_read().get_width().expect("Failed to get width");
        let height = window
            .lock_read()
            .get_height()
            .expect("Failed to get height");

        assert!(width > 0, "Window width should be > 0");
        assert!(height > 0, "Window height should be > 0");

        window.lock_write().close().expect("Failed to close window");
    }

    pub fn test_window_current<E: WindowEditor>(editor: E) {
        let window = editor
            .current_window()
            .expect("Failed to get current window");

        assert!(
            window.lock_read().get_width().is_ok(),
            "get_width should succeed on current window"
        );
        assert!(
            window.lock_read().get_height().is_ok(),
            "get_height should succeed on current window"
        );
    }

    pub fn test_window_set_current<E: WindowEditor>(editor: E) {
        let window = editor.new_window(None).expect("Failed to create window");

        window
            .lock_write()
            .set_as_current()
            .expect("Failed to set as current");

        let current = editor
            .current_window()
            .expect("Failed to get current window");
        assert_eq!(window, current, "current_window should be the one we set");

        window.lock_write().close().expect("Failed to close window");
    }

    pub fn test_window_atomic_read<E: WindowEditor>(editor: E) {
        let win1 = editor.new_window(None).expect("Failed to create window 1");
        let win2 = editor.new_window(None).expect("Failed to create window 2");

        let store = editor.get_window_store();
        let lock = store.windows_read();
        let w1 = win1.read(&lock).get_width().expect("get_width win1");
        let w2 = win2.read(&lock).get_width().expect("get_width win2");
        drop(lock);

        assert!(w1 > 0, "win1 width should be > 0");
        assert!(w2 > 0, "win2 width should be > 0");

        win1.lock_write().close().expect("close win1");
        win2.lock_write().close().expect("close win2");
    }

    pub fn test_window_atomic_write<E: WindowEditor>(editor: E)
    where
        E::BufferHandle: std::fmt::Debug,
    {
        let buf1 = editor.new_buffer().expect("Failed to create buffer 1");
        let buf2 = editor.new_buffer().expect("Failed to create buffer 2");
        let win1 = editor
            .new_window(Some(&buf1))
            .expect("Failed to create window 1");
        let win2 = editor
            .new_window(Some(&buf2))
            .expect("Failed to create window 2");

        // Swap buffers atomically under one write lock
        let store = editor.get_window_store();
        let mut lock = store.windows_write();
        win1.write(&mut lock)
            .set_buffer(Some(&buf2))
            .expect("set_buffer win1 -> buf2");
        win2.write(&mut lock)
            .set_buffer(Some(&buf1))
            .expect("set_buffer win2 -> buf1");
        drop(lock);

        let got1 = win1
            .lock_read()
            .get_buffer()
            .expect("get_buffer win1")
            .expect("win1 should have a buffer");
        let got2 = win2
            .lock_read()
            .get_buffer()
            .expect("get_buffer win2")
            .expect("win2 should have a buffer");

        assert_eq!(got1, buf2, "win1 should now hold buf2");
        assert_eq!(got2, buf1, "win2 should now hold buf1");

        win1.lock_write().close().expect("close win1");
        win2.lock_write().close().expect("close win2");
    }

    pub fn test_window_list<E: WindowEditor>(editor: E) {
        let win1 = editor.new_window(None).expect("Failed to create window 1");
        let win2 = editor.new_window(None).expect("Failed to create window 2");

        let windows = editor.list_windows().expect("list_windows should succeed");

        assert!(
            windows.contains(&win1),
            "list_windows should contain win1"
        );
        assert!(
            windows.contains(&win2),
            "list_windows should contain win2"
        );

        win1.lock_write().close().expect("close win1");
        win2.lock_write().close().expect("close win2");
    }

    #[macro_export]
    macro_rules! eel_window_tests {
        ($test_tag:path, $editor_factory:expr, $prefix:tt) => {
            $crate::eel_tests!(
                test_tag: $test_tag,
                editor_factory: $editor_factory,
                editor_bounds: {
                    E: $crate::window::WindowEditor,
                    E::BufferHandle: ::std::fmt::Debug,
                },
                module_path: $crate::window::tests,
                prefix: $prefix,
                tests: [
                    test_window_new_close,
                    test_window_get_buffer,
                    test_window_set_buffer,
                    test_window_dimensions,
                    test_window_current,
                    test_window_set_current,
                    test_window_atomic_read,
                    test_window_atomic_write,
                    test_window_list,
                ],
            );
        };

        ($test_tag:path, $editor_factory:expr) => {
            $crate::eel_window_tests!($test_tag, $editor_factory, "");
        };
    }
}
