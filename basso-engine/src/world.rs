//! What the pathfinder needs to know about blocks, and the costs it derives
//! from them (Azalea's `CachedWorld`, without its chunk storage).

use std::{cell::RefCell, collections::HashMap};

use crate::{costs::BLOCK_BREAK_ADDITIONAL_PENALTY, pos::BlockPos};

/// What a block is like for walking and mining, filled in by the game.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    /// A player can be inside it: air, flowers, torches, open doors...
    pub passable: bool,
    /// Its top is a full face to stand on.
    pub solid: bool,
    /// It can be stood on, solid or not (slabs, stairs).
    pub standable: bool,
    /// Water, which a player swims through.
    pub water: bool,
    /// Any fluid, waterlogged blocks included: breaking next to it floods.
    pub liquid: bool,
    /// Sand, gravel and the like, which fall when the block below goes.
    pub falling: bool,
    /// Ticks to break it with what the player holds, `f32::INFINITY` when it
    /// can't be broken (bedrock, lava, fluids).
    pub break_ticks: f32,
}

impl Cell {
    pub const AIR: Self = Self {
        passable: true,
        solid: false,
        standable: false,
        water: false,
        liquid: false,
        falling: false,
        break_ticks: 0.0,
    };

    /// A full block like stone, broken in `break_ticks`.
    pub const fn solid(break_ticks: f32) -> Self {
        Self {
            passable: false,
            solid: true,
            standable: true,
            water: false,
            liquid: false,
            falling: false,
            break_ticks,
        }
    }

    pub const WATER: Self = Self {
        passable: false,
        solid: false,
        standable: false,
        water: true,
        liquid: true,
        falling: false,
        break_ticks: f32::INFINITY,
    };

    pub const LAVA: Self = Self {
        passable: false,
        solid: false,
        standable: false,
        water: false,
        liquid: true,
        falling: false,
        break_ticks: f32::INFINITY,
    };

    /// What an unloaded block counts as: nothing can go there.
    pub const UNLOADED: Self = Self {
        passable: false,
        solid: false,
        standable: false,
        water: false,
        liquid: false,
        falling: false,
        break_ticks: f32::INFINITY,
    };
}

/// The game's world, as the pathfinder reads it.
pub trait BlockView {
    /// The block at `pos`, or `None` when its chunk isn't loaded.
    fn cell(&self, pos: BlockPos) -> Option<Cell>;
    /// Whether the block at `pos` is one of `blocks` (ids such as
    /// `diamond_ore` or `minecraft:diamond_ore`). `None` when not loaded.
    fn is_block(&self, pos: BlockPos, blocks: &[String]) -> Option<bool>;
    /// The lowest block y of the world.
    fn min_y(&self) -> i32;
    /// Whether `id` names a block at all, to reject typos in commands.
    fn knows_block(&self, _id: &str) -> bool {
        true
    }
}

/// A [`BlockView`] with every block looked up once, and the costs Azalea's
/// moves use. One lives for one search.
pub struct CachedWorld<'a> {
    view: &'a dyn BlockView,
    cells: RefCell<HashMap<BlockPos, Option<Cell>>>,
    break_costs: RefCell<HashMap<BlockPos, f32>>,
    /// Whether moves may break blocks.
    pub allow_mining: bool,
}

impl<'a> CachedWorld<'a> {
    pub fn new(view: &'a dyn BlockView, allow_mining: bool) -> Self {
        Self {
            view,
            cells: RefCell::default(),
            break_costs: RefCell::default(),
            allow_mining,
        }
    }

    fn lookup(&self, pos: BlockPos) -> Option<Cell> {
        if let Some(cell) = self.cells.borrow().get(&pos) {
            return *cell;
        }
        let cell = self.view.cell(pos);
        self.cells.borrow_mut().insert(pos, cell);
        cell
    }

    /// The block at `pos`, [`Cell::UNLOADED`] when its chunk isn't loaded.
    pub fn cell(&self, pos: BlockPos) -> Cell {
        self.lookup(pos).unwrap_or(Cell::UNLOADED)
    }

    pub fn is_loaded(&self, pos: BlockPos) -> bool {
        self.lookup(pos).is_some()
    }

    pub fn is_block_passable(&self, pos: BlockPos) -> bool {
        self.cell(pos).passable
    }

    pub fn is_block_water(&self, pos: BlockPos) -> bool {
        self.cell(pos).water
    }

    pub fn is_block_solid(&self, pos: BlockPos) -> bool {
        self.cell(pos).solid
    }

    pub fn is_block_standable(&self, pos: BlockPos) -> bool {
        self.cell(pos).standable
    }

    /// Both the feet and head blocks are passable.
    pub fn is_passable(&self, pos: BlockPos) -> bool {
        self.is_block_passable(pos) && self.is_block_passable(pos.up(1))
    }

    /// A player could stand with their feet in `pos`.
    pub fn is_standable(&self, pos: BlockPos) -> bool {
        self.is_block_standable(pos.down(1)) && self.is_passable(pos)
    }

    /// The cost of breaking the block so a player fits through, 0 when it's
    /// already passable or water and infinite when it can't or mustn't be
    /// broken.
    pub fn cost_for_breaking_block(&self, pos: BlockPos) -> f32 {
        if let Some(cost) = self.break_costs.borrow().get(&pos) {
            return *cost;
        }
        let cost = self.uncached_cost_for_breaking_block(pos);
        self.break_costs.borrow_mut().insert(pos, cost);
        cost
    }

    fn uncached_cost_for_breaking_block(&self, pos: BlockPos) -> f32 {
        let cell = self.cell(pos);
        if cell.passable {
            return 0.0;
        }
        if !self.allow_mining || !cell.break_ticks.is_finite() || cell.liquid {
            return f32::INFINITY;
        }
        // Breaking next to a fluid floods the path; Azalea and Baritone refuse.
        let above = self.cell(pos.up(1));
        if above.liquid {
            return f32::INFINITY;
        }
        for side in [
            pos.offset(1, 0, 0),
            pos.offset(-1, 0, 0),
            pos.offset(0, 0, 1),
            pos.offset(0, 0, -1),
        ] {
            if self.cell(side).liquid {
                return f32::INFINITY;
            }
        }
        let mut cost = cell.break_ticks + BLOCK_BREAK_ADDITIONAL_PENALTY;
        if above.falling {
            // it falls into the gap, so it has to be broken too
            cost += self.cost_for_breaking_block(pos.up(1));
        }
        cost
    }

    /// The cost of clearing the feet and head blocks.
    pub fn cost_for_passing(&self, pos: BlockPos) -> f32 {
        self.cost_for_breaking_block(pos) + self.cost_for_breaking_block(pos.up(1))
    }

    /// The cost of clearing `pos` to stand in, infinite without a floor.
    pub fn cost_for_standing(&self, pos: BlockPos) -> f32 {
        if !self.is_block_standable(pos.down(1)) {
            return f32::INFINITY;
        }
        self.cost_for_passing(pos)
    }

    /// How many passable blocks are below `pos`, `u32::MAX` into the void.
    pub fn fall_distance(&self, pos: BlockPos) -> u32 {
        let min_y = self.view.min_y();
        let mut distance = 0;
        let mut current = pos.down(1);
        while self.is_block_passable(current) {
            distance += 1;
            current = current.down(1);
            if current.y < min_y {
                return u32::MAX;
            }
        }
        distance
    }

    /// Whether a player at `pos` must break this block to go on: it isn't
    /// passable or water.
    pub fn should_mine(&self, pos: BlockPos) -> bool {
        let cell = self.cell(pos);
        !cell.passable && !cell.water
    }

    pub fn view(&self) -> &dyn BlockView {
        self.view
    }
}

#[cfg(test)]
pub(crate) mod test_world {
    //! A small in-memory world for the tests.

    use std::collections::{HashMap, HashSet};

    use super::*;

    #[derive(Default)]
    pub struct TestWorld {
        pub cells: HashMap<BlockPos, (Cell, &'static str)>,
        /// Columns (x, z) that count as loaded; everything else is unloaded.
        pub loaded: HashSet<(i32, i32)>,
    }

    impl TestWorld {
        /// A flat stone floor at y = 63 over `-r..=r` in x and z, air above.
        pub fn flat(r: i32) -> Self {
            let mut world = Self::default();
            for x in -r..=r {
                for z in -r..=r {
                    world.loaded.insert((x, z));
                    world.set(BlockPos::new(x, 63, z), Cell::solid(30.0), "stone");
                }
            }
            world
        }

        pub fn set(&mut self, pos: BlockPos, cell: Cell, name: &'static str) {
            self.loaded.insert((pos.x, pos.z));
            self.cells.insert(pos, (cell, name));
        }

        pub fn stone(&mut self, pos: BlockPos) {
            self.set(pos, Cell::solid(30.0), "stone");
        }

        pub fn air(&mut self, pos: BlockPos) {
            self.cells.remove(&pos);
        }
    }

    impl BlockView for TestWorld {
        fn cell(&self, pos: BlockPos) -> Option<Cell> {
            if !self.loaded.contains(&(pos.x, pos.z)) {
                return None;
            }
            Some(self.cells.get(&pos).map_or(Cell::AIR, |(cell, _)| *cell))
        }

        fn is_block(&self, pos: BlockPos, blocks: &[String]) -> Option<bool> {
            if !self.loaded.contains(&(pos.x, pos.z)) {
                return None;
            }
            let name = self.cells.get(&pos).map_or("air", |(_, name)| name);
            Some(blocks.iter().any(|b| crate::block_id_matches(b, name)))
        }

        fn min_y(&self) -> i32 {
            -64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{test_world::TestWorld, *};

    #[test]
    fn passable_and_standable() {
        let world = TestWorld::flat(2);
        let cached = CachedWorld::new(&world, true);
        assert!(cached.is_standable(BlockPos::new(0, 64, 0)));
        assert!(!cached.is_standable(BlockPos::new(0, 65, 0)));
        assert!(!cached.is_passable(BlockPos::new(0, 63, 0)));
    }

    #[test]
    fn unloaded_blocks_block_everything() {
        let world = TestWorld::flat(1);
        let cached = CachedWorld::new(&world, true);
        let outside = BlockPos::new(5, 64, 0);
        assert!(!cached.is_loaded(outside));
        assert!(!cached.is_passable(outside));
        assert_eq!(cached.cost_for_breaking_block(outside), f32::INFINITY);
    }

    #[test]
    fn breaking_costs_its_ticks_plus_a_penalty() {
        let world = TestWorld::flat(2);
        let cached = CachedWorld::new(&world, true);
        let cost = cached.cost_for_breaking_block(BlockPos::new(0, 63, 0));
        assert_eq!(cost, 30.0 + BLOCK_BREAK_ADDITIONAL_PENALTY);
        assert_eq!(cached.cost_for_breaking_block(BlockPos::new(0, 64, 0)), 0.0);
    }

    #[test]
    fn no_mining_when_not_allowed() {
        let world = TestWorld::flat(2);
        let cached = CachedWorld::new(&world, false);
        assert_eq!(
            cached.cost_for_breaking_block(BlockPos::new(0, 63, 0)),
            f32::INFINITY
        );
    }

    #[test]
    fn never_breaks_next_to_a_fluid() {
        let mut world = TestWorld::flat(2);
        world.stone(BlockPos::new(0, 64, 0));
        world.set(BlockPos::new(1, 64, 0), Cell::WATER, "water");
        let cached = CachedWorld::new(&world, true);
        assert_eq!(
            cached.cost_for_breaking_block(BlockPos::new(0, 64, 0)),
            f32::INFINITY
        );
        world.set(BlockPos::new(1, 64, 0), Cell::AIR, "air");
        world.set(BlockPos::new(0, 65, 0), Cell::LAVA, "lava");
        let cached = CachedWorld::new(&world, true);
        assert_eq!(
            cached.cost_for_breaking_block(BlockPos::new(0, 64, 0)),
            f32::INFINITY
        );
    }

    #[test]
    fn a_falling_block_above_adds_its_cost() {
        let mut world = TestWorld::flat(2);
        world.stone(BlockPos::new(0, 64, 0));
        let gravel = Cell {
            falling: true,
            ..Cell::solid(10.0)
        };
        world.set(BlockPos::new(0, 65, 0), gravel, "gravel");
        let cached = CachedWorld::new(&world, true);
        assert_eq!(
            cached.cost_for_breaking_block(BlockPos::new(0, 64, 0)),
            30.0 + 10.0 + 2.0 * BLOCK_BREAK_ADDITIONAL_PENALTY
        );
    }

    #[test]
    fn fall_distance_counts_air_below() {
        let mut world = TestWorld::flat(2);
        world.stone(BlockPos::new(0, 60, 0));
        world.air(BlockPos::new(0, 63, 0));
        let cached = CachedWorld::new(&world, true);
        assert_eq!(cached.fall_distance(BlockPos::new(0, 64, 0)), 3);
        assert_eq!(cached.fall_distance(BlockPos::new(1, 64, 0)), 0);
    }
}
