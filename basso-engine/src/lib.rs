//! basso's pathfinder without Azalea's client: Azalea's path search and move
//! execution over a [`world::BlockView`] the game fills in. It has no
//! dependencies so it can ship inside a Horizon native mod bundle.

pub mod astar;
pub mod blocks;
pub mod bot;
pub mod commands;
pub mod costs;
pub mod executor;
pub mod goals;
pub mod moves;
pub mod pos;
pub mod render;
pub mod world;

/// Whether the block id `wanted` (`diamond_ore` or `minecraft:diamond_ore`)
/// names the block `name`, which may carry a namespace too.
pub fn block_id_matches(wanted: &str, name: &str) -> bool {
    let strip = |id: &str| id.strip_prefix("minecraft:").unwrap_or(id).to_owned();
    strip(wanted.trim()).eq_ignore_ascii_case(&strip(name))
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
}
