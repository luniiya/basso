//! Turning a game's facts about a block state into the pathfinder's [`Cell`],
//! so the game glue only copies fields across.

use crate::world::Cell;

/// The fluid in a block state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fluid {
    #[default]
    None,
    Water,
    Lava,
}

/// What the game knows about a block state, as plain values.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BlockTraits<'a> {
    /// The block id, such as `minecraft:stone`.
    pub name: &'a str,
    pub air: bool,
    /// Its collision shape is the full cube.
    pub full_collision: bool,
    /// `BlockState.isSolid()`: a collision shape big enough to block movement.
    pub legacy_solid: bool,
    pub fluid: Fluid,
    pub waterlogged: bool,
    /// Destroy time; negative for unbreakable blocks like bedrock.
    pub hardness: f32,
    /// Needs the right tool to drop and to break at full speed.
    pub requires_tool: bool,
}

/// Blocks a player could stand in but should never path through.
const AVOID: &[&str] = &[
    "fire",
    "soul_fire",
    "cobweb",
    "sweet_berry_bush",
    "powder_snow",
    "magma_block",
    "campfire",
    "soul_campfire",
    "wither_rose",
    "pointed_dripstone",
    "end_portal",
    "nether_portal",
];

/// Blocks that fall when the block under them goes.
const FALLING: &[&str] = &[
    "sand",
    "red_sand",
    "suspicious_sand",
    "gravel",
    "suspicious_gravel",
    "anvil",
    "chipped_anvil",
    "damaged_anvil",
    "dragon_egg",
];

fn short_name(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

/// Ticks to break a block bare-handed, as vanilla's `getDestroyProgress`:
/// hardness × 30 (× 100 when it needs a tool the hand isn't).
pub fn hand_break_ticks(hardness: f32, requires_tool: bool) -> f32 {
    if hardness < 0.0 {
        return f32::INFINITY;
    }
    if hardness == 0.0 {
        return 1.0;
    }
    (hardness * if requires_tool { 100.0 } else { 30.0 }).ceil()
}

pub fn classify(block: &BlockTraits) -> Cell {
    if block.air {
        return Cell::AIR;
    }
    let name = short_name(block.name);
    match block.fluid {
        Fluid::Water if !block.waterlogged => return Cell::WATER,
        Fluid::Lava => return Cell::LAVA,
        _ => {}
    }
    let liquid = block.fluid != Fluid::None || block.waterlogged;
    let avoid = AVOID.contains(&name);
    let collides = block.full_collision || block.legacy_solid;
    let solid = block.full_collision && !avoid;
    let standable = solid || (collides && !avoid);
    Cell {
        passable: !collides && !liquid && !avoid,
        solid,
        standable,
        water: false,
        liquid,
        falling: FALLING.contains(&name) || name.ends_with("concrete_powder"),
        break_ticks: if liquid {
            f32::INFINITY
        } else {
            hand_break_ticks(block.hardness, block.requires_tool)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn traits(name: &str) -> BlockTraits<'_> {
        BlockTraits {
            name,
            hardness: 1.5,
            ..Default::default()
        }
    }

    #[test]
    fn air_is_air() {
        let air = BlockTraits {
            air: true,
            ..traits("minecraft:air")
        };
        assert_eq!(classify(&air), Cell::AIR);
    }

    #[test]
    fn stone_is_solid_and_breakable() {
        let stone = BlockTraits {
            full_collision: true,
            legacy_solid: true,
            requires_tool: true,
            ..traits("minecraft:stone")
        };
        let cell = classify(&stone);
        assert!(cell.solid && cell.standable && !cell.passable);
        assert_eq!(cell.break_ticks, 150.0);
    }

    #[test]
    fn grass_is_passable() {
        let grass = BlockTraits {
            hardness: 0.0,
            ..traits("minecraft:short_grass")
        };
        let cell = classify(&grass);
        assert!(cell.passable && !cell.solid && !cell.standable);
        assert_eq!(cell.break_ticks, 1.0);
    }

    #[test]
    fn slabs_are_standable_but_not_solid() {
        let slab = BlockTraits {
            legacy_solid: true,
            ..traits("minecraft:oak_slab")
        };
        let cell = classify(&slab);
        assert!(!cell.solid && cell.standable && !cell.passable);
    }

    #[test]
    fn fluids() {
        let water = BlockTraits {
            fluid: Fluid::Water,
            ..traits("minecraft:water")
        };
        assert_eq!(classify(&water), Cell::WATER);
        let lava = BlockTraits {
            fluid: Fluid::Lava,
            ..traits("minecraft:lava")
        };
        assert_eq!(classify(&lava), Cell::LAVA);
    }

    #[test]
    fn waterlogged_blocks_are_liquid_and_unbreakable() {
        let seagrass = BlockTraits {
            fluid: Fluid::Water,
            waterlogged: true,
            ..traits("minecraft:oak_stairs")
        };
        let cell = classify(&seagrass);
        assert!(cell.liquid && !cell.passable);
        assert_eq!(cell.break_ticks, f32::INFINITY);
    }

    #[test]
    fn hazards_are_avoided() {
        for name in ["minecraft:fire", "minecraft:cobweb", "sweet_berry_bush"] {
            assert!(!classify(&traits(name)).passable, "{name}");
        }
        let magma = BlockTraits {
            full_collision: true,
            ..traits("minecraft:magma_block")
        };
        let cell = classify(&magma);
        assert!(!cell.solid && !cell.standable);
    }

    #[test]
    fn falling_blocks() {
        assert!(classify(&traits("minecraft:gravel")).falling);
        assert!(classify(&traits("minecraft:red_concrete_powder")).falling);
        assert!(!classify(&traits("minecraft:dirt")).falling);
    }

    #[test]
    fn break_ticks() {
        assert_eq!(hand_break_ticks(-1.0, false), f32::INFINITY);
        assert_eq!(hand_break_ticks(0.0, false), 1.0);
        assert_eq!(hand_break_ticks(0.5, false), 15.0);
        assert_eq!(hand_break_ticks(3.0, true), 300.0);
    }
}
