use azalea_chat::FormattedText;
use azalea_core::game_type::GameMode;
use bevy_ecs::component::Component;
use derive_more::{Deref, DerefMut};
use uuid::Uuid;

use azalea_inventory::game_profile::GameProfile;

/// A player in the tab list.
#[derive(Clone, Debug)]
pub struct PlayerInfo {
    /// Information about the player's Minecraft account, including their
    /// username.
    pub profile: GameProfile,
    /// The player's UUID.
    pub uuid: Uuid,
    /// The current gamemode of the player, like survival or creative.
    pub gamemode: GameMode,
    /// The player's latency in milliseconds.
    ///
    /// The bars in the tab screen depend on this.
    pub latency: i32,
    /// The player's display name in the tab list, but only if it's different
    /// from the player's normal username.
    ///
    /// Use [`GameProfile::name`] to get the player's actual username.
    pub display_name: Option<Box<FormattedText>>,
}

/// A component only present in players that contains the [`GameProfile`] (which
/// you can use to get a player's name).
///
/// Note that it's possible for this to be missing in a player if the server
/// never sent the player info for them (though this is uncommon).
#[derive(Clone, Component, Debug, Deref, DerefMut)]
pub struct GameProfileComponent(pub GameProfile);
