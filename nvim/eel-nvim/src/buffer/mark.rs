use async_trait::async_trait;
use nvim_oxi::api::opts::{GetExtmarkByIdOpts, SetExtmarkOpts};

use eel::{
    Position, Result,
    mark::{Gravity, MarkId, MarkReadBuffer, MarkWriteBuffer},
};

use crate::{editor::get_eel_namespace, error::Error as NvimError, error::IntoNvimResult as _};

use super::{NativePosition, NvimBuffer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NvimMarkId(u32);

impl From<u32> for NvimMarkId {
    fn from(value: u32) -> Self {
        NvimMarkId(value)
    }
}

impl From<NvimMarkId> for u32 {
    fn from(value: NvimMarkId) -> Self {
        value.0
    }
}

impl From<&NvimMarkId> for u32 {
    fn from(value: &NvimMarkId) -> Self {
        value.0
    }
}

impl MarkId for NvimMarkId {}

#[async_trait]
impl MarkReadBuffer for NvimBuffer {
    type MarkId = NvimMarkId;

    async fn get_mark_position(&self, id: Self::MarkId) -> Result<Position> {
        let buf = self.inner_buf();

        let (row, col, _) = self
            .dispatcher
            .dispatch(move || {
                buf.get_extmark_by_id(
                    get_eel_namespace(),
                    id.into(),
                    &GetExtmarkByIdOpts::default(),
                )
            })
            .await?
            .into_nvim()?;

        Ok(Position::new(row, col))
    }
}

#[async_trait]
impl MarkWriteBuffer for NvimBuffer {
    async fn create_mark(&mut self, pos: &Position) -> Result<NvimMarkId> {
        let native_pos: NativePosition = pos.clone().into();
        let mut buf = self.inner_buf();

        let extmark_id = self
            .dispatcher
            .dispatch(move || {
                buf.set_extmark(
                    get_eel_namespace(),
                    native_pos.row - 1,
                    native_pos.col - 1,
                    &SetExtmarkOpts::default(),
                )
            })
            .await?
            .into_nvim()?;

        Ok(extmark_id.into())
    }

    async fn destroy_mark(&mut self, id: Self::MarkId) -> Result<()> {
        // TODO: Return specific Error::Destroyed error when accessing destroyed mark

        let mut buf = self.inner_buf();

        self.dispatcher
            .dispatch(move || buf.del_extmark(get_eel_namespace(), id.into()))
            .await?
            .into_nvim()?;

        Ok(())
    }
    async fn set_mark_position(&mut self, id: Self::MarkId, pos: &Position) -> Result<()> {
        let native_pos: NativePosition = pos.clone().into();
        let mut buf = self.inner_buf();

        self.dispatcher
            .dispatch(move || {
                buf.set_extmark(
                    get_eel_namespace(),
                    native_pos.row - 1,
                    native_pos.col - 1,
                    &SetExtmarkOpts::builder().id(id.into()).build(),
                )
            })
            .await?
            .into_nvim()?;

        Ok(())
    }

    async fn set_mark_gravity(&mut self, id: Self::MarkId, gravity: Gravity) -> Result<()> {
        let mut buf = self.inner_buf();

        let pos = self.get_mark_position(id).await?;

        self.dispatcher
            .dispatch(move || {
                // TODO: In my opinion you shouldn't have to delete an extmark and create a new one to change options,
                //       but it doesn't work otherwise. Should investigate.
                buf.del_extmark(get_eel_namespace(), id.into())?;

                buf.set_extmark(
                    get_eel_namespace(),
                    pos.row,
                    pos.col,
                    &SetExtmarkOpts::builder()
                        .id(id.into())
                        .right_gravity(match gravity {
                            Gravity::Left => false,
                            Gravity::Right => true,
                        })
                        .build(),
                )?;

                Ok::<_, NvimError>(())
            })
            .await??;

        Ok(())
    }
}
