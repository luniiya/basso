//! In-game commands, modeled after Baritone's but under a single `/basso`
//! command.
//!
//! [`execute`] parses a command line and immediately updates the client's ECS
//! state. Long-running orders (going to a block, mining, following) are driven
//! by [`process`]'s systems.
//!
//! The Horizon integration calls [`execute`] with the rest of the line after
//! `/basso`.

use std::str::FromStr;

use basso_block::BlockStates;
use basso_core::position::BlockPos;
use basso_entity::Position;
use basso_registry::builtin::{BlockKind, EntityKind};
use bevy_ecs::{entity::Entity, world::World};

use crate::{
    Client,
    pathfinder::{
        PathfinderOpts,
        goals::{BlockPosGoal, Goal, XZGoal, YGoal},
    },
};

pub mod process;

pub use process::{CommandPlugin, FollowProcess, FollowTarget, OrderState, TargetBlockProcess};

/// The result of running a command: lines to show in chat.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandOutput {
    pub lines: Vec<String>,
}

impl CommandOutput {
    pub fn ok(lines: impl IntoIterator<Item = String>) -> Self {
        Self {
            lines: lines.into_iter().collect(),
        }
    }

    pub fn line(line: impl Into<String>) -> Self {
        Self::ok([line.into()])
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::line(message)
    }
}

/// The commands the help lists, in order, with their usage and description.
pub const COMMAND_HELP: &[(&str, &str, &str)] = &[
    (
        "goto",
        "<x> <y> <z> | <x> <z> | <y> | <block>",
        "Go to coordinates or to the nearest matching block",
    ),
    (
        "mine",
        "[count] <block...>",
        "Mine matching blocks, optionally stopping after `count` mined",
    ),
    (
        "follow",
        "entities | players | entity <kind...> | player <name...>",
        "Follow living entities, players, or specific kinds/names",
    ),
    ("pause", "", "Pause pathing, mining and following"),
    ("resume", "", "Resume whatever was paused"),
    ("cancel", "", "Cancel the current order"),
    ("paused", "", "Tell whether basso is paused"),
    ("help", "[command]", "Show this help, or help for one command"),
];

/// Run a `/basso` command line for the given client.
///
/// The line may be just the arguments (`goto 0 64 0`), include the command
/// (`basso goto 0 64 0`), or a slash (`/basso goto 0 64 0`).
pub fn execute(client: &Client, line: &str) -> CommandOutput {
    let mut ecs = client.ecs.write();
    execute_in_world(&mut ecs, client.entity, line)
}

/// [`execute`] against a raw ECS world, for tests and the game integration.
pub fn execute_in_world(ecs: &mut World, entity: Entity, line: &str) -> CommandOutput {
    let args = parse_line(line);
    let Some((command, rest)) = args.split_first() else {
        return help_output(None);
    };

    match command.as_str() {
        "help" | "?" => help_output(rest.first().map(String::as_str)),
        "goto" => command_goto(ecs, entity, rest),
        "mine" => command_mine(ecs, entity, rest),
        "follow" => command_follow(ecs, entity, rest),
        "pause" => process::pause(ecs, entity),
        "resume" => process::resume(ecs, entity),
        "cancel" | "stop" => process::cancel(ecs, entity),
        "paused" => process::paused(ecs, entity),
        other => CommandOutput::error(format!(
            "Unknown basso command `{other}`. Try `basso help`."
        )),
    }
}

/// Split a command line into tokens, dropping `basso` and a leading slash.
fn parse_line(line: &str) -> Vec<String> {
    let mut split = line
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if split
        .first()
        .is_some_and(|first| first.trim_start_matches('/') == "basso")
    {
        split.remove(0);
    }
    split
}

fn help_output(command: Option<&str>) -> CommandOutput {
    if let Some(command) = command {
        let command = command.trim_start_matches('/');
        if let Some((name, usage, description)) = COMMAND_HELP
            .iter()
            .find(|(name, _, _)| *name == command)
        {
            let usage = if usage.is_empty() {
                format!("Usage: basso {name}")
            } else {
                format!("Usage: basso {name} {usage}")
            };
            return CommandOutput::ok([usage, (*description).to_owned()]);
        }
        return CommandOutput::error(format!(
            "Unknown basso command `{command}`. Try `basso help`."
        ));
    }

    let mut lines = vec!["basso commands:".to_owned()];
    for (name, usage, description) in COMMAND_HELP {
        let usage = if usage.is_empty() {
            name.to_string()
        } else {
            format!("{name} {usage}")
        };
        lines.push(format!("  {usage} - {description}"));
    }
    lines.push("Type `basso help <command>` for details.".to_owned());
    CommandOutput::ok(lines)
}

fn command_goto(ecs: &mut World, entity: Entity, args: &[String]) -> CommandOutput {
    let origin = player_block_pos(ecs, entity);
    // `goto <block>` searches for the block, everything else is coordinates.
    let first_is_coordinate = parse_i32(&args.first().cloned().unwrap_or_default(), origin.x).is_ok();
    if args.len() == 1 && !first_is_coordinate {
        let kind = match BlockKind::from_str(&args[0]) {
            Ok(kind) => kind,
            Err(()) => {
                return CommandOutput::error(format!(
                    "`{}` is not a block or a coordinate.",
                    args[0]
                ));
            }
        };
        replace_order(ecs, entity, Order::GotoBlock(TargetBlockProcess::goto(kind)));
        return CommandOutput::line(format!("Going to the nearest {kind:?}."));
    }

    let goal: Box<dyn Goal> = match args.len() {
        1 => match parse_i32(&args[0], origin.y) {
            Ok(y) => Box::new(YGoal { y }),
            Err(err) => return CommandOutput::error(err),
        },
        2 => match (
            parse_i32(&args[0], origin.x),
            parse_i32(&args[1], origin.z),
        ) {
            (Ok(x), Ok(z)) => Box::new(XZGoal { x, z }),
            (Err(err), _) | (_, Err(err)) => return CommandOutput::error(err),
        },
        3 => match (
            parse_i32(&args[0], origin.x),
            parse_i32(&args[1], origin.y),
            parse_i32(&args[2], origin.z),
        ) {
            (Ok(x), Ok(y), Ok(z)) => Box::new(BlockPosGoal(BlockPos::new(x, y, z))),
            (Err(err), _, _) | (_, Err(err), _) | (_, _, Err(err)) => {
                return CommandOutput::error(err);
            }
        },
        _ => {
            return CommandOutput::error(
                "Usage: basso goto <x> <y> <z>, <x> <z>, <y>, or <block>",
            );
        }
    };

    let description = format!("Going to {goal:?}.");
    replace_order(ecs, entity, Order::Goto(goal));
    CommandOutput::line(description)
}

fn command_mine(ecs: &mut World, entity: Entity, args: &[String]) -> CommandOutput {
    let (count, kinds) = match parse_mine_args(args) {
        Ok(parsed) => parsed,
        Err(err) => return CommandOutput::error(err),
    };

    let description = match count {
        Some(count) => format!("Mining up to {count} of {kinds:?}."),
        None => format!("Mining all {kinds:?}."),
    };
    replace_order(
        ecs,
        entity,
        Order::Mine(TargetBlockProcess::mine(
            BlockStates::from(kinds.as_slice()),
            count,
        )),
    );
    CommandOutput::line(description)
}

fn parse_mine_args(args: &[String]) -> Result<(Option<u32>, Vec<BlockKind>), String> {
    if args.is_empty() {
        return Err("Usage: basso mine [count] <block...>".to_owned());
    }

    let (count, block_args) = if let Ok(count) = args[0].parse::<u32>() {
        (Some(count), &args[1..])
    } else {
        (None, args)
    };
    if block_args.is_empty() {
        return Err("Usage: basso mine [count] <block...>".to_owned());
    }

    let mut kinds = Vec::with_capacity(block_args.len());
    for arg in block_args {
        match BlockKind::from_str(arg) {
            Ok(kind) => kinds.push(kind),
            Err(()) => return Err(format!("Unknown block `{arg}`.")),
        }
    }
    Ok((count, kinds))
}

fn command_follow(ecs: &mut World, entity: Entity, args: &[String]) -> CommandOutput {
    let target = match parse_follow_target(args) {
        Ok(target) => target,
        Err(err) => return CommandOutput::error(err),
    };

    let description = format!("Following {target:?}.");
    replace_order(ecs, entity, Order::Follow(FollowProcess::new(target)));
    CommandOutput::line(description)
}

fn parse_follow_target(args: &[String]) -> Result<FollowTarget, String> {
    match args {
        [group] if group == "entities" => Ok(FollowTarget::Entities),
        [group] if group == "players" => Ok(FollowTarget::Players),
        [list, kinds @ ..] if list == "entity" && !kinds.is_empty() => {
            let mut parsed = Vec::with_capacity(kinds.len());
            for kind in kinds {
                match EntityKind::from_str(kind) {
                    Ok(kind) => parsed.push(kind),
                    Err(()) => return Err(format!("Unknown entity kind `{kind}`.")),
                }
            }
            Ok(FollowTarget::EntityKinds(parsed))
        }
        [list, names @ ..] if list == "player" && !names.is_empty() => {
            Ok(FollowTarget::PlayerNames(names.to_vec()))
        }
        _ => Err(
            "Usage: basso follow entities|players|entity <kind...>|player <name...>".to_owned(),
        ),
    }
}

enum Order {
    Goto(Box<dyn Goal>),
    GotoBlock(TargetBlockProcess),
    Mine(TargetBlockProcess),
    Follow(FollowProcess),
}

/// Replace whatever the client is currently doing with the new order.
fn replace_order(ecs: &mut World, entity: Entity, order: Order) {
    process::clear_current_order(ecs, entity);

    match order {
        Order::Goto(goal) => {
            ecs.entity_mut(entity).insert(OrderState::default());
            ecs_start_goto(ecs, entity, goal);
        }
        Order::GotoBlock(process) => {
            ecs.entity_mut(entity)
                .insert((OrderState::default(), process));
        }
        Order::Mine(process) => {
            ecs.entity_mut(entity)
                .insert((OrderState::default(), process));
        }
        Order::Follow(process) => {
            ecs.entity_mut(entity)
                .insert((OrderState::default(), process));
        }
    }
}

pub(crate) fn ecs_start_goto(ecs: &mut World, entity: Entity, goal: Box<dyn Goal>) {
    ecs.write_message(crate::pathfinder::GotoEvent {
        entity,
        goal: goal.into(),
        opts: PathfinderOpts::new(),
    });
}

fn player_block_pos(ecs: &mut World, entity: Entity) -> BlockPos {
    ecs.get::<Position>(entity)
        .map(|position| BlockPos::from(**position))
        .unwrap_or_default()
}

/// Parse a coordinate, allowing `~` and `~N` to be relative to `origin`.
fn parse_i32(input: &str, origin: i32) -> Result<i32, String> {
    let input = input.trim();
    if input == "~" || input == "~0" {
        return Ok(origin);
    }
    if let Some(offset) = input.strip_prefix('~') {
        return offset
            .parse::<i32>()
            .map(|offset| origin + offset)
            .map_err(|_| format!("Invalid relative coordinate `{input}`."));
    }
    input
        .parse::<i32>()
        .map_err(|_| format!("Invalid coordinate `{input}`."))
}

/// The block search refuses blocks further away than this.
pub(crate) const MAX_BLOCK_DISTANCE: f32 = 64.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_relative_coordinates() {
        assert_eq!(parse_i32("10", 3).unwrap(), 10);
        assert_eq!(parse_i32("~", 3).unwrap(), 3);
        assert_eq!(parse_i32("~5", 3).unwrap(), 8);
        assert_eq!(parse_i32("~-2", 3).unwrap(), 1);
        assert!(parse_i32("nope", 3).is_err());
    }

    #[test]
    fn strips_basso_prefix() {
        let args = parse_line("/basso goto 1 2 3");
        assert_eq!(args, ["goto", "1", "2", "3"]);
        let args = parse_line("basso mine diamond_ore");
        assert_eq!(args, ["mine", "diamond_ore"]);
        let args = parse_line("goto 1 2 3");
        assert_eq!(args, ["goto", "1", "2", "3"]);
    }

    #[test]
    fn help_lists_the_commands() {
        let output = help_output(None);
        let text = output.lines.join("\n");
        for command in ["goto", "mine", "follow", "pause", "resume", "cancel", "help"] {
            assert!(text.contains(command), "{command} missing from {text}");
        }
    }

    #[test]
    fn help_for_one_command() {
        let output = help_output(Some("mine"));
        assert!(output.lines[0].starts_with("Usage: basso mine"));
        assert_eq!(output.lines.len(), 2);
    }

    #[test]
    fn parses_mine_arguments() {
        let args = ["64", "diamond_ore", "deepslate_diamond_ore"]
            .map(str::to_owned)
            .to_vec();
        let (count, kinds) = parse_mine_args(&args).unwrap();
        assert_eq!(count, Some(64));
        assert_eq!(kinds, [BlockKind::DiamondOre, BlockKind::DeepslateDiamondOre]);

        let args = ["diamond_ore"].map(str::to_owned).to_vec();
        let (count, kinds) = parse_mine_args(&args).unwrap();
        assert_eq!(count, None);
        assert_eq!(kinds, [BlockKind::DiamondOre]);

        assert!(parse_mine_args(&[]).is_err());
        assert!(parse_mine_args(&["nope_block".to_owned()]).is_err());
    }

    #[test]
    fn parses_follow_targets() {
        assert_eq!(
            parse_follow_target(&["players".to_owned()]).unwrap(),
            FollowTarget::Players
        );
        assert_eq!(
            parse_follow_target(&["entities".to_owned()]).unwrap(),
            FollowTarget::Entities
        );
        assert_eq!(
            parse_follow_target(&["entity".to_owned(), "zombie".to_owned()]).unwrap(),
            FollowTarget::EntityKinds(vec![EntityKind::Zombie])
        );
        assert_eq!(
            parse_follow_target(&["player".to_owned(), "alice".to_owned()]).unwrap(),
            FollowTarget::PlayerNames(vec!["alice".to_owned()])
        );
        assert!(parse_follow_target(&[]).is_err());
        assert!(parse_follow_target(&["entity".to_owned()]).is_err());
    }
}
