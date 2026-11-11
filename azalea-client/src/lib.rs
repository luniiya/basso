#![doc = include_str!("../README.md")]
#![allow(clippy::type_complexity, reason = "Bevy Query types")]

pub mod client_chat;
mod client;
pub mod local_player;
pub mod packet;
pub mod player;
mod plugins;

pub use azalea_physics::client_movement::{ClientMovementState, SprintDirection, WalkDirection};
#[deprecated = "renamed to `ClientMovementState`."]
pub type PhysicsState = ClientMovementState;

// Re-export bevy-tasks so plugins can make sure that they're using the same
// version.
pub use bevy_tasks;
pub use client::{
    InGameState, JoinedClientBundle, LocalPlayerBundle, start_ecs_runner,
};
pub use movement::{StartSprintEvent, StartWalkEvent};
pub use packet::{InteractionHand, SendGamePacketEvent, ServerboundGamePacket};
pub use plugins::*;
