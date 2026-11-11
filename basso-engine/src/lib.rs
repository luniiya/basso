//! basso's pathfinder without Azalea's client: Azalea's path search and move
//! execution over a [`world::BlockView`] the game fills in. It has no
//! dependencies so it can ship inside a Horizon native mod bundle.

pub mod astar;
pub mod blocks;
pub mod bot;
pub mod commands;
pub mod completion;
pub mod costs;
pub mod executor;
pub mod goals;
pub mod moves;
pub mod pos;
pub mod render;
pub mod settings;
pub mod tools;
pub mod world;
pub mod camera;

/// Whether the block id `wanted` (`diamond_ore` or `minecraft:diamond_ore`)
/// names the block `name`, which may carry a namespace too. `wanted` may hold
/// one `*` standing for any text, as in `*_log`.
pub fn block_id_matches(wanted: &str, name: &str) -> bool {
    let strip = |id: &str| id.strip_prefix("minecraft:").unwrap_or(id).to_ascii_lowercase();
    let (wanted, name) = (strip(wanted.trim()), strip(name));
    match wanted.split_once('*') {
        Some((head, tail)) => {
            name.len() >= head.len() + tail.len() && name.starts_with(head) && name.ends_with(tail)
        }
        None => wanted == name,
    }
}

/// What a block word in a command means: the block itself when `knows` it,
/// else every block ending in `_word` (`log` is `*_log`, `ore` is `*_ore`).
/// `None` when neither names a block.
pub fn resolve_block_word(word: &str, knows: impl Fn(&str) -> bool) -> Option<String> {
    if knows(word) {
        return Some(word.to_owned());
    }
    let word = word.strip_prefix("minecraft:").unwrap_or(word);
    if word.contains('*') {
        return None;
    }
    let pattern = format!("*_{word}");
    knows(&pattern).then_some(pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_ids_match_with_or_without_namespace() {
        assert!(block_id_matches("diamond_ore", "minecraft:diamond_ore"));
        assert!(block_id_matches("minecraft:diamond_ore", "diamond_ore"));
        assert!(block_id_matches("Diamond_Ore", "diamond_ore"));
        assert!(!block_id_matches("diamond_ore", "deepslate_diamond_ore"));
        assert!(!block_id_matches("other:diamond_ore", "diamond_ore"));
    }

    #[test]
    fn wildcards_match_families() {
        assert!(block_id_matches("*_log", "minecraft:oak_log"));
        assert!(block_id_matches("*_log", "dark_oak_log"));
        assert!(block_id_matches("*_LOG", "minecraft:Birch_Log"));
        assert!(!block_id_matches("*_log", "oak_planks"));
        assert!(!block_id_matches("*_log", "_lo"));
        assert!(block_id_matches("oak_*", "oak_leaves"));
        assert!(block_id_matches("deepslate_*_ore", "deepslate_iron_ore"));
        assert!(!block_id_matches("deepslate_*_ore", "iron_ore"));
        assert!(block_id_matches("minecraft:*_ore", "coal_ore"));
    }

    #[test]
    fn block_words_resolve_to_families() {
        let known = ["oak_log", "birch_log", "stone", "coal_ore"];
        let knows = |id: &str| known.iter().any(|k| block_id_matches(id, k));
        assert_eq!(resolve_block_word("stone", knows).as_deref(), Some("stone"));
        assert_eq!(resolve_block_word("oak_log", knows).as_deref(), Some("oak_log"));
        assert_eq!(resolve_block_word("log", knows).as_deref(), Some("*_log"));
        assert_eq!(resolve_block_word("minecraft:log", knows).as_deref(), Some("*_log"));
        assert_eq!(resolve_block_word("ore", knows).as_deref(), Some("*_ore"));
        assert_eq!(resolve_block_word("*_log", knows).as_deref(), Some("*_log"));
        assert_eq!(resolve_block_word("nope", knows), None);
        assert_eq!(resolve_block_word("*_nope", knows), None);
    }
}
