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

pub use azalea_block as block;
#[doc(hidden)]
#[deprecated = "moved to `azalea::block`"]
pub mod blocks {
    pub type BlockStates = azalea_block::BlockStates;
    pub type BlockState = azalea_block::BlockState;
    pub trait BlockTrait: azalea_block::BlockTrait {}
}

pub use azalea_buf as buf;
pub use azalea_chat::FormattedText;
pub use azalea_client::*;
pub use azalea_core as core;
// these are re-exported on this level because they're very common
pub use azalea_core::position::{BlockPos, Vec3};
pub use azalea_entity as entity;
pub use azalea_physics as physics;
pub use azalea_registry as registry;
#[doc(hidden)]
#[deprecated(note = "renamed to `Identifier`.")]
pub type ResourceLocation = azalea_registry::identifier::Identifier;

pub mod chat {
    pub use azalea_chat::*;
}

pub use azalea_registry::identifier::Identifier;
pub use azalea_world as world;
pub use bevy_app as app;
pub use bevy_ecs as ecs;
use bevy_ecs::resource::Resource;

pub use crate::{client_impl::Client, entity_ref::EntityRef};

/// An alias for the error type used by the client helpers.
pub mod error {
    pub use crate::client_impl::error::*;
}

// for convenience, adds the alias `azalea::Result` instead of
// `azalea::error::AzaleaResult`. the user should probably be using anyhow/eyre,
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
