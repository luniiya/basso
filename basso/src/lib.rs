#![doc = include_str!("../README.md")]

#[cfg(doc)]
pub mod _docs;
pub mod auto_tool;
pub mod bot;
mod client_impl;
pub mod entity_ref;
pub mod nearest_entity;
pub mod pathfinder;
pub mod prelude;
pub mod tick_broadcast;

pub use basso_block as block;
#[doc(hidden)]
#[deprecated = "moved to `basso::block`"]
pub mod blocks {
    pub type BlockStates = basso_block::BlockStates;
    pub type BlockState = basso_block::BlockState;
    pub trait BlockTrait: basso_block::BlockTrait {}
}

pub use basso_buf as buf;
pub use basso_chat::FormattedText;
pub use basso_client::*;
pub use basso_core as core;
// these are re-exported on this level because they're very common
pub use basso_core::position::{BlockPos, Vec3};
pub use basso_entity as entity;
pub use basso_physics as physics;
pub use basso_registry as registry;
#[doc(hidden)]
#[deprecated(note = "renamed to `Identifier`.")]
pub type ResourceLocation = basso_registry::identifier::Identifier;

pub mod chat {
    pub use basso_chat::*;
}

pub use basso_registry::identifier::Identifier;
pub use basso_world as world;
pub use bevy_app as app;
pub use bevy_ecs as ecs;
use bevy_ecs::resource::Resource;

pub use crate::{client_impl::Client, entity_ref::EntityRef};

/// An alias for the error type used by the client helpers.
pub mod error {
    pub use crate::client_impl::error::*;
}

// for convenience, adds the alias `basso::Result` instead of
// `basso::error::AzaleaResult`. the user should probably be using anyhow/eyre,
// but in some cases they may prefer to have the errors more strictly defined.
pub type Result<T> = error::AzaleaResult<T>;

/// A reference to a `tokio::runtime::Handle`, allowing you to spawn Tokio tasks
/// inside of ECS systems.
#[derive(Resource)]
pub struct TokioRuntimeHandle(pub tokio::runtime::Handle);
impl std::ops::Deref for TokioRuntimeHandle {
    type Target = tokio::runtime::Handle;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
