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
    /// The top of the highest part of the shape that covers most of the
    /// block's floor (0.5 for a bottom slab), 0 when there is none.
    pub floor_top: f32,
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

/// Blocks a player climbs by holding jump or sliding down.
const CLIMBABLE: &[&str] = &[
    "ladder",
    "vine",
    "weeping_vines",
    "weeping_vines_plant",
    "twisting_vines",
    "twisting_vines_plant",
    "cave_vines",
    "cave_vines_plant",
];

/// Whether a full cube block may be built with when no throwaway block is
/// carried: not something that falls, explodes, or is worth keeping.
pub fn is_buildable(name: &str, full_collision: bool) -> bool {
    let name = short_name(name);
    const NEVER: &[&str] = &[
        "sand", "red_sand", "gravel", "tnt", "bedrock", "barrier", "spawner", "beacon", "ancient_debris",
        "diamond_block", "emerald_block", "gold_block", "iron_block", "netherite_block", "lapis_block",
        "redstone_block", "raw_iron_block", "raw_gold_block", "raw_copper_block", "command_block",
        "chain_command_block", "repeating_command_block", "structure_block", "jigsaw",
    ];
    full_collision
        && !NEVER.contains(&name)
        && !name.ends_with("_ore")
        && !name.ends_with("_powder")
        && !AVOID.contains(&name)
}

/// Blocks 1.5 high to walk into, however their outline looks: a player can't
/// jump onto them.
fn is_tall(name: &str) -> bool {
    name.ends_with("_fence") || name.ends_with("_fence_gate") || name.ends_with("_wall") || name == "fence"
}

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
    // a half block is a floor even where the game's legacy solidity says no
    let half_floor = block.floor_top >= 0.5 && block.floor_top < 1.0;
    let tall = is_tall(name);
    let collides = block.full_collision || block.legacy_solid || half_floor || tall;
    let solid = block.full_collision && !avoid;
    let standable = !tall && (solid || (collides && !avoid));
    Cell {
        passable: !collides && !liquid && !avoid,
        solid,
        standable,
        water: false,
        liquid,
        falling: FALLING.contains(&name) || name.ends_with("concrete_powder"),
        climbable: CLIMBABLE.contains(&name),
        thin_floor: name.ends_with("_trapdoor") && block.floor_top >= 0.1 && block.floor_top < 0.5,
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

    #[test]
    fn a_closed_trapdoor_is_a_thin_floor_and_an_open_one_is_not() {
        let closed = BlockTraits { name: "minecraft:oak_trapdoor", floor_top: 0.1875, ..Default::default() };
        let cell = classify(&closed);
        assert!(cell.thin_floor && cell.passable);
        let open = BlockTraits { floor_top: 0.0, ..closed };
        assert!(!classify(&open).thin_floor);
        let carpet = BlockTraits { name: "minecraft:white_carpet", floor_top: 0.0625, ..Default::default() };
        assert!(!classify(&carpet).thin_floor);
    }

    #[test]
    fn ladders_and_vines_are_climbable_and_passable() {
        for name in ["minecraft:ladder", "minecraft:vine", "minecraft:cave_vines_plant", "minecraft:twisting_vines"] {
            let cell = classify(&traits(name));
            assert!(cell.climbable && cell.passable && !cell.standable, "{name}");
        }
        assert!(!classify(&traits("minecraft:oak_fence")).climbable);
    }

    #[test]
    fn fences_walls_and_gates_are_in_the_way_and_cannot_be_stood_on() {
        for name in [
            "minecraft:oak_fence",
            "minecraft:nether_brick_fence",
            "minecraft:oak_fence_gate",
            "minecraft:cobblestone_wall",
            "minecraft:mossy_stone_brick_wall",
        ] {
            // however the game describes them, 1.5 high stops a jump
            let fence = BlockTraits {
                legacy_solid: true,
                floor_top: 0.375,
                ..traits(name)
            };
            let cell = classify(&fence);
            assert!(!cell.passable && !cell.standable && !cell.solid, "{name}");
        }
        // a lookalike name is not one
        assert!(classify(&BlockTraits { full_collision: true, legacy_solid: true, ..traits("minecraft:fenced_stone") }).standable);
    }

    #[test]
    fn a_half_block_is_a_floor_even_without_legacy_solidity() {
        let slab = BlockTraits {
            floor_top: 0.5,
            ..traits("minecraft:oak_slab")
        };
        let cell = classify(&slab);
        assert!(cell.standable && !cell.passable && !cell.solid);
    }

    #[test]
    fn flat_things_on_the_floor_are_walked_through() {
        for (name, top) in [("minecraft:white_carpet", 0.0625), ("minecraft:oak_pressure_plate", 0.0625)] {
            let flat = BlockTraits {
                hardness: 0.1,
                floor_top: top,
                ..traits(name)
            };
            let cell = classify(&flat);
            assert!(cell.passable && !cell.standable, "{name}");
        }
    }

    #[test]
    fn a_full_height_floor_top_is_left_to_the_other_flags() {
        let stone = BlockTraits {
            full_collision: true,
            legacy_solid: true,
            floor_top: 1.0,
            ..traits("minecraft:stone")
        };
        assert!(classify(&stone).solid);
    }

    #[test]
    fn glowstone_and_planks_are_buildable() {
        for name in ["minecraft:glowstone", "oak_planks", "minecraft:stone_bricks", "minecraft:crafting_table"] {
            assert!(is_buildable(name, true), "{name}");
        }
    }

    #[test]
    fn valuables_and_falling_blocks_are_not_buildable() {
        for name in [
            "minecraft:sand", "gravel", "minecraft:diamond_block", "minecraft:iron_ore",
            "minecraft:deepslate_diamond_ore", "tnt", "white_concrete_powder", "minecraft:bedrock",
        ] {
            assert!(!is_buildable(name, true), "{name}");
        }
    }

    #[test]
    fn only_full_cubes_are_buildable() {
        assert!(!is_buildable("minecraft:oak_slab", false));
        assert!(!is_buildable("minecraft:glowstone", false));
    }
}
