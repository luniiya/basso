//! Outgoing "packet" types for the parts of the client that still simulate
//! player actions.
//!
//! basso doesn't do networking, so these are plain data structures instead of
//! protocol packets. The Horizon integration listens for [`SendGamePacketEvent`]
//! and applies the actions to the real game.

use basso_core::{
    direction::Direction,
    entity_id::MinecraftEntityId,
    hit_result::BlockHitResult,
    position::{BlockPos, Vec3},
};
use basso_entity::LookDirection;
use basso_inventory::{
    ItemStack,
    operations::ClickType,
};
use bevy_ecs::prelude::*;
use indexmap::IndexMap;

/// An event that's triggered when the client wants to perform an action in the
/// world (moving, mining, interacting, etc).
///
/// The Horizon side consumes this event and translates it into the actual
/// game actions.
#[derive(Clone, Debug, EntityEvent)]
pub struct SendGamePacketEvent {
    #[event_target]
    pub sent_by: Entity,
    pub packet: ServerboundGamePacket,
}
impl SendGamePacketEvent {
    pub fn new(sent_by: Entity, packet: impl Into<ServerboundGamePacket>) -> Self {
        Self {
            sent_by,
            packet: packet.into(),
        }
    }
}

/// Every action that a local player can send out.
#[derive(Clone, Debug)]
pub enum ServerboundGamePacket {
    MovePlayerPosRot(ServerboundMovePlayerPosRot),
    MovePlayerPos(ServerboundMovePlayerPos),
    MovePlayerRot(ServerboundMovePlayerRot),
    MovePlayerStatusOnly(ServerboundMovePlayerStatusOnly),
    PlayerInput(ServerboundPlayerInput),
    PlayerCommand(ServerboundPlayerCommand),
    PlayerAction(ServerboundPlayerAction),
    Interact(ServerboundInteract),
    UseItem(ServerboundUseItem),
    UseItemOn(ServerboundUseItemOn),
    Swing(ServerboundSwing),
    Attack(ServerboundAttack),
    ContainerClick(ServerboundContainerClick),
    ContainerClose(ServerboundContainerClose),
    SetCarriedItem(ServerboundSetCarriedItem),
    PlayerLoaded(ServerboundPlayerLoaded),
}

macro_rules! impl_from_packet {
    ($($variant:ident => $ty:ident),* $(,)?) => {
        $(
            impl From<$ty> for ServerboundGamePacket {
                fn from(packet: $ty) -> Self {
                    ServerboundGamePacket::$variant(packet)
                }
            }
        )*
    };
}

impl_from_packet!(
    MovePlayerPosRot => ServerboundMovePlayerPosRot,
    MovePlayerPos => ServerboundMovePlayerPos,
    MovePlayerRot => ServerboundMovePlayerRot,
    MovePlayerStatusOnly => ServerboundMovePlayerStatusOnly,
    PlayerInput => ServerboundPlayerInput,
    PlayerCommand => ServerboundPlayerCommand,
    PlayerAction => ServerboundPlayerAction,
    Interact => ServerboundInteract,
    UseItem => ServerboundUseItem,
    UseItemOn => ServerboundUseItemOn,
    Swing => ServerboundSwing,
    Attack => ServerboundAttack,
    ContainerClick => ServerboundContainerClick,
    ContainerClose => ServerboundContainerClose,
    SetCarriedItem => ServerboundSetCarriedItem,
    PlayerLoaded => ServerboundPlayerLoaded,
);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MoveFlags {
    pub on_ground: bool,
    pub horizontal_collision: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct ServerboundMovePlayerPosRot {
    pub pos: Vec3,
    pub look_direction: LookDirection,
    pub flags: MoveFlags,
}

#[derive(Clone, Copy, Debug)]
pub struct ServerboundMovePlayerPos {
    pub pos: Vec3,
    pub flags: MoveFlags,
}

#[derive(Clone, Copy, Debug)]
pub struct ServerboundMovePlayerRot {
    pub look_direction: LookDirection,
    pub flags: MoveFlags,
}

#[derive(Clone, Copy, Debug)]
pub struct ServerboundMovePlayerStatusOnly {
    pub flags: MoveFlags,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ServerboundPlayerInput {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub shift: bool,
    pub sprint: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServerboundPlayerCommand {
    pub id: MinecraftEntityId,
    pub action: PlayerCommandAction,
    pub data: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerCommandAction {
    StartSprinting,
    StopSprinting,
    StartFallFlying,
}

#[derive(Clone, Copy, Debug)]
pub struct ServerboundPlayerAction {
    pub action: PlayerActionType,
    pub pos: BlockPos,
    pub direction: Direction,
    pub seq: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerActionType {
    StartDestroyBlock,
    StopDestroyBlock,
    AbortDestroyBlock,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InteractionHand {
    #[default]
    MainHand,
    OffHand,
}

#[derive(Clone, Debug)]
pub struct ServerboundInteract {
    pub entity_id: MinecraftEntityId,
    pub hand: InteractionHand,
    pub location: Vec3,
    pub using_secondary_action: bool,
}

#[derive(Clone, Debug)]
pub struct ServerboundUseItem {
    pub hand: InteractionHand,
    pub seq: u32,
    pub x_rot: f32,
    pub y_rot: f32,
}

#[derive(Clone, Debug)]
pub struct ServerboundUseItemOn {
    pub hand: InteractionHand,
    pub block_hit: BlockHitResult,
    pub seq: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct ServerboundSwing {
    pub hand: InteractionHand,
}

#[derive(Clone, Copy, Debug)]
pub struct ServerboundAttack {
    pub entity_id: MinecraftEntityId,
}

#[derive(Clone, Debug)]
pub struct ServerboundContainerClick {
    pub container_id: i32,
    pub state_id: i32,
    pub slot_num: i16,
    pub button_num: u8,
    pub click_type: ClickType,
    pub changed_slots: IndexMap<u16, ItemStack>,
    pub carried_item: ItemStack,
}

#[derive(Clone, Debug)]
pub struct ServerboundContainerClose {
    pub container_id: i32,
}

#[derive(Clone, Debug)]
pub struct ServerboundSetCarriedItem {
    pub slot: u16,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ServerboundPlayerLoaded;
