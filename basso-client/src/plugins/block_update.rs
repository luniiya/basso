use basso_block::BlockState;
use basso_core::position::BlockPos;
use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;

use crate::{interact::BlockStatePredictionHandler, local_player::WorldHolder};

pub struct BlockUpdatePlugin;
impl Plugin for BlockUpdatePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            handle_block_update_event,
        );
    }
}

/// A component that holds the list of block updates that need to be handled.
///
/// This is updated by `read_packets` (in `PreUpdate`) and handled/cleared by
/// [`handle_block_update_event`] (`Update`).
///
/// This is a component instead of an ECS event for performance reasons.
#[derive(Clone, Component, Debug, Default)]
pub struct QueuedServerBlockUpdates {
    pub list: Vec<(BlockPos, BlockState)>,
}

pub fn handle_block_update_event(
    mut query: Query<(
        &mut QueuedServerBlockUpdates,
        &WorldHolder,
        &mut BlockStatePredictionHandler,
    )>,
) {
    for (mut queued, world_holder, mut prediction_handler) in query.iter_mut() {
        let world = world_holder.shared.read();
        for (pos, block_state) in queued.list.drain(..) {
            if !prediction_handler.update_known_server_state(pos, block_state) {
                world.chunks.set_block_state(pos, block_state);
            }
        }
    }
}
