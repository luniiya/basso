//! Parsing `/basso` command lines, after `basso::commands` but without the
//! client: block and entity names stay strings for the game to check.

use crate::{goals::Goal, pos::BlockPos};

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
    (
        "click",
        "",
        "Toggle click mode: left click goes into the block you point at, right click onto it",
    ),
    (
        "set",
        "[setting] [true|false]",
        "List the settings, show one, or change one",
    ),
    ("toggle", "<setting>", "Turn a setting on or off"),
    ("help", "[command]", "Show this help, or help for one command"),
];

/// The commands' names, in [`COMMAND_HELP`] order, for completion.
pub const COMMAND_NAMES: &[&str] = &[
    "goto", "mine", "follow", "pause", "resume", "cancel", "paused", "click", "set", "toggle", "help",
];

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// A bare `/basso`: point at the help instead of printing all of it.
    Hint,
    Help(Option<String>),
    Goto(Goal),
    GotoBlock(String),
    Mine {
        count: Option<u32>,
        blocks: Vec<String>,
    },
    Follow(FollowTarget),
    Pause,
    Resume,
    Cancel,
    Paused,
    Click,
    Set {
        name: Option<String>,
        value: Option<String>,
    },
    Toggle(String),
}

/// What a bare `/basso` says.
pub const HINT: &str = "Type `basso help` for the commands.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FollowTarget {
    Entities,
    Players,
    /// Entity types such as `zombie` or `minecraft:zombie`.
    EntityKinds(Vec<String>),
    PlayerNames(Vec<String>),
}

impl std::fmt::Display for FollowTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FollowTarget::Entities => write!(f, "entities"),
            FollowTarget::Players => write!(f, "players"),
            FollowTarget::EntityKinds(kinds) => write!(f, "{}", kinds.join(", ")),
            FollowTarget::PlayerNames(names) => write!(f, "{}", names.join(", ")),
        }
    }
}

/// Parse a command line, relative coordinates (`~`, `~5`) taken from
/// `origin`. The line may start with `basso` or `/basso`.
pub fn parse(line: &str, origin: BlockPos) -> Result<Command, String> {
    let args = parse_line(line);
    let Some((command, rest)) = args.split_first() else {
        return Ok(Command::Hint);
    };
    match command.as_str() {
        "help" | "?" => Ok(Command::Help(rest.first().cloned())),
        "goto" => parse_goto(rest, origin),
        "mine" => parse_mine_args(rest).map(|(count, blocks)| Command::Mine { count, blocks }),
        "follow" => parse_follow_target(rest).map(Command::Follow),
        "pause" => Ok(Command::Pause),
        "resume" => Ok(Command::Resume),
        "cancel" | "stop" => Ok(Command::Cancel),
        "paused" => Ok(Command::Paused),
        "click" => Ok(Command::Click),
        "set" => match rest {
            [] | [_] | [_, _] => Ok(Command::Set {
                name: rest.first().cloned(),
                value: rest.get(1).cloned(),
            }),
            _ => Err("Usage: basso set [setting] [true|false]".to_owned()),
        },
        "toggle" => match rest {
            [name] => Ok(Command::Toggle(name.clone())),
            _ => Err("Usage: basso toggle <setting>".to_owned()),
        },
        other => Err(format!("Unknown basso command `{other}`. Try `basso help`.")),
    }
}

/// Split a command line into tokens, dropping `basso` and a leading slash.
fn parse_line(line: &str) -> Vec<String> {
    let mut split = line.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
    if split
        .first()
        .is_some_and(|first| first.trim_start_matches('/') == "basso")
    {
        split.remove(0);
    }
    split
}

pub fn help(command: Option<&str>) -> Vec<String> {
    if let Some(command) = command {
        let command = command.trim_start_matches('/');
        if let Some((name, usage, description)) =
            COMMAND_HELP.iter().find(|(name, _, _)| *name == command)
        {
            let usage = if usage.is_empty() {
                format!("Usage: basso {name}")
            } else {
                format!("Usage: basso {name} {usage}")
            };
            return vec![usage, (*description).to_owned()];
        }
        return vec![format!("Unknown basso command `{command}`. Try `basso help`.")];
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
    lines
}

fn parse_goto(args: &[String], origin: BlockPos) -> Result<Command, String> {
    // `goto <block>` searches for the block, everything else is coordinates
    if let [block] = args
        && parse_i32(block, origin.y).is_err()
    {
        return Ok(Command::GotoBlock(block.clone()));
    }
    let goal = match args {
        [y] => Goal::Y(parse_i32(y, origin.y)?),
        [x, z] => Goal::XZ {
            x: parse_i32(x, origin.x)?,
            z: parse_i32(z, origin.z)?,
        },
        [x, y, z] => Goal::Block(BlockPos::new(
            parse_i32(x, origin.x)?,
            parse_i32(y, origin.y)?,
            parse_i32(z, origin.z)?,
        )),
        _ => return Err("Usage: basso goto <x> <y> <z>, <x> <z>, <y>, or <block>".to_owned()),
    };
    Ok(Command::Goto(goal))
}

fn parse_mine_args(args: &[String]) -> Result<(Option<u32>, Vec<String>), String> {
    const USAGE: &str = "Usage: basso mine [count] <block...>";
    let (count, blocks) = match args.split_first() {
        None => return Err(USAGE.to_owned()),
        Some((first, rest)) => match first.parse::<u32>() {
            Ok(count) => (Some(count), rest),
            Err(_) => (None, args),
        },
    };
    if blocks.is_empty() {
        return Err(USAGE.to_owned());
    }
    if count == Some(0) {
        return Err("The count must be at least 1.".to_owned());
    }
    Ok((count, blocks.to_vec()))
}

fn parse_follow_target(args: &[String]) -> Result<FollowTarget, String> {
    match args {
        [group] if group == "entities" => Ok(FollowTarget::Entities),
        [group] if group == "players" => Ok(FollowTarget::Players),
        [list, kinds @ ..] if list == "entity" && !kinds.is_empty() => {
            Ok(FollowTarget::EntityKinds(kinds.to_vec()))
        }
        [list, names @ ..] if list == "player" && !names.is_empty() => {
            Ok(FollowTarget::PlayerNames(names.to_vec()))
        }
        _ => Err("Usage: basso follow entities|players|entity <kind...>|player <name...>".to_owned()),
    }
}

/// Parse a coordinate, allowing `~` and `~N` to be relative to `origin`.
fn parse_i32(input: &str, origin: i32) -> Result<i32, String> {
    let input = input.trim();
    if input == "~" {
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

#[cfg(test)]
mod tests {
    #[test]
    fn command_names_follow_the_help() {
        let help: Vec<&str> = COMMAND_HELP.iter().map(|(name, _, _)| *name).collect();
        assert_eq!(help, COMMAND_NAMES);
    }

    use super::*;

    const ORIGIN: BlockPos = BlockPos::new(10, 64, -5);

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_relative_coordinates() {
        assert_eq!(parse_i32("10", 3).unwrap(), 10);
        assert_eq!(parse_i32("~", 3).unwrap(), 3);
        assert_eq!(parse_i32("~5", 3).unwrap(), 8);
        assert_eq!(parse_i32("~-2", 3).unwrap(), 1);
        assert!(parse_i32("nope", 3).is_err());
        assert!(parse_i32("~x", 3).is_err());
    }

    #[test]
    fn strips_basso_prefix() {
        assert_eq!(parse_line("/basso goto 1 2 3"), ["goto", "1", "2", "3"]);
        assert_eq!(parse_line("basso mine diamond_ore"), ["mine", "diamond_ore"]);
        assert_eq!(parse_line("goto 1 2 3"), ["goto", "1", "2", "3"]);
    }

    #[test]
    fn empty_line_is_a_hint() {
        assert_eq!(parse("basso", ORIGIN), Ok(Command::Hint));
        assert_eq!(parse("/basso", ORIGIN), Ok(Command::Hint));
        assert_eq!(parse("", ORIGIN), Ok(Command::Hint));
        assert_eq!(parse("basso help", ORIGIN), Ok(Command::Help(None)));
    }

    #[test]
    fn parses_settings_commands() {
        assert_eq!(parse("basso set", ORIGIN), Ok(Command::Set { name: None, value: None }));
        assert_eq!(
            parse("basso set autoTool", ORIGIN),
            Ok(Command::Set {
                name: Some("autoTool".to_owned()),
                value: None
            })
        );
        assert_eq!(
            parse("basso set autoTool false", ORIGIN),
            Ok(Command::Set {
                name: Some("autoTool".to_owned()),
                value: Some("false".to_owned())
            })
        );
        assert!(parse("basso set a b c", ORIGIN).is_err());
        assert_eq!(parse("basso toggle freecam", ORIGIN), Ok(Command::Toggle("freecam".to_owned())));
        assert!(parse("basso toggle", ORIGIN).is_err());
        assert!(parse("basso toggle a b", ORIGIN).is_err());
    }

    #[test]
    fn parses_goto_forms() {
        assert_eq!(
            parse("basso goto 1 2 3", ORIGIN),
            Ok(Command::Goto(Goal::Block(BlockPos::new(1, 2, 3))))
        );
        assert_eq!(
            parse("goto ~ ~1 ~-1", ORIGIN),
            Ok(Command::Goto(Goal::Block(BlockPos::new(10, 65, -6))))
        );
        assert_eq!(
            parse("goto 100 -200", ORIGIN),
            Ok(Command::Goto(Goal::XZ { x: 100, z: -200 }))
        );
        assert_eq!(parse("goto 12", ORIGIN), Ok(Command::Goto(Goal::Y(12))));
        assert_eq!(
            parse("goto crafting_table", ORIGIN),
            Ok(Command::GotoBlock("crafting_table".to_owned()))
        );
        assert!(parse("goto 1 2 3 4", ORIGIN).is_err());
        assert!(parse("goto", ORIGIN).is_err());
        assert!(parse("goto 1 nope 3", ORIGIN).is_err());
    }

    #[test]
    fn help_lists_the_commands() {
        let text = help(None).join("\n");
        for command in ["goto", "mine", "follow", "pause", "resume", "cancel", "help", "set", "toggle"] {
            assert!(text.contains(command), "{command} missing from {text}");
        }
    }

    #[test]
    fn help_for_one_command() {
        let lines = help(Some("mine"));
        assert!(lines[0].starts_with("Usage: basso mine"));
        assert_eq!(lines.len(), 2);
        assert!(help(Some("nope"))[0].contains("Unknown"));
    }

    #[test]
    fn parses_mine_arguments() {
        let (count, blocks) =
            parse_mine_args(&strings(&["64", "diamond_ore", "deepslate_diamond_ore"])).unwrap();
        assert_eq!(count, Some(64));
        assert_eq!(blocks, ["diamond_ore", "deepslate_diamond_ore"]);

        let (count, blocks) = parse_mine_args(&strings(&["diamond_ore"])).unwrap();
        assert_eq!(count, None);
        assert_eq!(blocks, ["diamond_ore"]);

        assert!(parse_mine_args(&[]).is_err());
        assert!(parse_mine_args(&strings(&["5"])).is_err());
        assert!(parse_mine_args(&strings(&["0", "stone"])).is_err());
    }

    #[test]
    fn parses_follow_targets() {
        assert_eq!(parse_follow_target(&strings(&["players"])), Ok(FollowTarget::Players));
        assert_eq!(parse_follow_target(&strings(&["entities"])), Ok(FollowTarget::Entities));
        assert_eq!(
            parse_follow_target(&strings(&["entity", "zombie"])),
            Ok(FollowTarget::EntityKinds(strings(&["zombie"])))
        );
        assert_eq!(
            parse_follow_target(&strings(&["player", "alice"])),
            Ok(FollowTarget::PlayerNames(strings(&["alice"])))
        );
        assert!(parse_follow_target(&[]).is_err());
        assert!(parse_follow_target(&strings(&["entity"])).is_err());
    }

    #[test]
    fn simple_commands() {
        assert_eq!(parse("basso pause", ORIGIN), Ok(Command::Pause));
        assert_eq!(parse("basso resume", ORIGIN), Ok(Command::Resume));
        assert_eq!(parse("basso cancel", ORIGIN), Ok(Command::Cancel));
        assert_eq!(parse("basso stop", ORIGIN), Ok(Command::Cancel));
        assert_eq!(parse("basso paused", ORIGIN), Ok(Command::Paused));
        assert!(parse("basso dance", ORIGIN).is_err());
    }
}
