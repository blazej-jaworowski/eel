use crate::{Result, buffer::BufferHandle};

#[derive(thiserror::Error, Debug, PartialEq)]
pub enum Error {
    #[error("Invalid window")]
    InvalidWindow,

    #[error("Cannot split a floating window")]
    CannotSplitFloat,

    #[error("Invalid split_at: {split_at} (must be between 1 and {limit})")]
    InvalidSplitAt { split_at: usize, limit: usize },

    #[error("Floating window is out of bounds")]
    FloatOutOfBounds,
}

pub trait WindowId: Copy + Eq + std::fmt::Debug + Send + Sync {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitDirection {
    Above,
    Below,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitConfig<W> {
    pub direction: SplitDirection,
    pub window: W,
    pub split_at: usize,
}

impl<S: WindowStoreHandle> SplitConfig<Window<S>> {
    pub fn with_id(self) -> SplitConfig<S::WindowId> {
        SplitConfig {
            direction: self.direction,
            window: self.window.id,
            split_at: self.split_at,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowPosition {
    pub row: usize,
    pub col: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowDimensions {
    pub width: usize,
    pub height: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FloatConfig {
    pub position: WindowPosition,
    pub dimensions: WindowDimensions,
    pub focusable: bool,
    pub z_index: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowOpenConfig<W> {
    Split(SplitConfig<W>),
    Float(FloatConfig),
}

pub trait ReadWindowLock {
    type WindowId: WindowId;
    type BufferHandle: BufferHandle;

    fn current_window_id(&self) -> Result<Self::WindowId>;
    fn list_window_ids(&self) -> Result<Vec<Self::WindowId>>;
    fn get_buffer(&self, id: Self::WindowId) -> Result<Option<Self::BufferHandle>>;

    fn get_dimensions(&self, id: Self::WindowId) -> Result<WindowDimensions>;
    fn get_position(&self, id: Self::WindowId) -> Result<WindowPosition>;
    fn is_floating(&self, id: Self::WindowId) -> Result<bool>;
}

pub trait WriteWindowLock: ReadWindowLock {
    fn new_window(
        &mut self,
        buffer: Option<&Self::BufferHandle>,
        config: WindowOpenConfig<Self::WindowId>,
    ) -> Result<Self::WindowId>;
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

    fn get_dimensions(&self, id: Self::WindowId) -> Result<WindowDimensions> {
        (**self).get_dimensions(id)
    }

    fn get_position(&self, id: Self::WindowId) -> Result<WindowPosition> {
        (**self).get_position(id)
    }

    fn is_floating(&self, id: Self::WindowId) -> Result<bool> {
        (**self).is_floating(id)
    }
}

impl<L: WriteWindowLock, D: std::ops::DerefMut<Target = L>> WriteWindowLock for D {
    fn new_window(
        &mut self,
        buffer: Option<&Self::BufferHandle>,
        config: WindowOpenConfig<Self::WindowId>,
    ) -> Result<Self::WindowId> {
        (**self).new_window(buffer, config)
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

    fn new_split_window(
        &self,
        config: SplitConfig<Window<Self::WindowStoreHandle>>,
        buffer: Option<&Self::BufferHandle>,
    ) -> Result<Window<Self::WindowStoreHandle>> {
        let store = self.get_window_store();
        let id = store
            .windows_write()
            .new_window(buffer, WindowOpenConfig::Split(config.with_id()))?;
        Ok(Window { id, store })
    }

    fn new_float_window(
        &self,
        config: FloatConfig,
        buffer: Option<&Self::BufferHandle>,
    ) -> Result<Window<Self::WindowStoreHandle>> {
        let store = self.get_window_store();
        let id = store
            .windows_write()
            .new_window(buffer, WindowOpenConfig::Float(config))?;
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
        Ok(ids
            .into_iter()
            .map(|id| Window {
                id,
                store: store.clone(),
            })
            .collect())
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

    pub fn get_dimensions(&self) -> Result<WindowDimensions> {
        self.lock.get_dimensions(self.id)
    }

    pub fn get_position(&self) -> Result<WindowPosition> {
        self.lock.get_position(self.id)
    }

    pub fn is_floating(&self) -> Result<bool> {
        self.lock.is_floating(self.id)
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

    fn new_window<E: WindowEditor>(
        editor: &E,
        buffer: Option<&E::BufferHandle>,
    ) -> Window<E::WindowStoreHandle> {
        editor
            .new_float_window(
                FloatConfig {
                    position: WindowPosition { row: 1, col: 1 },
                    dimensions: WindowDimensions {
                        width: 20,
                        height: 10,
                    },
                    focusable: true,
                    z_index: 50,
                },
                buffer,
            )
            .expect("new_float_window failed")
    }

    pub fn test_window_new_close<E: WindowEditor>(editor: E) {
        let buffer = editor.new_buffer().expect("Failed to create buffer");
        let window = new_window(&editor, Some(&buffer));

        assert!(
            window.lock_read().get_dimensions().is_ok(),
            "get_dimensions should succeed on new window"
        );

        window.lock_write().close().expect("Failed to close window");
    }

    pub fn test_window_get_buffer<E: WindowEditor>(editor: E)
    where
        E::BufferHandle: std::fmt::Debug,
    {
        let buffer = editor.new_buffer().expect("Failed to create buffer");
        let window = new_window(&editor, Some(&buffer));

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
        let window = new_window(&editor, Some(&buf1));

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
        let window = new_window(&editor, None);

        let dims = window
            .lock_read()
            .get_dimensions()
            .expect("Failed to get dimensions");

        assert!(dims.width > 0, "Window width should be > 0");
        assert!(dims.height > 0, "Window height should be > 0");

        window.lock_write().close().expect("Failed to close window");
    }

    pub fn test_window_current<E: WindowEditor>(editor: E) {
        let window = editor
            .current_window()
            .expect("Failed to get current window");

        assert!(
            window.lock_read().get_dimensions().is_ok(),
            "get_dimensions should succeed on current window"
        );
    }

    pub fn test_window_set_current<E: WindowEditor>(editor: E) {
        let window = new_window(&editor, None);

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
        let win1 = new_window(&editor, None);
        let win2 = new_window(&editor, None);

        let store = editor.get_window_store();
        let lock = store.windows_read();
        let dims1 = win1
            .read(&lock)
            .get_dimensions()
            .expect("get_dimensions win1");
        let dims2 = win2
            .read(&lock)
            .get_dimensions()
            .expect("get_dimensions win2");
        drop(lock);

        assert!(dims1.width > 0, "win1 width should be > 0");
        assert!(dims2.width > 0, "win2 width should be > 0");

        win1.lock_write().close().expect("close win1");
        win2.lock_write().close().expect("close win2");
    }

    pub fn test_window_atomic_write<E: WindowEditor>(editor: E)
    where
        E::BufferHandle: std::fmt::Debug,
    {
        let buf1 = editor.new_buffer().expect("Failed to create buffer 1");
        let buf2 = editor.new_buffer().expect("Failed to create buffer 2");
        let win1 = new_window(&editor, Some(&buf1));
        let win2 = new_window(&editor, Some(&buf2));

        // Swap buffers atomically under one write lock
        let store = editor.get_window_store();
        let mut lock = store.windows_write();
        win1.write(&mut lock)
            .set_buffer(Some(&buf2))
            .expect("set_buffer win1 -> buf2");
        win2.write(&mut lock)
            .set_buffer(Some(&buf1))
            .expect("set_buffer win2 -> buf1");

        // Read back while still holding the write lock
        let got1_under_lock = win1
            .read(&lock)
            .get_buffer()
            .expect("get_buffer win1 under write lock")
            .expect("win1 should have a buffer under write lock");
        let got2_under_lock = win2
            .read(&lock)
            .get_buffer()
            .expect("get_buffer win2 under write lock")
            .expect("win2 should have a buffer under write lock");
        assert_eq!(
            got1_under_lock, buf2,
            "win1 should hold buf2 (read under write lock)"
        );
        assert_eq!(
            got2_under_lock, buf1,
            "win2 should hold buf1 (read under write lock)"
        );
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
        let win1 = new_window(&editor, None);
        let win2 = new_window(&editor, None);

        let windows = editor.list_windows().expect("list_windows should succeed");

        assert!(windows.contains(&win1), "list_windows should contain win1");
        assert!(windows.contains(&win2), "list_windows should contain win2");

        win1.lock_write().close().expect("close win1");
        win2.lock_write().close().expect("close win2");
    }

    pub fn test_window_get_position<E: WindowEditor>(editor: E) {
        let window = new_window(&editor, None);

        window
            .lock_read()
            .get_position()
            .expect("get_position should succeed on a split window");

        window.lock_write().close().expect("Failed to close window");
    }

    pub fn test_window_split_above<E: WindowEditor>(editor: E) {
        let current = editor.current_window().expect("get current window");
        let win = editor
            .new_split_window(
                SplitConfig {
                    direction: SplitDirection::Above,
                    window: current.clone(),
                    split_at: 5,
                },
                None,
            )
            .expect("Failed to create split-above window");

        let dims = win
            .lock_read()
            .get_dimensions()
            .expect("Failed to get dimensions");
        assert_eq!(
            dims.height, 5,
            "Split-above window height should match split_at"
        );

        let win_pos = win
            .lock_read()
            .get_position()
            .expect("Failed to get new window position");
        let cur_pos = current
            .lock_read()
            .get_position()
            .expect("Failed to get current window position");
        assert!(
            win_pos.row < cur_pos.row,
            "Split-above window should be above current"
        );

        win.lock_write().close().expect("Failed to close window");
    }

    pub fn test_window_split_below<E: WindowEditor>(editor: E) {
        let current = editor.current_window().expect("get current window");
        let win = editor
            .new_split_window(
                SplitConfig {
                    direction: SplitDirection::Below,
                    window: current.clone(),
                    split_at: 5,
                },
                None,
            )
            .expect("Failed to create split-below window");

        let dims = win
            .lock_read()
            .get_dimensions()
            .expect("Failed to get dimensions");
        assert_eq!(
            dims.height, 5,
            "Split-below window height should match split_at"
        );

        let win_pos = win
            .lock_read()
            .get_position()
            .expect("Failed to get new window position");
        let cur_pos = current
            .lock_read()
            .get_position()
            .expect("Failed to get current window position");
        assert!(
            win_pos.row > cur_pos.row,
            "Split-below window should be below current"
        );

        win.lock_write().close().expect("Failed to close window");
    }

    pub fn test_window_split_left<E: WindowEditor>(editor: E) {
        let current = editor.current_window().expect("get current window");
        let win = editor
            .new_split_window(
                SplitConfig {
                    direction: SplitDirection::Left,
                    window: current.clone(),
                    split_at: 5,
                },
                None,
            )
            .expect("Failed to create split-left window");

        let dims = win
            .lock_read()
            .get_dimensions()
            .expect("Failed to get dimensions");
        assert_eq!(
            dims.width, 5,
            "Split-left window width should match split_at"
        );

        let win_pos = win
            .lock_read()
            .get_position()
            .expect("Failed to get new window position");
        let cur_pos = current
            .lock_read()
            .get_position()
            .expect("Failed to get current window position");
        assert!(
            win_pos.col < cur_pos.col,
            "Split-left window should be left of current"
        );

        win.lock_write().close().expect("Failed to close window");
    }

    pub fn test_window_split_right<E: WindowEditor>(editor: E) {
        let current = editor.current_window().expect("get current window");
        let win = editor
            .new_split_window(
                SplitConfig {
                    direction: SplitDirection::Right,
                    window: current.clone(),
                    split_at: 5,
                },
                None,
            )
            .expect("Failed to create split-right window");

        let dims = win
            .lock_read()
            .get_dimensions()
            .expect("Failed to get dimensions");
        assert_eq!(
            dims.width, 5,
            "Split-right window width should match split_at"
        );

        let win_pos = win
            .lock_read()
            .get_position()
            .expect("Failed to get new window position");
        let cur_pos = current
            .lock_read()
            .get_position()
            .expect("Failed to get current window position");
        assert!(
            win_pos.col > cur_pos.col,
            "Split-right window should be right of current"
        );

        win.lock_write().close().expect("Failed to close window");
    }

    pub fn test_window_split<E: WindowEditor>(editor: E) {
        let current = editor.current_window().expect("get current window");
        let win1 = editor
            .new_split_window(
                SplitConfig {
                    direction: SplitDirection::Below,
                    window: current,
                    split_at: 10,
                },
                None,
            )
            .expect("Failed to create win1");

        let win2 = editor
            .new_split_window(
                SplitConfig {
                    direction: SplitDirection::Right,
                    window: win1.clone(),
                    split_at: 10,
                },
                None,
            )
            .expect("Failed to split win1");

        let windows = editor.list_windows().expect("list_windows should succeed");
        assert!(windows.contains(&win1), "list_windows should contain win1");
        assert!(windows.contains(&win2), "list_windows should contain win2");

        win2.lock_write().close().expect("Failed to close win2");
        win1.lock_write().close().expect("Failed to close win1");
    }

    pub fn test_window_float<E: WindowEditor>(editor: E) {
        let config = FloatConfig {
            position: WindowPosition { row: 1, col: 2 },
            dimensions: WindowDimensions {
                width: 20,
                height: 10,
            },
            focusable: true,
            z_index: 50,
        };

        let win = editor
            .new_float_window(config, None)
            .expect("Failed to create floating window");

        let dims = win
            .lock_read()
            .get_dimensions()
            .expect("Failed to get float dimensions");
        assert_eq!(dims.width, 20, "Float window width should match config");
        assert_eq!(dims.height, 10, "Float window height should match config");

        let pos = win
            .lock_read()
            .get_position()
            .expect("get_position should succeed on a float window");
        assert_eq!(
            pos,
            WindowPosition { row: 1, col: 2 },
            "Float window position should match config"
        );

        let windows = editor.list_windows().expect("list_windows should succeed");
        assert!(
            windows.contains(&win),
            "list_windows should contain float window"
        );

        win.lock_write()
            .close()
            .expect("Failed to close float window");
    }

    pub fn test_window_is_floating<E: WindowEditor>(editor: E) {
        let float_win = new_window(&editor, None);
        let current = editor.current_window().expect("get current window");
        let split_win = editor
            .new_split_window(
                SplitConfig {
                    direction: SplitDirection::Below,
                    window: current,
                    split_at: 5,
                },
                None,
            )
            .expect("create split window");

        assert!(
            float_win
                .lock_read()
                .is_floating()
                .expect("is_floating on float"),
            "float window should report is_floating = true"
        );
        assert!(
            !split_win
                .lock_read()
                .is_floating()
                .expect("is_floating on split"),
            "split window should report is_floating = false"
        );

        float_win.lock_write().close().expect("close float");
        split_win.lock_write().close().expect("close split");
    }

    pub fn test_window_invalid_id<E: WindowEditor>(editor: E) {
        let win = new_window(&editor, None);
        win.lock_write().close().expect("close window");
        let result = win.lock_read().is_floating();
        assert!(
            matches!(result, Err(crate::Error::Window(Error::InvalidWindow))),
            "expected InvalidWindow after close, got {result:?}"
        );
    }

    pub fn test_window_split_float_error<E: WindowEditor>(editor: E) {
        let float_win = new_window(&editor, None);
        let result = editor.new_split_window(
            SplitConfig {
                direction: SplitDirection::Below,
                window: float_win.clone(),
                split_at: 5,
            },
            None,
        );
        assert!(
            matches!(result, Err(crate::Error::Window(Error::CannotSplitFloat))),
            "expected CannotSplitFloat, got {result:?}"
        );
        float_win.lock_write().close().expect("close float");
    }

    pub fn test_window_split_at_zero<E: WindowEditor>(editor: E) {
        let current = editor.current_window().expect("get current window");
        let height = current
            .lock_read()
            .get_dimensions()
            .expect("get dims")
            .height;
        let result = editor.new_split_window(
            SplitConfig {
                direction: SplitDirection::Below,
                window: current,
                split_at: 0,
            },
            None,
        );
        let crate::Error::Window(err) = result.unwrap_err() else {
            panic!("expected Window error for split_at=0");
        };
        assert_eq!(
            err,
            Error::InvalidSplitAt {
                split_at: 0,
                limit: height - 1
            }
        );
    }

    pub fn test_window_split_at_full<E: WindowEditor>(editor: E) {
        let current = editor.current_window().expect("get current window");
        let height = current
            .lock_read()
            .get_dimensions()
            .expect("get dims")
            .height;
        let result = editor.new_split_window(
            SplitConfig {
                direction: SplitDirection::Below,
                window: current,
                split_at: height,
            },
            None,
        );
        let crate::Error::Window(err) = result.unwrap_err() else {
            panic!("expected Window error for split_at=full height");
        };
        assert_eq!(
            err,
            Error::InvalidSplitAt {
                split_at: height,
                limit: height - 1
            }
        );
    }

    pub fn test_window_float_out_of_bounds<E: WindowEditor>(editor: E) {
        let result = editor.new_float_window(
            FloatConfig {
                position: WindowPosition { row: 0, col: 0 },
                dimensions: WindowDimensions {
                    width: usize::MAX,
                    height: usize::MAX,
                },
                focusable: false,
                z_index: 1,
            },
            None,
        );
        assert!(
            matches!(result, Err(crate::Error::Window(Error::FloatOutOfBounds))),
            "expected FloatOutOfBounds, got {result:?}"
        );
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
                    test_window_get_position,
                    test_window_split_above,
                    test_window_split_below,
                    test_window_split_left,
                    test_window_split_right,
                    test_window_split,
                    test_window_float,
                    test_window_is_floating,
                    test_window_invalid_id,
                    test_window_split_float_error,
                    test_window_split_at_zero,
                    test_window_split_at_full,
                    test_window_float_out_of_bounds,
                ],
            );
        };

        ($test_tag:path, $editor_factory:expr) => {
            $crate::eel_window_tests!($test_tag, $editor_factory, "");
        };
    }
}
