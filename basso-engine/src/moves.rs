//! The moves the pathfinder can make from a node, ported from Azalea's
//! `moves/basic.rs`. Parkour and stair ascents are left out for now.

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
    descend_move(world, pos, &mut edges);
    diagonal_move(world, pos, &mut edges);
    downward_move(world, pos, &mut edges);
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
    if !world.is_block_solid(pos.down(1)) && !world.is_block_water(pos) {
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

/// The blocks a move has to break, in the order the executor mines them.
pub fn blocks_to_mine(kind: MoveKind, start: BlockPos, target: BlockPos) -> Vec<BlockPos> {
    match kind {
        MoveKind::Forward => vec![target.up(1), target],
        MoveKind::Ascend => vec![start.up(2), target, target.up(1)],
        MoveKind::Descend => (0..=(start.y - target.y + 1)).rev().map(|i| target.up(i)).collect(),
        MoveKind::Diagonal => vec![],
        MoveKind::Downward => vec![target],
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
}
