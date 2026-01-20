use crate::buffer::{BufferHandle, ReadBuffer, WriteBuffer};

#[cfg(feature = "cursor")]
mod cursor {
    use crate::cursor::{CursorReadBuffer, CursorWriteBuffer};

    pub trait CursorReadRequirement: CursorReadBuffer {}
    impl<T: CursorReadBuffer> CursorReadRequirement for T {}

    pub trait CursorWriteRequirement: CursorWriteBuffer {}
    impl<T: CursorWriteBuffer> CursorWriteRequirement for T {}
}

#[cfg(not(feature = "cursor"))]
mod cursor {
    pub trait CursorReadRequirement {}
    impl<T> CursorReadRequirement for T {}

    pub trait CursorWriteRequirement {}
    impl<T> CursorWriteRequirement for T {}
}

#[cfg(feature = "mark")]
mod mark {
    use crate::mark::{MarkId, MarkReadBuffer, MarkWriteBuffer};

    pub trait MarkReadRequirement: MarkReadBuffer<MarkId = Self::RMarkId> {
        type RMarkId: MarkId;
    }
    impl<T: MarkReadBuffer> MarkReadRequirement for T {
        type RMarkId = T::MarkId;
    }

    pub trait MarkWriteRequirement: MarkWriteBuffer<MarkId = Self::RMarkId> {
        type RMarkId: MarkId;
    }
    impl<T: MarkWriteBuffer> MarkWriteRequirement for T {
        type RMarkId = T::MarkId;
    }
}

#[cfg(not(feature = "mark"))]
mod mark {
    pub trait MarkReadRequirement {
        type RMarkId;
    }
    impl<T> MarkReadRequirement for T {
        type RMarkId = ();
    }

    pub trait MarkWriteRequirement {
        type RMarkId;
    }
    impl<T> MarkWriteRequirement for T {
        type RMarkId = ();
    }
}

pub trait CompleteBufferHandle:
    BufferHandle<ReadBuffer = Self::CompleteReadBuffer, WriteBuffer = Self::CompleteWriteBuffer>
{
    type CompleteReadBuffer: ReadBuffer
        + mark::MarkReadRequirement<RMarkId = Self::MarkId>
        + cursor::CursorReadRequirement;

    type CompleteWriteBuffer: WriteBuffer
        + mark::MarkWriteRequirement<RMarkId = Self::MarkId>
        + cursor::CursorWriteRequirement;

    type MarkId;
}

impl<B> CompleteBufferHandle for B
where
    B: BufferHandle,
    B::ReadBuffer: ReadBuffer + mark::MarkReadRequirement + cursor::CursorReadRequirement,
    B::WriteBuffer: WriteBuffer
        + mark::MarkWriteRequirement<RMarkId = <B::ReadBuffer as mark::MarkReadRequirement>::RMarkId>
        + cursor::CursorWriteRequirement,
{
    type CompleteReadBuffer = B::ReadBuffer;
    type CompleteWriteBuffer = B::WriteBuffer;

    type MarkId = <B::ReadBuffer as mark::MarkReadRequirement>::RMarkId;
}

mod static_tests {
    use crate::{CompleteBufferHandle, buffer::BufferHandle};

    fn _test_buffer_trait() {
        fn _check_trait<B>(_: B)
        where
            B: BufferHandle,
        {
        }

        fn _static_check<B>(buffer: B)
        where
            B: CompleteBufferHandle,
        {
            _check_trait(buffer);
        }
    }

    #[cfg(feature = "mark")]
    fn _test_mark_trait() {
        use crate::mark::{MarkReadBuffer, MarkWriteBuffer};

        fn _check_trait<B>(_: B)
        where
            B: BufferHandle,
            B::ReadBuffer: MarkReadBuffer,
            B::WriteBuffer: MarkWriteBuffer,
        {
        }

        fn _static_check<B>(buffer: B)
        where
            B: CompleteBufferHandle,
        {
            _check_trait(buffer);
        }
    }

    #[cfg(feature = "cursor")]
    fn _test_cursor_trait() {
        use crate::cursor::{CursorReadBuffer, CursorWriteBuffer};

        fn _check_trait<B>(_: B)
        where
            B: BufferHandle,
            B::ReadBuffer: CursorReadBuffer,
            B::WriteBuffer: CursorWriteBuffer,
        {
        }

        fn _static_check<B>(buffer: B)
        where
            B: CompleteBufferHandle,
        {
            _check_trait(buffer);
        }
    }
}
