//! What the chat box completes after `/basso`, as plain data: the game turns
//! each [`Arg`] into its own argument type. Every node also takes free text
//! after it, so a line the game can't parse still reaches basso.

use crate::{commands::COMMAND_NAMES, settings};

/// One word (or group of words) of a command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arg {
    /// The word itself.
    Literal(&'static str),
    /// `x y z`, with `~` relative parts.
    BlockPos,
    /// `x z`.
    ColumnPos,
    /// A whole number.
    Integer,
    /// A block id, completed from the blocks the game knows.
    Block,
    /// An entity type, completed from the summonable ones.
    EntityType,
    /// A player's name.
    Player,
    /// One of these words.
    OneOf(&'static [&'static str]),
    /// `true` or `false`.
    Bool,
}

/// A node of the tree: the argument and what may follow it.
#[derive(Debug)]
pub struct Node {
    /// The argument's name, shown in usage hints.
    pub name: &'static str,
    pub arg: Arg,
    pub children: &'static [Node],
}

const fn node(name: &'static str, arg: Arg, children: &'static [Node]) -> Node {
    Node { name, arg, children }
}

const fn literal(name: &'static str, children: &'static [Node]) -> Node {
    node(name, Arg::Literal(name), children)
}

const BLOCKS: &[Node] = &[node("blocks", Arg::Block, &[])];

/// The words after `/basso`.
pub const TREE: &[Node] = &[
    literal(
        "goto",
        &[
            node("pos", Arg::BlockPos, &[]),
            node("column", Arg::ColumnPos, &[]),
            node("y", Arg::Integer, &[]),
            node("block", Arg::Block, &[]),
        ],
    ),
    literal(
        "mine",
        &[
            node("count", Arg::Integer, BLOCKS),
            node("block", Arg::Block, &[]),
        ],
    ),
    literal(
        "follow",
        &[
            literal("entities", &[]),
            literal("players", &[]),
            literal("entity", &[node("kind", Arg::EntityType, &[])]),
            literal("player", &[node("name", Arg::Player, &[])]),
        ],
    ),
    literal("pause", &[]),
    literal("resume", &[]),
    literal("cancel", &[]),
    literal("stop", &[]),
    literal("paused", &[]),
    literal(
        "set",
        &[node(
            "setting",
            Arg::OneOf(settings::NAMES),
            &[node("value", Arg::Bool, &[])],
        )],
    ),
    literal("toggle", &[node("setting", Arg::OneOf(settings::NAMES), &[])]),
    literal("help", &[node("command", Arg::OneOf(COMMAND_NAMES), &[])]),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        commands::{COMMAND_HELP, Command, parse},
        pos::BlockPos,
    };

    fn top_words() -> Vec<&'static str> {
        TREE
            .iter()
            .map(|n| match n.arg {
                Arg::Literal(word) => word,
                other => panic!("top-level {other:?} isn't a word"),
            })
            .collect()
    }

    #[test]
    fn every_command_is_completed() {
        let words = top_words();
        for (name, _, _) in COMMAND_HELP {
            assert!(words.contains(name), "{name} has no completion");
        }
    }

    #[test]
    fn every_completed_word_is_a_command() {
        for word in top_words() {
            assert!(
                !matches!(parse(word, BlockPos::new(0, 64, 0)), Err(e) if e.starts_with("Unknown basso command")),
                "{word}"
            );
        }
    }

    #[test]
    fn literal_names_match_their_words() {
        fn check(nodes: &[Node]) {
            for n in nodes {
                if let Arg::Literal(word) = n.arg {
                    assert_eq!(n.name, word);
                }
                check(n.children);
            }
        }
        check(TREE);
    }

    #[test]
    fn completed_lines_parse() {
        let origin = BlockPos::new(0, 64, 0);
        for line in [
            "goto 1 2 3",
            "goto ~ ~1 ~",
            "goto 1 3",
            "goto 70",
            "goto diamond_ore",
            "mine 5 coal_ore",
            "mine iron_ore",
            "follow entities",
            "follow players",
            "follow entity minecraft:zombie",
            "follow player Steve",
            "set autoTool false",
            "set freecam",
            "toggle exploreForBlocks",
            "help mine",
        ] {
            assert!(parse(line, origin).is_ok(), "{line}");
        }
        assert_eq!(parse("stop", origin), Ok(Command::Cancel));
    }

    #[test]
    fn help_completes_every_command() {
        let help = TREE.iter().find(|n| n.name == "help").unwrap();
        assert_eq!(help.children[0].arg, Arg::OneOf(COMMAND_NAMES));
        assert_eq!(COMMAND_NAMES.len(), COMMAND_HELP.len());
    }
}
