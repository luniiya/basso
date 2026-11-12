//! Where a path should end (Azalea's and Baritone's goals).

use std::f32::consts::SQRT_2;

use crate::{
    costs::{
        COST_HEURISTIC, FALL_N_BLOCKS_COST, JUMP_ONE_BLOCK_COST, JUMP_PENALTY,
        SPRINT_ONE_BLOCK_COST, WALK_ONE_BLOCK_COST,
    },
    pos::{BlockPos, Vec3},
};

/// A goal, tested against the block the player's feet are in.
#[derive(Clone, Debug, PartialEq)]
pub enum Goal {
    /// Stand with the feet in this block.
    Block(BlockPos),
    /// Reach this column at any height.
    XZ { x: i32, z: i32 },
    /// Reach this height anywhere.
    Y(i32),
    /// Get within `radius` of a point (following an entity).
    Radius { pos: Vec3, radius: f32 },
    /// Baritone's `GoalTwoBlocks`: the feet or the head in this block, so the
    /// path itself breaks it (mining).
    TwoBlocks(BlockPos),
    /// Baritone's `GoalGetToBlock`: next to the block or standing on it, to
    /// reach it without breaking it.
    GetToBlock(BlockPos),
    /// Baritone's `MineProcess.GoalThreeBlocks`: the feet in this block or
    /// one of the two below it.
    ThreeBlocks(BlockPos),
    /// Baritone's `GoalComposite`: any one of these, whichever is nearest.
    Composite(Vec<Goal>),
}

impl Goal {
    pub fn heuristic(&self, n: BlockPos) -> f32 {
        match *self {
            Goal::Composite(ref goals) => goals
                .iter()
                .map(|g| g.heuristic(n))
                .fold(f32::INFINITY, f32::min),
            Goal::Block(p) => {
                xz_heuristic((p.x - n.x) as f32, (p.z - n.z) as f32)
                    + y_heuristic((p.y - n.y) as f32)
            }
            Goal::XZ { x, z } => xz_heuristic((x - n.x) as f32, (z - n.z) as f32),
            Goal::Y(y) => y_heuristic((y - n.y) as f32),
            Goal::Radius { pos, .. } => {
                let dx = (pos.x - (f64::from(n.x) + 0.5)) as f32;
                let dy = (pos.y - f64::from(n.y)) as f32;
                let dz = (pos.z - (f64::from(n.z) + 0.5)) as f32;
                xz_heuristic(dx, dz) + y_heuristic(dy)
            }
            Goal::TwoBlocks(p) => {
                // the feet are in it or just below it
                let dy = if n.y < p.y { (p.y - 1 - n.y) as f32 } else { (p.y - n.y) as f32 };
                xz_heuristic((p.x - n.x) as f32, (p.z - n.z) as f32) + y_heuristic(dy)
            }
            Goal::GetToBlock(p) => {
                xz_heuristic((p.x - n.x) as f32, (p.z - n.z) as f32)
                    + y_heuristic((p.y - n.y) as f32)
            }
            Goal::ThreeBlocks(p) => {
                // Baritone's y difference is the node's minus the goal's
                let below = n.y - p.y;
                let below = match below {
                    d if d < -1 => d + 2,
                    -1 => 0,
                    d => d,
                };
                xz_heuristic((p.x - n.x) as f32, (p.z - n.z) as f32) + y_heuristic(-below as f32)
            }
        }
    }

    pub fn success(&self, n: BlockPos) -> bool {
        match *self {
            Goal::Composite(ref goals) => goals.iter().any(|g| g.success(n)),
            Goal::Block(p) => n == p,
            Goal::XZ { x, z } => n.x == x && n.z == z,
            Goal::Y(y) => n.y == y,
            Goal::Radius { pos, radius } => {
                let center = n.bottom_center();
                center.distance_squared(pos) <= f64::from(radius * radius)
            }
            Goal::TwoBlocks(p) => n == p || n == p.down(1),
            Goal::ThreeBlocks(p) => n == p || n == p.down(1) || n == p.down(2),
            Goal::GetToBlock(p) => {
                let (dx, mut dy, dz) = (n.x - p.x, n.y - p.y, n.z - p.z);
                if dy < 0 {
                    dy += 1;
                }
                dx.abs() + dy.abs() + dz.abs() <= 1
            }
        }
    }
}

impl std::fmt::Display for Goal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Goal::Block(p) => write!(f, "{p}"),
            Goal::XZ { x, z } => write!(f, "x {x} z {z}"),
            Goal::Y(y) => write!(f, "y {y}"),
            Goal::Radius { pos, radius } => {
                write!(f, "within {radius} of {:.1} {:.1} {:.1}", pos.x, pos.y, pos.z)
            }
            Goal::TwoBlocks(p) | Goal::ThreeBlocks(p) | Goal::GetToBlock(p) => write!(f, "the block at {p}"),
            Goal::Composite(goals) => write!(f, "one of {} goals", goals.len()),
        }
    }
}

fn xz_heuristic(dx: f32, dz: f32) -> f32 {
    let x = dx.abs();
    let z = dz.abs();
    let (diagonal, straight) = if x < z { (x, z - x) } else { (z, x - z) };
    (diagonal * SQRT_2 + straight) * COST_HEURISTIC
}

fn y_heuristic(dy: f32) -> f32 {
    if dy > 0. {
        // forward+up is the main way up; take off the sprint cost the xz
        // heuristic already counted
        (f32::max(*JUMP_ONE_BLOCK_COST, WALK_ONE_BLOCK_COST) + JUMP_PENALTY - SPRINT_ONE_BLOCK_COST)
            * dy
    } else if dy < 0. {
        (FALL_N_BLOCKS_COST[2] / 2.) * -dy
    } else {
        0.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_blocks_goal_takes_the_feet_in_it_or_two_below() {
        let goal = Goal::ThreeBlocks(BlockPos::new(0, 10, 0));
        for y in 8..=10 {
            assert!(goal.success(BlockPos::new(0, y, 0)), "{y}");
            assert_eq!(goal.heuristic(BlockPos::new(0, y, 0)), 0.0, "{y}");
        }
        assert!(!goal.success(BlockPos::new(0, 7, 0)));
        assert!(!goal.success(BlockPos::new(0, 11, 0)));
        // from below it is as far as from below the lowest of the three
        assert_eq!(
            goal.heuristic(BlockPos::new(0, 5, 0)),
            Goal::Block(BlockPos::new(0, 8, 0)).heuristic(BlockPos::new(0, 5, 0))
        );
        assert_eq!(
            goal.heuristic(BlockPos::new(0, 13, 0)),
            Goal::Block(BlockPos::new(0, 10, 0)).heuristic(BlockPos::new(0, 13, 0))
        );
    }

    #[test]
    fn composite_goal_is_the_nearest_of_its_goals() {
        let a = Goal::TwoBlocks(BlockPos::new(10, 64, 0));
        let b = Goal::TwoBlocks(BlockPos::new(0, 64, 3));
        let both = Goal::Composite(vec![a.clone(), b.clone()]);
        let n = BlockPos::new(0, 64, 0);
        assert_eq!(both.heuristic(n), a.heuristic(n).min(b.heuristic(n)));
        assert_eq!(both.heuristic(n), b.heuristic(n));
        assert!(both.success(BlockPos::new(10, 64, 0)));
        assert!(both.success(BlockPos::new(0, 64, 3)));
        assert!(!both.success(n));
        assert!(!Goal::Composite(Vec::new()).success(n));
        assert_eq!(both.to_string(), "one of 2 goals");
    }

    #[test]
    fn block_goal() {
        let goal = Goal::Block(BlockPos::new(3, 64, 3));
        assert!(goal.success(BlockPos::new(3, 64, 3)));
        assert!(!goal.success(BlockPos::new(3, 65, 3)));
        assert_eq!(goal.heuristic(BlockPos::new(3, 64, 3)), 0.0);
        assert!(goal.heuristic(BlockPos::new(0, 64, 0)) > goal.heuristic(BlockPos::new(2, 64, 2)));
    }

    #[test]
    fn xz_and_y_goals_ignore_the_other_axes() {
        assert!(Goal::XZ { x: 1, z: 2 }.success(BlockPos::new(1, -40, 2)));
        assert!(Goal::Y(12).success(BlockPos::new(99, 12, -99)));
        assert!(!Goal::Y(12).success(BlockPos::new(0, 13, 0)));
    }

    #[test]
    fn two_blocks_goal_takes_feet_or_head() {
        let goal = Goal::TwoBlocks(BlockPos::new(0, 10, 0));
        assert!(goal.success(BlockPos::new(0, 10, 0)));
        assert!(goal.success(BlockPos::new(0, 9, 0)));
        assert!(!goal.success(BlockPos::new(0, 11, 0)));
        assert!(!goal.success(BlockPos::new(1, 10, 0)));
    }

    #[test]
    fn get_to_block_is_next_to_or_on_it() {
        let goal = Goal::GetToBlock(BlockPos::new(0, 10, 0));
        for ok in [
            BlockPos::new(1, 10, 0),
            BlockPos::new(0, 11, 0),
            BlockPos::new(0, 9, 1),
            BlockPos::new(0, 9, 0),
        ] {
            assert!(goal.success(ok), "{ok:?}");
        }
        for not in [BlockPos::new(1, 11, 0), BlockPos::new(2, 10, 0), BlockPos::new(0, 12, 0)] {
            assert!(!goal.success(not), "{not:?}");
        }
    }

    #[test]
    fn radius_goal() {
        let goal = Goal::Radius {
            pos: Vec3::new(0.5, 64.0, 0.5),
            radius: 2.0,
        };
        assert!(goal.success(BlockPos::new(1, 64, 1)));
        assert!(!goal.success(BlockPos::new(3, 64, 0)));
    }
}
