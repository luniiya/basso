//! Long-running orders and the systems that execute them.
//!
//! `goto` is handled entirely by the pathfinder, but going to a block, mining
//! and following need to react to the world and reissue goals, which is what
//! this module does.

use std::sync::Arc;

use basso_block::BlockStates;
use basso_client::{
    local_player::TabList,
    mining::{Mining, MiningQueued, StartMiningBlockEvent, StopMiningBlockEvent},
};
use basso_core::{
    position::{BlockPos, Vec3},
    tick::GameTick,
};
use basso_entity::{EntityKindComponent, EntityUuid, LocalEntity, Position, metadata::Player};
use basso_registry::builtin::{BlockKind, EntityKind};
use basso_world::{WorldName, Worlds};
use bevy_app::{Plugin, Update};
use bevy_ecs::prelude::*;

use super::{CommandOutput, MAX_BLOCK_DISTANCE};
use crate::pathfinder::{
    ComputePath, ExecutingPath, GotoEvent, Pathfinder, PathfinderOpts,
    goals::{Goal, RadiusGoal, ReachBlockPosGoal},
};

/// Whether a [`TargetBlockProcess`] is going to a block or mining it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetBlockGoal {
    Goto,
    Mine,
}

/// An order that repeatedly finds and walks to blocks.
///
/// Used for `goto <block>` (`TargetBlockGoal::Goto`) and `mine`
/// (`TargetBlockGoal::Mine`).
#[derive(Clone, Debug, Component)]
pub struct TargetBlockProcess {
    pub block_states: BlockStates,
    pub goal: TargetBlockGoal,
    /// Stop after mining this many blocks. `None` means keep going.
    pub remaining: Option<u32>,
    pub mined: u32,
    /// The block we're currently going to or mining.
    pub target: Option<BlockPos>,
    pub last_search_tick: u32,
}

impl TargetBlockProcess {
    /// Mine the given blocks, optionally stopping after `remaining` mined.
    pub fn mine(block_states: BlockStates, remaining: Option<u32>) -> Self {
        Self {
            block_states,
            goal: TargetBlockGoal::Mine,
            remaining,
            mined: 0,
            target: None,
            last_search_tick: 0,
        }
    }

    /// Go to the nearest instance of one block kind.
    pub fn goto(kind: BlockKind) -> Self {
        Self {
            block_states: BlockStates::from(&[kind][..]),
            goal: TargetBlockGoal::Goto,
            remaining: None,
            mined: 0,
            target: None,
            last_search_tick: 0,
        }
    }
}

/// What a [`FollowProcess`] is following.
#[derive(Clone, Debug, PartialEq)]
pub enum FollowTarget {
    /// All living entities, including players.
    Entities,
    /// All players in the tab list.
    Players,
    /// Living entities of these kinds.
    EntityKinds(Vec<EntityKind>),
    /// Players with these usernames.
    PlayerNames(Vec<String>),
}

/// An order that follows entities or players.
#[derive(Clone, Debug, Component)]
pub struct FollowProcess {
    pub target: FollowTarget,
    /// The position the last issued goal was for.
    pub last_goal: Option<Vec3>,
    pub last_recalc_tick: u32,
}

impl FollowProcess {
    pub fn new(target: FollowTarget) -> Self {
        Self {
            target,
            last_goal: None,
            last_recalc_tick: 0,
        }
    }
}

#[derive(Clone)]
struct SavedOrder {
    goal: Arc<dyn Goal>,
    opts: PathfinderOpts,
}

impl std::fmt::Debug for SavedOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SavedOrder")
            .field("goal", &self.goal)
            .field("opts", &self.opts)
            .finish()
    }
}

/// Per-client basso state: whether orders are paused, and what to resume.
#[derive(Clone, Debug, Component, Default)]
pub struct OrderState {
    pub paused: bool,
    saved: Option<SavedOrder>,
}

/// How often the block/follow processes reconsider their targets, in ticks.
const SEARCH_INTERVAL_TICKS: u32 = 5;
/// Only following targets that moved more than this get a new goal.
const FOLLOW_RECALC_DISTANCE_SQUARED: f32 = 4.0;

pub struct CommandPlugin;

impl Plugin for CommandPlugin {
    fn build(&self, app: &mut bevy_app::App) {
        app.add_systems(Update, add_default_order_state).add_systems(
            GameTick,
            (handle_target_block_process, handle_follow_process),
        );
    }
}

pub fn add_default_order_state(
    mut commands: Commands,
    query: Query<Entity, (With<LocalEntity>, Without<OrderState>)>,
) {
    for entity in &query {
        commands.entity(entity).insert(OrderState::default());
    }
}

/// Go to a found block, or mine it, reissuing goals as needed.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn handle_target_block_process(
    mut commands: Commands,
    mut clients: Query<
        (
            Entity,
            &WorldName,
            &Position,
            &mut TargetBlockProcess,
            &OrderState,
            Option<&Pathfinder>,
            Option<&ExecutingPath>,
            Option<&Mining>,
            Option<&MiningQueued>,
        ),
        With<LocalEntity>,
    >,
    worlds: Res<Worlds>,
    mut goto_events: MessageWriter<GotoEvent>,
    mut start_mining_events: MessageWriter<StartMiningBlockEvent>,
    mut tick: Local<u32>,
) {
    *tick += 1;

    for (
        entity,
        world_name,
        position,
        mut process,
        order,
        pathfinder,
        executing_path,
        mining,
        mining_queued,
    ) in &mut clients
    {
        if order.paused {
            continue;
        }
        let Some(world_lock) = worlds.get(world_name) else {
            continue;
        };
        let world = world_lock.read();

        if let Some(target) = process.target {
            match world.get_block_state(target) {
                Some(state) if process.block_states.contains(&state) => {}
                Some(_) => {
                    // The block is gone (or changed), so this target is done.
                    process.target = None;
                    process.mined += 1;
                    if process
                        .remaining
                        .is_some_and(|remaining| process.mined >= remaining)
                    {
                        commands.entity(entity).remove::<TargetBlockProcess>();
                    }
                    continue;
                }
                None => {
                    // The chunk isn't loaded, so we can't tell yet.
                    continue;
                }
            }

            let reach_goal = ReachBlockPosGoal::new(target, world.chunks.clone());
            let reachable = reach_goal.success(BlockPos::from(**position));
            match process.goal {
                TargetBlockGoal::Goto => {
                    if reachable {
                        commands.entity(entity).remove::<TargetBlockProcess>();
                    }
                }
                TargetBlockGoal::Mine => {
                    if reachable {
                        if mining.is_none() && mining_queued.is_none() {
                            start_mining_events.write(StartMiningBlockEvent {
                                entity,
                                position: target,
                                force: true,
                            });
                        }
                    } else if executing_path.is_none()
                        && pathfinder.is_none_or(|pathfinder| pathfinder.goal.is_none())
                    {
                        goto_events.write(GotoEvent {
                            entity,
                            goal: Arc::new(reach_goal),
                            opts: PathfinderOpts::new(),
                        });
                    }
                }
            }
            continue;
        }

        if (*tick).wrapping_sub(process.last_search_tick) < SEARCH_INTERVAL_TICKS {
            continue;
        }
        process.last_search_tick = *tick;

        let origin = BlockPos::from(**position);
        let found = world
            .find_blocks(origin, &process.block_states)
            .find(|pos| origin.distance_to(*pos) <= f64::from(MAX_BLOCK_DISTANCE));
        if let Some(found) = found {
            process.target = Some(found);
        }
    }
}

/// Walk towards the nearest matching entity, recalculating only when it moves.
#[allow(clippy::type_complexity)]
pub fn handle_follow_process(
    mut clients: Query<
        (
            Entity,
            &WorldName,
            &Position,
            &mut FollowProcess,
            Option<&TabList>,
            Option<&Pathfinder>,
            Option<&ExecutingPath>,
        ),
        With<LocalEntity>,
    >,
    entities: Query<
        (
            Entity,
            &Position,
            &WorldName,
            Option<&EntityUuid>,
            Option<&EntityKindComponent>,
            Has<Player>,
        ),
        Without<LocalEntity>,
    >,
    mut goto_events: MessageWriter<GotoEvent>,
    mut tick: Local<u32>,
) {
    *tick += 1;

    for (entity, world_name, position, mut process, tab_list, pathfinder, executing_path) in
        &mut clients
    {
        let wanted_uuids = match &process.target {
            FollowTarget::PlayerNames(names) => {
                let Some(tab_list) = tab_list else {
                    continue;
                };
                let uuids = tab_list
                    .values()
                    .filter(|info| names.iter().any(|name| info.profile.name == *name))
                    .map(|info| info.uuid)
                    .collect::<Vec<_>>();
                if uuids.is_empty() {
                    continue;
                }
                Some(uuids)
            }
            _ => None,
        };

        let mut nearest: Option<(Vec3, f32)> = None;
        for (_other, other_position, other_world, uuid, kind, is_player) in &entities {
            if other_world != world_name {
                continue;
            }
            let matches = match &process.target {
                FollowTarget::Entities => true,
                FollowTarget::Players => is_player,
                FollowTarget::EntityKinds(kinds) => {
                    kind.is_some_and(|kind| kinds.contains(&kind.0))
                }
                FollowTarget::PlayerNames(_) => uuid.is_some_and(|uuid| {
                    wanted_uuids
                        .as_ref()
                        .is_some_and(|uuids| uuids.contains(&**uuid))
                }),
            };
            if !matches {
                continue;
            }

            let distance_squared = (**position).distance_squared_to(**other_position);
            if nearest.is_none_or(|(_, best)| distance_squared < best) {
                nearest = Some((**other_position, distance_squared));
            }
        }

        let Some((target_position, _)) = nearest else {
            continue;
        };

        let target_moved = process.last_goal.is_none_or(|last_goal| {
            last_goal.distance_squared_to(target_position) > FOLLOW_RECALC_DISTANCE_SQUARED
        });
        let no_path = executing_path.is_none()
            && pathfinder.is_none_or(|pathfinder| pathfinder.goal.is_none());
        if !target_moved && !no_path {
            continue;
        }

        process.last_goal = Some(target_position);
        process.last_recalc_tick = *tick;
        goto_events.write(GotoEvent {
            entity,
            goal: Arc::new(RadiusGoal::new(target_position, 3.0)),
            opts: PathfinderOpts::new(),
        });
    }
}

/// Pause pathing, mining and following, remembering the pathfinder goal.
pub(crate) fn pause(ecs: &mut World, entity: Entity) -> CommandOutput {
    if ecs
        .get::<OrderState>(entity)
        .is_some_and(|order| order.paused)
    {
        return CommandOutput::line("basso is already paused.");
    }
    if !ecs.contains::<OrderState>(entity) {
        ecs.entity_mut(entity).insert(OrderState::default());
    }

    let mut saved = None;
    if let Some(mut pathfinder) = ecs.get_mut::<Pathfinder>(entity) {
        if let Some(goal) = pathfinder.goal.take() {
            saved = Some(SavedOrder {
                goal,
                opts: pathfinder.opts.take().unwrap_or_default(),
            });
        }
        pathfinder.opts = None;
        pathfinder.is_calculating = false;
    }
    ecs.entity_mut(entity)
        .remove::<(ExecutingPath, ComputePath, MiningQueued)>();

    let mut order = ecs
        .get_mut::<OrderState>(entity)
        .expect("just inserted OrderState");
    order.paused = true;
    order.saved = saved;

    CommandOutput::line("Paused.")
}

/// Resume whatever was paused.
pub(crate) fn resume(ecs: &mut World, entity: Entity) -> CommandOutput {
    let Some(mut order) = ecs.get_mut::<OrderState>(entity) else {
        return CommandOutput::line("basso is not paused.");
    };
    if !order.paused {
        return CommandOutput::line("basso is not paused.");
    }
    order.paused = false;
    let saved = order.saved.take();
    drop(order);

    if let Some(saved) = saved {
        ecs.write_message(GotoEvent {
            entity,
            goal: saved.goal,
            opts: saved.opts,
        });
    }
    CommandOutput::line("Resumed.")
}

/// Cancel the current order and stop pathing and mining.
pub(crate) fn cancel(ecs: &mut World, entity: Entity) -> CommandOutput {
    let had_order = ecs.contains::<TargetBlockProcess>()
        || ecs.contains::<FollowProcess>()
        || ecs.contains::<ExecutingPath>()
        || ecs.contains::<Mining>()
        || ecs
            .get::<Pathfinder>(entity)
            .is_some_and(|pathfinder| pathfinder.goal.is_some() || pathfinder.is_calculating);
    clear_current_order(ecs, entity);
    if let Some(mut order) = ecs.get_mut::<OrderState>(entity) {
        order.paused = false;
        order.saved = None;
    }

    if had_order {
        CommandOutput::line("Cancelled.")
    } else {
        CommandOutput::line("Nothing to cancel.")
    }
}

/// Tell the user whether basso is paused.
pub(crate) fn paused(ecs: &mut World, entity: Entity) -> CommandOutput {
    let paused = ecs
        .get::<OrderState>(entity)
        .is_some_and(|order| order.paused);
    if paused {
        CommandOutput::line("basso is paused.")
    } else {
        CommandOutput::line("basso is not paused.")
    }
}

/// Remove any processes, pathfinder goal and movement, and stop mining.
pub(crate) fn clear_current_order(ecs: &mut World, entity: Entity) {
    ecs.entity_mut(entity)
        .remove::<(TargetBlockProcess, FollowProcess, MiningQueued)>();

    if ecs.contains::<Mining>(entity) {
        ecs.write_message(StopMiningBlockEvent { entity });
    }

    if let Some(mut pathfinder) = ecs.get_mut::<Pathfinder>(entity) {
        pathfinder.goal = None;
        pathfinder.opts = None;
        pathfinder.is_calculating = false;
    }
    ecs.entity_mut(entity)
        .remove::<(ExecutingPath, ComputePath)>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goto_process_targets_one_block_kind() {
        let process = TargetBlockProcess::goto(BlockKind::DiamondOre);
        assert_eq!(process.goal, TargetBlockGoal::Goto);
        assert!(process.remaining.is_none());
        assert!(process.target.is_none());
        assert!(!process.block_states.set.is_empty());
    }

    #[test]
    fn mine_process_remembers_count() {
        let process =
            TargetBlockProcess::mine(BlockStates::from(&[BlockKind::DiamondOre][..]), Some(3));
        assert_eq!(process.goal, TargetBlockGoal::Mine);
        assert_eq!(process.remaining, Some(3));
        assert_eq!(process.mined, 0);
    }

    #[test]
    fn target_goals_compare() {
        assert_ne!(TargetBlockGoal::Goto, TargetBlockGoal::Mine);
    }
}
