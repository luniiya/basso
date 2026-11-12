//! The moves the pathfinder can make from a node, ported from Azalea's
//! `moves/basic.rs`, plus Baritone's parkour. Stair ascents are left out.

use std::f32::consts::SQRT_2;

use crate::{
    costs::*,
    pos::BlockPos,
    world::CachedWorld,
};

/// What kind of move an edge is, which decides how the executor plays it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveKind {
    Forward,
    Ascend,
    Descend,
    Diagonal,
    Downward,
    /// A sprint jump over a gap, flat or one block up.
    Parkour,
    /// Sneak off the edge placing the block to stand on, as Baritone's
    /// `MovementTraverse` does over a gap.
    Bridge,
    /// Jump straight up placing a block under the feet, as Baritone's
    /// `MovementPillar`.
    Pillar,
    /// Up or down one block inside a ladder or vine.
    Climb,
}

/// One way to leave a node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub target: BlockPos,
    pub kind: MoveKind,
    pub cost: f32,
}

const CARDINALS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

/// The cardinal direction a quarter turn clockwise from `(x, z)`.
fn right_of((x, z): (i32, i32)) -> (i32, i32) {
    (-z, x)
}

/// Every move out of `pos`.
pub fn successors(world: &CachedWorld, pos: BlockPos) -> Vec<Edge> {
    let mut edges = Vec::with_capacity(16);
    forward_move(world, pos, &mut edges);
    ascend_move(world, pos, &mut edges);
    climb_move(world, pos, &mut edges);
    descend_move(world, pos, &mut edges);
    diagonal_move(world, pos, &mut edges);
    downward_move(world, pos, &mut edges);
    if world.allow_parkour {
        parkour_move(world, pos, &mut edges);
    }
    if world.allow_place {
        bridge_move(world, pos, &mut edges);
        pillar_move(world, pos, &mut edges);
    }
    edges
}

fn forward_move(world: &CachedWorld, pos: BlockPos, edges: &mut Vec<Edge>) {
    // the water cost is applied when leaving the water rather than entering
    let currently_in_water = world.is_block_water(pos);
    let base_cost = if currently_in_water {
        WALK_ONE_IN_WATER_COST
    } else {
        SPRINT_ONE_BLOCK_COST
    };

    for (dx, dz) in CARDINALS {
        let new_pos = pos.offset(dx, 0, dz);
        let break_cost = if currently_in_water {
            if !world.is_block_water(new_pos) {
                continue;
            }
            world.cost_for_breaking_block(new_pos.up(1))
        } else {
            world.cost_for_standing(new_pos)
        };
        if break_cost == f32::INFINITY {
            continue;
        }
        edges.push(Edge {
            target: new_pos,
            kind: MoveKind::Forward,
            cost: base_cost + break_cost,
        });
    }
}

fn ascend_move(world: &CachedWorld, pos: BlockPos, edges: &mut Vec<Edge>) {
    // jumping up needs a full block to jump from; Azalea also allows stairs
    // facing the right way, which the cells don't describe yet
    if !world.is_block_solid(pos.down(1)) && !world.is_block_water(pos) && !world.is_block_climbable(pos) {
        return;
    }

    let break_cost_1 = world.cost_for_breaking_block(pos.up(2));
    if break_cost_1 == f32::INFINITY {
        return;
    }
    let base_cost =
        f32::max(WALK_ONE_BLOCK_COST, *JUMP_ONE_BLOCK_COST) + JUMP_PENALTY + break_cost_1;

    for (dx, dz) in CARDINALS {
        let target = pos.offset(dx, 1, dz);
        let break_cost_2 = world.cost_for_standing(target);
        if break_cost_2 == f32::INFINITY {
            continue;
        }
        edges.push(Edge {
            target,
            kind: MoveKind::Ascend,
            cost: base_cost + break_cost_2,
        });
    }
}

/// Up and down inside a ladder or vine, a block at a time; the sides are
/// left by the ordinary moves.
fn climb_move(world: &CachedWorld, pos: BlockPos, edges: &mut Vec<Edge>) {
    if !world.is_block_climbable(pos) {
        return;
    }
    let up = pos.up(1);
    if world.is_block_climbable(up) && world.is_block_passable(up.up(1)) {
        edges.push(Edge {
            target: up,
            kind: MoveKind::Climb,
            cost: LADDER_UP_ONE_COST,
        });
    }
    let down = pos.down(1);
    if (world.is_block_climbable(down) || world.is_block_standable(down.down(1))) && world.is_block_passable(down) {
        edges.push(Edge {
            target: down,
            kind: MoveKind::Climb,
            cost: LADDER_DOWN_ONE_COST,
        });
    }
}

fn descend_move(world: &CachedWorld, pos: BlockPos, edges: &mut Vec<Edge>) {
    for (dx, dz) in CARDINALS {
        let new_horizontal = pos.offset(dx, 0, dz);

        let break_cost_1 = world.cost_for_passing(new_horizontal);
        if break_cost_1 == f32::INFINITY {
            continue;
        }

        let mut fall_distance = world.fall_distance(new_horizontal);
        if fall_distance > 3 {
            continue;
        }

        let mut into_water = false;
        if fall_distance == 0 {
            if world.is_block_water(new_horizontal.down(1)) {
                fall_distance = 1;
                into_water = true;
            } else {
                continue;
            }
        }

        let target = new_horizontal.down(fall_distance as i32);

        // only mine when descending one block
        let break_cost_2 = if into_water {
            let cost = world.cost_for_breaking_block(target.up(1));
            if cost == f32::INFINITY {
                continue;
            }
            cost + ENTER_WATER_PENALTY
        } else if fall_distance == 1 {
            let cost = world.cost_for_standing(target);
            if cost == f32::INFINITY {
                continue;
            }
            cost
        } else {
            if !world.is_standable(target) {
                continue;
            }
            0.
        };

        let cost = WALK_OFF_BLOCK_COST
            + f32::max(FALL_N_BLOCKS_COST[fall_distance as usize], CENTER_AFTER_FALL_COST)
            + break_cost_1
            + break_cost_2;
        edges.push(Edge {
            target,
            kind: MoveKind::Descend,
            cost,
        });
    }
}

fn diagonal_move(world: &CachedWorld, pos: BlockPos, edges: &mut Vec<Edge>) {
    let currently_in_water = world.is_block_water(pos);
    let base_cost = if currently_in_water {
        WALK_ONE_IN_WATER_COST
    } else {
        SPRINT_ONE_BLOCK_COST
    };
    // 0.001 as a tie-breaker to avoid going diagonal for nothing
    let base_cost = base_cost.mul_add(SQRT_2, 0.001);

    for dir in CARDINALS {
        let right = right_of(dir);
        let left_pos = pos.offset(dir.0, 0, dir.1);
        let right_pos = pos.offset(right.0, 0, right.1);
        let target = pos.offset(dir.0 + right.0, 0, dir.1 + right.1);

        let (left_passable, right_passable) = if currently_in_water {
            let water_ok = |p: BlockPos| world.is_block_water(p) && world.is_block_passable(p.up(1));
            // don't bother hugging corners while in water
            if !water_ok(left_pos) || !water_ok(right_pos) {
                continue;
            }
            (true, true)
        } else {
            let l = world.is_passable(left_pos);
            let r = world.is_passable(right_pos);
            if !l && !r {
                continue;
            }
            (l, r)
        };

        if currently_in_water {
            if !world.is_block_water(target) || !world.is_block_passable(target.up(1)) {
                continue;
            }
        } else if !world.is_standable(target) {
            continue;
        }

        let mut cost = base_cost;
        if !left_passable || !right_passable {
            // it'll probably be hugging a wall
            cost += WALK_ONE_BLOCK_COST / 2.;
        }
        edges.push(Edge {
            target,
            kind: MoveKind::Diagonal,
            cost,
        });
    }
}

/// Straight down, by mining the block underneath.
fn downward_move(world: &CachedWorld, pos: BlockPos, edges: &mut Vec<Edge>) {
    // land on something solid after breaking the block below
    if !world.is_block_solid(pos.down(2)) {
        return;
    }
    let break_cost = world.cost_for_breaking_block(pos.down(1));
    if break_cost == f32::INFINITY {
        return;
    }
    edges.push(Edge {
        target: pos.down(1),
        kind: MoveKind::Downward,
        cost: FALL_N_BLOCKS_COST[1] + break_cost,
    });
}

/// One block forward over thin air, placing the floor from the one stood on.
fn bridge_move(world: &CachedWorld, pos: BlockPos, edges: &mut Vec<Edge>) {
    if !world.is_block_standable(pos.down(1)) || world.is_block_water(pos) {
        return;
    }
    for (dx, dz) in CARDINALS {
        let new_pos = pos.offset(dx, 0, dz);
        let floor = world.cell(new_pos.down(1));
        // placed into open air: nothing to stand on, no fluid, not a wall
        if floor.standable || !floor.passable || floor.liquid || !world.is_passable(new_pos) {
            continue;
        }
        edges.push(Edge {
            target: new_pos,
            kind: MoveKind::Bridge,
            cost: SNEAK_ONE_BLOCK_COST + PLACE_BLOCK_COST,
        });
    }
}

/// One block up on a block placed at the feet while jumping.
fn pillar_move(world: &CachedWorld, pos: BlockPos, edges: &mut Vec<Edge>) {
    if !world.is_block_standable(pos.down(1))
        || world.is_block_water(pos)
        || !world.is_passable(pos.up(1))
        || !world.is_block_passable(pos.up(3))
    {
        return;
    }
    edges.push(Edge {
        target: pos.up(1),
        kind: MoveKind::Pillar,
        cost: *JUMP_ONE_BLOCK_COST + PLACE_BLOCK_COST + JUMP_PENALTY,
    });
}

/// The longest gap jump with a block to run up on: the player lands four
/// blocks from where they left (three missing blocks).
const PARKOUR_MAX_RUN: i32 = 4;
/// Without a run-up the sprint is short: three blocks.
const PARKOUR_MAX_STANDING: i32 = 3;

/// Baritone's `MovementParkour`: a sprint jump over missing floor to the
/// first block that can be landed on, in line with the jump, flat or one
/// higher. Only from a real edge: where the next block has a floor, walking
/// does it.
fn parkour_move(world: &CachedWorld, pos: BlockPos, edges: &mut Vec<Edge>) {
    if !world.is_block_standable(pos.down(1))
        || world.is_block_water(pos)
        || !world.is_block_passable(pos.up(2))
    {
        return;
    }
    for (dx, dz) in CARDINALS {
        let at = |k: i32, up: i32| pos.offset(dx * k, up, dz * k);
        // a gap: the block past the edge has nothing to stand on
        if world.is_block_standable(at(1, -1)) || !world.is_passable(at(1, 0)) {
            continue;
        }
        // two blocks to run on, as the sprint has to get its speed
        let run_up = world.is_standable(pos.offset(-dx, 0, -dz)) && world.is_standable(pos.offset(-2 * dx, 0, -2 * dz));
        let max = if run_up { PARKOUR_MAX_RUN } else { PARKOUR_MAX_STANDING };

        // flat first, then one up (a shorter jump, it costs height)
        let mut flat_clear = true;
        let mut landed = false;
        for i in 2..=max {
            // the flight passes over the cell before
            let before = at(i - 1, 0);
            flat_clear &= world.is_block_passable(before)
                && world.is_block_passable(before.up(1))
                && world.is_block_passable(before.up(2));
            if !flat_clear {
                break;
            }
            if world.is_standable(at(i, 0)) && world.is_block_passable(at(i, 2)) {
                edges.push(Edge {
                    target: at(i, 0),
                    kind: MoveKind::Parkour,
                    cost: i as f32 * SPRINT_ONE_BLOCK_COST + JUMP_PENALTY,
                });
                landed = true;
                break;
            }
        }
        if landed {
            continue;
        }
        if !world.is_block_passable(pos.up(3)) {
            continue;
        }
        let mut up_clear = true;
        for i in 2..max {
            let before = at(i - 1, 0);
            up_clear &= world.is_block_passable(before)
                && world.is_block_passable(before.up(1))
                && world.is_block_passable(before.up(2))
                && world.is_block_passable(before.up(3));
            if !up_clear {
                break;
            }
            let landing = at(i, 1);
            if world.is_standable(landing) && world.is_block_passable(landing.up(2)) {
                edges.push(Edge {
                    target: landing,
                    kind: MoveKind::Parkour,
                    cost: i as f32 * SPRINT_ONE_BLOCK_COST + 2. * JUMP_PENALTY,
                });
                break;
            }
        }
    }
}

/// The blocks a move has to break, in the order the executor mines them.
pub fn blocks_to_mine(kind: MoveKind, start: BlockPos, target: BlockPos) -> Vec<BlockPos> {
    match kind {
        MoveKind::Forward => vec![target.up(1), target],
        MoveKind::Ascend => vec![start.up(2), target, target.up(1)],
        MoveKind::Descend => (0..=(start.y - target.y + 1)).rev().map(|i| target.up(i)).collect(),
        MoveKind::Diagonal => vec![],
        MoveKind::Downward => vec![target],
        MoveKind::Parkour | MoveKind::Bridge | MoveKind::Pillar | MoveKind::Climb => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Cell, test_world::TestWorld};

    fn find(edges: &[Edge], target: BlockPos) -> Option<Edge> {
        edges.iter().copied().find(|e| e.target == target)
    }

    #[test]
    fn flat_ground_has_four_forward_and_four_diagonal_moves() {
        let world = TestWorld::flat(3);
        let cached = CachedWorld::new(&world, false);
        let edges = successors(&cached, BlockPos::new(0, 64, 0));
        let forward = edges.iter().filter(|e| e.kind == MoveKind::Forward).count();
        let diagonal = edges.iter().filter(|e| e.kind == MoveKind::Diagonal).count();
        assert_eq!(forward, 4);
        assert_eq!(diagonal, 4);
        assert_eq!(edges.len(), 8);
        let e = find(&edges, BlockPos::new(1, 64, 0)).unwrap();
        assert_eq!(e.cost, SPRINT_ONE_BLOCK_COST);
        assert!(find(&edges, BlockPos::new(1, 64, 1)).unwrap().cost > e.cost);
    }

    #[test]
    fn ascends_onto_a_step() {
        let mut world = TestWorld::flat(3);
        world.stone(BlockPos::new(1, 64, 0));
        let cached = CachedWorld::new(&world, false);
        let edges = successors(&cached, BlockPos::new(0, 64, 0));
        let up = find(&edges, BlockPos::new(1, 65, 0)).unwrap();
        assert_eq!(up.kind, MoveKind::Ascend);
        assert!(find(&edges, BlockPos::new(1, 64, 0)).is_none());
    }

    #[test]
    fn no_ascent_under_a_ceiling_without_mining() {
        let mut world = TestWorld::flat(3);
        world.stone(BlockPos::new(1, 64, 0));
        world.stone(BlockPos::new(0, 66, 0));
        let cached = CachedWorld::new(&world, false);
        let edges = successors(&cached, BlockPos::new(0, 64, 0));
        assert!(find(&edges, BlockPos::new(1, 65, 0)).is_none());
        let cached = CachedWorld::new(&world, true);
        let edges = successors(&cached, BlockPos::new(0, 64, 0));
        assert!(find(&edges, BlockPos::new(1, 65, 0)).unwrap().cost > 30.0);
    }

    #[test]
    fn descends_up_to_three_blocks() {
        let mut world = TestWorld::flat(3);
        // a pit at x = 1 with its floor at y = 60 (fall 3)
        for y in 61..=63 {
            world.air(BlockPos::new(1, y, 0));
        }
        world.stone(BlockPos::new(1, 60, 0));
        let cached = CachedWorld::new(&world, false);
        let edges = successors(&cached, BlockPos::new(0, 64, 0));
        let down = find(&edges, BlockPos::new(1, 61, 0)).unwrap();
        assert_eq!(down.kind, MoveKind::Descend);
        assert_eq!(
            down.cost,
            WALK_OFF_BLOCK_COST + FALL_N_BLOCKS_COST[3]
        );

        // one deeper and it's too far
        world.air(BlockPos::new(1, 60, 0));
        world.stone(BlockPos::new(1, 59, 0));
        let cached = CachedWorld::new(&world, false);
        let edges = successors(&cached, BlockPos::new(0, 64, 0));
        assert!(edges.iter().all(|e| e.target.x != 1 || e.target.z != 0));
    }

    #[test]
    fn descends_into_water() {
        let mut world = TestWorld::flat(3);
        world.set(BlockPos::new(1, 63, 0), Cell::WATER, "water");
        let cached = CachedWorld::new(&world, false);
        let edges = successors(&cached, BlockPos::new(0, 64, 0));
        let down = find(&edges, BlockPos::new(1, 63, 0)).unwrap();
        assert_eq!(down.kind, MoveKind::Descend);
        assert!(down.cost > WALK_OFF_BLOCK_COST + ENTER_WATER_PENALTY);
    }

    #[test]
    fn swims_forward_in_water_only() {
        let mut world = TestWorld::flat(3);
        world.set(BlockPos::new(0, 64, 0), Cell::WATER, "water");
        world.set(BlockPos::new(1, 64, 0), Cell::WATER, "water");
        let cached = CachedWorld::new(&world, false);
        let edges = successors(&cached, BlockPos::new(0, 64, 0));
        let swim = find(&edges, BlockPos::new(1, 64, 0)).unwrap();
        assert_eq!(swim.cost, WALK_ONE_IN_WATER_COST);
        assert!(find(&edges, BlockPos::new(-1, 64, 0)).is_none());
    }

    #[test]
    fn diagonal_needs_one_open_side() {
        let mut world = TestWorld::flat(3);
        world.stone(BlockPos::new(1, 64, 0));
        world.stone(BlockPos::new(1, 65, 0));
        let cached = CachedWorld::new(&world, false);
        let edges = successors(&cached, BlockPos::new(0, 64, 0));
        let hug = find(&edges, BlockPos::new(1, 64, 1)).unwrap();
        assert_eq!(hug.kind, MoveKind::Diagonal);
        assert!(hug.cost > SPRINT_ONE_BLOCK_COST * SQRT_2 + WALK_ONE_BLOCK_COST / 2. - 0.01);

        world.stone(BlockPos::new(0, 64, 1));
        let cached = CachedWorld::new(&world, false);
        let edges = successors(&cached, BlockPos::new(0, 64, 0));
        assert!(find(&edges, BlockPos::new(1, 64, 1)).is_none());
    }

    #[test]
    fn mines_down_onto_solid_ground() {
        let mut world = TestWorld::flat(3);
        world.stone(BlockPos::new(0, 62, 0));
        let cached = CachedWorld::new(&world, true);
        let edges = successors(&cached, BlockPos::new(0, 64, 0));
        let down = find(&edges, BlockPos::new(0, 63, 0)).unwrap();
        assert_eq!(down.kind, MoveKind::Downward);
        let cached = CachedWorld::new(&world, false);
        assert!(find(&successors(&cached, BlockPos::new(0, 64, 0)), BlockPos::new(0, 63, 0)).is_none());
    }

    #[test]
    fn forward_mines_through_a_wall() {
        let mut world = TestWorld::flat(3);
        world.stone(BlockPos::new(1, 64, 0));
        world.stone(BlockPos::new(1, 65, 0));
        let cached = CachedWorld::new(&world, true);
        let edges = successors(&cached, BlockPos::new(0, 64, 0));
        let e = find(&edges, BlockPos::new(1, 64, 0)).unwrap();
        assert_eq!(e.kind, MoveKind::Forward);
        assert_eq!(
            e.cost,
            SPRINT_ONE_BLOCK_COST + 2. * (30.0 + BLOCK_BREAK_ADDITIONAL_PENALTY)
        );
    }

    #[test]
    fn blocks_to_mine_follow_azalea_order() {
        let s = BlockPos::new(0, 64, 0);
        assert_eq!(
            blocks_to_mine(MoveKind::Forward, s, BlockPos::new(1, 64, 0)),
            vec![BlockPos::new(1, 65, 0), BlockPos::new(1, 64, 0)]
        );
        assert_eq!(
            blocks_to_mine(MoveKind::Descend, s, BlockPos::new(1, 63, 0)),
            vec![BlockPos::new(1, 65, 0), BlockPos::new(1, 64, 0), BlockPos::new(1, 63, 0)]
        );
    }

    /// A flat world with the floor cut out for x in `from..=to` at z = 0.
    fn gap(from: i32, to: i32) -> TestWorld {
        let mut world = TestWorld::flat(10);
        for x in from..=to {
            world.air(BlockPos::new(x, 63, 0));
        }
        world
    }

    fn parkour(world: &TestWorld, from: BlockPos) -> Vec<Edge> {
        let cached = CachedWorld::new(world, false);
        successors(&cached, from)
            .into_iter()
            .filter(|e| e.kind == MoveKind::Parkour)
            .collect()
    }

    #[test]
    fn jumps_a_one_block_gap() {
        let world = gap(1, 1);
        let edges = parkour(&world, BlockPos::new(0, 64, 0));
        let jump = find(&edges, BlockPos::new(2, 64, 0)).expect("lands on the far side");
        assert_eq!(edges.len(), 1);
        assert_eq!(jump.cost, 2. * SPRINT_ONE_BLOCK_COST + JUMP_PENALTY);
    }

    #[test]
    fn a_floor_next_to_the_start_is_walked_not_jumped() {
        let world = TestWorld::flat(10);
        assert!(parkour(&world, BlockPos::new(0, 64, 0)).is_empty());
    }

    #[test]
    fn a_long_gap_needs_a_run_up() {
        // a three block gap: four blocks from edge to landing
        let world = gap(1, 3);
        // with a floor behind the start there is a run-up
        let edges = parkour(&world, BlockPos::new(0, 64, 0));
        assert!(find(&edges, BlockPos::new(4, 64, 0)).is_some(), "{edges:?}");
        // with nothing behind, the sprint is too short
        let mut world = gap(1, 3);
        world.air(BlockPos::new(-1, 63, 0));
        let edges = parkour(&world, BlockPos::new(0, 64, 0));
        assert!(edges.iter().all(|e| e.target.x < 0), "{edges:?}");
        // and one block behind is too little too
        let mut world = gap(1, 3);
        world.air(BlockPos::new(-2, 63, 0));
        let edges = parkour(&world, BlockPos::new(0, 64, 0));
        assert!(edges.iter().all(|e| e.target.x < 0), "{edges:?}");
    }

    #[test]
    fn too_wide_a_gap_is_not_jumped() {
        let world = gap(1, 4);
        assert!(parkour(&world, BlockPos::new(0, 64, 0)).is_empty());
    }

    #[test]
    fn jumps_up_a_block_over_a_gap() {
        let mut world = gap(1, 1);
        world.stone(BlockPos::new(2, 64, 0));
        let edges = parkour(&world, BlockPos::new(0, 64, 0));
        let up = find(&edges, BlockPos::new(2, 65, 0)).expect("lands one higher");
        assert!(up.cost > 2. * SPRINT_ONE_BLOCK_COST + JUMP_PENALTY);
    }

    #[test]
    fn a_low_ceiling_stops_the_jump() {
        let mut world = gap(1, 1);
        world.stone(BlockPos::new(1, 66, 0));
        assert!(parkour(&world, BlockPos::new(0, 64, 0)).is_empty());
        let mut world = gap(1, 1);
        world.stone(BlockPos::new(0, 66, 0));
        assert!(parkour(&world, BlockPos::new(0, 64, 0)).is_empty());
    }

    #[test]
    fn a_wall_in_the_flight_stops_the_jump() {
        let mut world = gap(1, 2);
        world.stone(BlockPos::new(2, 64, 0));
        // landing on x = 3 would cross the wall at x = 2
        let edges = parkour(&world, BlockPos::new(0, 64, 0));
        assert!(find(&edges, BlockPos::new(3, 64, 0)).is_none());
    }

    #[test]
    fn jumps_over_lava_but_lands_on_stone() {
        let mut world = gap(1, 1);
        world.set(BlockPos::new(1, 63, 0), Cell::LAVA, "lava");
        let edges = parkour(&world, BlockPos::new(0, 64, 0));
        assert!(find(&edges, BlockPos::new(2, 64, 0)).is_some());
        // but not onto it
        let mut world = gap(1, 1);
        world.set(BlockPos::new(2, 63, 0), Cell::LAVA, "lava");
        let edges = parkour(&world, BlockPos::new(0, 64, 0));
        assert!(find(&edges, BlockPos::new(2, 64, 0)).is_none(), "{edges:?}");
    }

    #[test]
    fn jumps_in_all_four_directions() {
        let mut world = TestWorld::flat(10);
        for (x, z) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            world.air(BlockPos::new(x, 63, z));
        }
        let edges = parkour(&world, BlockPos::new(0, 64, 0));
        assert_eq!(edges.len(), 4);
    }

    fn placing(world: &TestWorld, from: BlockPos) -> Vec<Edge> {
        let cached = CachedWorld::new(world, false).with_moves(true, true);
        successors(&cached, from)
    }

    #[test]
    fn placing_is_off_without_blocks() {
        let world = gap(1, 1);
        let cached = CachedWorld::new(&world, false);
        let edges = successors(&cached, BlockPos::new(0, 64, 0));
        assert!(edges.iter().all(|e| e.kind != MoveKind::Bridge && e.kind != MoveKind::Pillar));
    }

    #[test]
    fn bridges_over_a_gap_and_pillars_up() {
        let world = gap(1, 1);
        let edges = placing(&world, BlockPos::new(0, 64, 0));
        let bridge = find(&edges, BlockPos::new(1, 64, 0)).expect("bridges the gap");
        assert_eq!(bridge.kind, MoveKind::Bridge);
        assert_eq!(bridge.cost, SNEAK_ONE_BLOCK_COST + PLACE_BLOCK_COST);
        let pillar = find(&edges, BlockPos::new(0, 65, 0)).expect("pillars up");
        assert_eq!(pillar.kind, MoveKind::Pillar);
        // over a floor it just walks
        let flat = TestWorld::flat(5);
        let edges = placing(&flat, BlockPos::new(0, 64, 0));
        assert!(edges.iter().all(|e| e.kind != MoveKind::Bridge));
        assert_eq!(find(&edges, BlockPos::new(1, 64, 0)).unwrap().kind, MoveKind::Forward);
    }

    #[test]
    fn does_not_bridge_into_water_lava_or_a_wall() {
        let mut world = gap(1, 1);
        world.set(BlockPos::new(1, 63, 0), Cell::WATER, "water");
        let edges = placing(&world, BlockPos::new(0, 64, 0));
        assert!(edges.iter().all(|e| e.target != BlockPos::new(1, 64, 0) || e.kind != MoveKind::Bridge));
        let mut world = gap(1, 1);
        world.set(BlockPos::new(1, 63, 0), Cell::LAVA, "lava");
        let edges = placing(&world, BlockPos::new(0, 64, 0));
        assert!(edges.iter().all(|e| e.kind != MoveKind::Bridge || e.target.x != 1));
        let mut world = gap(1, 1);
        world.stone(BlockPos::new(1, 65, 0));
        let edges = placing(&world, BlockPos::new(0, 64, 0));
        assert!(edges.iter().all(|e| e.kind != MoveKind::Bridge || e.target.x != 1));
    }

    #[test]
    fn no_pillar_under_a_ceiling_or_without_ground() {
        let mut world = TestWorld::flat(5);
        world.stone(BlockPos::new(0, 66, 0));
        let edges = placing(&world, BlockPos::new(0, 64, 0));
        assert!(edges.iter().all(|e| e.kind != MoveKind::Pillar));
        // standing on nothing (falling): nothing to place against
        let mut world = TestWorld::flat(5);
        world.air(BlockPos::new(0, 63, 0));
        let edges = placing(&world, BlockPos::new(0, 64, 0));
        assert!(edges.iter().all(|e| e.kind != MoveKind::Pillar));
    }

    #[test]
    fn parkour_can_be_switched_off() {
        let world = gap(1, 1);
        let cached = CachedWorld::new(&world, false).with_moves(false, false);
        let edges = successors(&cached, BlockPos::new(0, 64, 0));
        assert!(edges.iter().all(|e| e.kind != MoveKind::Parkour));
    }

    fn ladder() -> Cell {
        Cell {
            climbable: true,
            ..Cell::AIR
        }
    }

    /// A ladder of `height` cells at x=1 on flat ground.
    fn ladder_world(height: i32) -> TestWorld {
        let mut world = TestWorld::flat(6);
        for y in 64..64 + height {
            world.set(BlockPos::new(1, y, 0), ladder(), "ladder");
        }
        world
    }

    #[test]
    fn a_ladder_is_climbed_up_one_block_at_a_time() {
        let world = ladder_world(4);
        let cached = CachedWorld::new(&world, false);
        let edges = successors(&cached, BlockPos::new(1, 64, 0));
        let up = find(&edges, BlockPos::new(1, 65, 0)).unwrap();
        assert_eq!(up.kind, MoveKind::Climb);
        assert_eq!(up.cost, LADDER_UP_ONE_COST);
        // the ground is below: no way down
        assert!(find(&edges, BlockPos::new(1, 63, 0)).is_none());
    }

    #[test]
    fn a_ladder_is_climbed_down_slower_than_up_is_not() {
        let world = ladder_world(4);
        let cached = CachedWorld::new(&world, false);
        let edges = successors(&cached, BlockPos::new(1, 66, 0));
        let down = find(&edges, BlockPos::new(1, 65, 0)).unwrap();
        assert_eq!((down.kind, down.cost), (MoveKind::Climb, LADDER_DOWN_ONE_COST));
    }

    #[test]
    fn the_top_of_a_ladder_stops_climbing() {
        let world = ladder_world(3);
        let cached = CachedWorld::new(&world, false);
        let edges = successors(&cached, BlockPos::new(1, 66, 0));
        assert!(find(&edges, BlockPos::new(1, 67, 0)).is_none());
    }

    #[test]
    fn a_ceiling_over_a_ladder_stops_climbing() {
        let mut world = ladder_world(5);
        world.stone(BlockPos::new(1, 69, 0));
        let cached = CachedWorld::new(&world, false);
        // head room: the cell above the target must be free
        let edges = successors(&cached, BlockPos::new(1, 67, 0));
        assert!(find(&edges, BlockPos::new(1, 68, 0)).is_none());
        let edges = successors(&cached, BlockPos::new(1, 66, 0));
        assert!(find(&edges, BlockPos::new(1, 67, 0)).is_some());
    }

    #[test]
    fn plain_air_cannot_be_climbed() {
        let world = TestWorld::flat(4);
        let cached = CachedWorld::new(&world, false);
        let edges = successors(&cached, BlockPos::new(0, 64, 0));
        assert!(edges.iter().all(|e| e.kind != MoveKind::Climb));
    }

    #[test]
    fn a_ladder_cell_can_be_walked_into_and_stepped_off_onto_a_ledge() {
        let mut world = ladder_world(4);
        // a ledge one block above the ladder's top, next to it
        world.stone(BlockPos::new(2, 64, 0));
        world.stone(BlockPos::new(2, 65, 0));
        world.stone(BlockPos::new(2, 66, 0));
        world.stone(BlockPos::new(2, 67, 0));
        let cached = CachedWorld::new(&world, false);
        // onto the ladder from the ground
        let from_ground = successors(&cached, BlockPos::new(0, 64, 0));
        assert_eq!(find(&from_ground, BlockPos::new(1, 64, 0)).unwrap().kind, MoveKind::Forward);
        // off its top onto the ledge a block higher, with no floor of its own
        let from_top = successors(&cached, BlockPos::new(1, 67, 0));
        assert_eq!(find(&from_top, BlockPos::new(2, 68, 0)).unwrap().kind, MoveKind::Ascend);
    }

    #[test]
    fn a_ladder_is_climbed_down_to_the_ground_not_below_it() {
        let world = ladder_world(2);
        let cached = CachedWorld::new(&world, false);
        let edges = successors(&cached, BlockPos::new(1, 65, 0));
        let down = find(&edges, BlockPos::new(1, 64, 0)).unwrap();
        assert_eq!(down.kind, MoveKind::Climb);
    }

    #[test]
    fn a_fence_is_not_stepped_onto() {
        // a 1.5 high fence: no floor to jump up to, so no ascend onto it
        let mut world = TestWorld::flat(4);
        world.set(
            BlockPos::new(1, 64, 0),
            Cell {
                passable: false,
                ..Cell::AIR
            },
            "oak_fence",
        );
        let cached = CachedWorld::new(&world, false);
        let edges = successors(&cached, BlockPos::new(0, 64, 0));
        assert!(find(&edges, BlockPos::new(1, 65, 0)).is_none());
        assert!(find(&edges, BlockPos::new(1, 64, 0)).is_none());
    }
}
