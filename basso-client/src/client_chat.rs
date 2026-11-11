//! Chat support.
//!
//! Sending chat messages is handled by the Horizon integration, which should
//! listen for [`SendChatEvent`].

use bevy_ecs::prelude::*;

/// A message that asks the client to send a chat message or command.
///
/// If the content starts with a `/`, it's a command.
#[derive(Clone, Debug, Message)]
pub struct SendChatEvent {
    pub entity: Entity,
    pub content: String,
}
