//! What to outline in the world while basso works, after Baritone's
//! `PathRenderer`: the path in red, blocks it will break in red, the goal in
//! green. The game draws outlines of existing blocks, so a path node (air) is
//! shown by its floor.

use crate::{goals::Goal, pos::BlockPos};

/// Baritone's `colorCurrentPath`.
pub const PATH_COLOR: u32 = 0xFFFF_0000;
/// Baritone's `colorBlocksToBreak`.
pub const BREAK_COLOR: u32 = 0xFFFF_0000;
/// Baritone's `colorGoalBox`.
pub const GOAL_COLOR: u32 = 0xFF00_FF00;

/// How far from the player outlines may be (Horizon draws within 32).
pub const RANGE: i32 = 31;
/// How many outlines one mod gets a frame.
pub const MAX_RIMS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rim {
    pub pos: BlockPos,
    pub argb: u32,
    pub width: f32,
}

/// The block whose outline marks the goal, if it has one place.
pub fn goal_block(goal: &Goal) -> Option<BlockPos> {
    match *goal {
        // the player stands in the goal, so outline what they stand on
        Goal::Block(p) => Some(p.down(1)),
        Goal::TwoBlocks(p) | Goal::GetToBlock(p) => Some(p),
        Goal::Radius { pos, .. } => Some(BlockPos::new(
            pos.x.floor() as i32,
            pos.y.floor() as i32 - 1,
            pos.z.floor() as i32,
        )),
        Goal::XZ { .. } | Goal::Y(_) => None,
    }
}

/// The outlines to draw, most important first, within range of `player`
/// and no more than [`MAX_RIMS`].
pub fn rims(
    path: &[BlockPos],
    to_break: &[BlockPos],
    goal: Option<&Goal>,
    player: BlockPos,
) -> Vec<Rim> {
    let near = |p: &BlockPos| {
        (p.x - player.x).abs() <= RANGE
            && (p.y - player.y).abs() <= RANGE
            && (p.z - player.z).abs() <= RANGE
    };
    let mut out: Vec<Rim> = Vec::new();
    let mut push = |pos: BlockPos, argb: u32, width: f32| {
        if near(&pos) && out.len() < MAX_RIMS && !out.iter().any(|r| r.pos == pos) {
            out.push(Rim { pos, argb, width });
        }
    };
    for &pos in to_break {
        push(pos, BREAK_COLOR, 3.0);
    }
    if let Some(pos) = goal.and_then(goal_block) {
        push(pos, GOAL_COLOR, 3.0);
    }
    for &node in path {
        push(node.down(1), PATH_COLOR, 2.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pos::Vec3;

    const ORIGIN: BlockPos = BlockPos::new(0, 64, 0);

    #[test]
    fn path_is_drawn_on_its_floor() {
        let path = [BlockPos::new(0, 64, 0), BlockPos::new(1, 64, 0)];
        let rims = rims(&path, &[], None, ORIGIN);
        assert_eq!(rims.len(), 2);
        assert_eq!(rims[0].pos, BlockPos::new(0, 63, 0));
        assert_eq!(rims[1].pos, BlockPos::new(1, 63, 0));
        assert!(rims.iter().all(|r| r.argb == PATH_COLOR));
    }

    #[test]
    fn breaks_and_goal_come_first() {
        let path = [BlockPos::new(1, 64, 0)];
        let to_break = [BlockPos::new(2, 64, 0)];
        let goal = Goal::Block(BlockPos::new(3, 64, 0));
        let rims = rims(&path, &to_break, Some(&goal), ORIGIN);
        assert_eq!(rims[0].pos, BlockPos::new(2, 64, 0));
        assert_eq!(rims[1].pos, BlockPos::new(3, 63, 0));
        assert_eq!(rims[1].argb, GOAL_COLOR);
        assert_eq!(rims[2].argb, PATH_COLOR);
    }

    #[test]
    fn far_blocks_are_left_out() {
        let path = [BlockPos::new(40, 64, 0), BlockPos::new(5, 64, 0)];
        let rims = rims(&path, &[], None, ORIGIN);
        assert_eq!(rims.len(), 1);
        assert_eq!(rims[0].pos, BlockPos::new(5, 63, 0));
    }

    #[test]
    fn no_more_than_the_budget_and_no_duplicates() {
        let path: Vec<_> = (0..20)
            .flat_map(|x| (0..20).map(move |z| BlockPos::new(x, 64, z)))
            .collect();
        assert_eq!(rims(&path, &[], None, ORIGIN).len(), MAX_RIMS);
        let twice = [BlockPos::new(1, 64, 0), BlockPos::new(1, 64, 0)];
        assert_eq!(rims(&twice, &[], None, ORIGIN).len(), 1);
    }

    #[test]
    fn goal_blocks() {
        let p = BlockPos::new(1, 2, 3);
        assert_eq!(goal_block(&Goal::Block(p)), Some(BlockPos::new(1, 1, 3)));
        assert_eq!(goal_block(&Goal::TwoBlocks(p)), Some(p));
        assert_eq!(goal_block(&Goal::GetToBlock(p)), Some(p));
        assert_eq!(goal_block(&Goal::Y(5)), None);
        assert_eq!(goal_block(&Goal::XZ { x: 1, z: 2 }), None);
        let radius = Goal::Radius {
            pos: Vec3::new(1.5, 64.0, -0.5),
            radius: 3.0,
        };
        assert_eq!(goal_block(&radius), Some(BlockPos::new(1, 63, -1)));
    }
}
