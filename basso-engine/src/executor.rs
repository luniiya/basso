//! Plays a path back as player input, from Azalea's move executors: each
//! tick it turns where the player is into where to look and which keys to
//! hold. The game's own physics moves the player.

use std::collections::VecDeque;

use crate::{
    astar::{Path, Step},
    moves::{self, MoveKind, blocks_to_mine},
    pos::{BlockPos, Vec3, yaw_pitch},
    world::{BlockView, CachedWorld},
};

/// Eye height of a standing player.
const EYE_HEIGHT: f64 = 1.62;
/// Ticks without reaching the next node before the path counts as stuck.
const STUCK_TICKS: u32 = 100;

/// The player as the executor sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlayerState {
    /// The feet position.
    pub pos: Vec3,
    pub velocity: Vec3,
    pub on_ground: bool,
    pub in_water: bool,
    /// The eyes are under water.
    pub eye_in_water: bool,
    /// Air left, in ticks (vanilla's 300 when full).
    pub air: i32,
}

impl PlayerState {
    pub fn eye(&self) -> Vec3 {
        Vec3::new(self.pos.x, self.pos.y + EYE_HEIGHT, self.pos.z)
    }

    /// The node the player is at.
    pub fn node(&self) -> BlockPos {
        BlockPos::of_feet(self.pos)
    }
}

/// What to do this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Controls {
    /// Yaw and pitch to look at, in degrees.
    pub look: Option<(f32, f32)>,
    pub forward: bool,
    pub sprint: bool,
    pub jump: bool,
    /// Hold attack on this block (looking at it).
    pub mine: Option<BlockPos>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Moving,
    /// Every step is done.
    Finished,
    /// No progress for a while; plan again.
    Stuck,
}

/// A path being walked.
#[derive(Debug)]
pub struct Executor {
    steps: VecDeque<Step>,
    /// The last node reached.
    start: BlockPos,
    pub is_partial: bool,
    ticks_on_step: u32,
}

impl Executor {
    pub fn new(path: Path, start: BlockPos) -> Self {
        Self {
            steps: path.steps.into(),
            start,
            is_partial: path.is_partial,
            ticks_on_step: 0,
        }
    }

    pub fn steps(&self) -> impl Iterator<Item = &Step> {
        self.steps.iter()
    }

    /// The node the path ends at.
    pub fn end(&self) -> BlockPos {
        self.steps.back().map_or(self.start, |s| s.target)
    }

    /// The last node reached.
    pub fn start(&self) -> BlockPos {
        self.start
    }

    /// Whether the next `lookahead` steps can still be made, the world having
    /// changed since the search (a block placed or broken in the way).
    pub fn is_still_valid(&self, world: &CachedWorld, lookahead: usize) -> bool {
        let mut at = self.start;
        for step in self.steps.iter().take(lookahead) {
            let possible = moves::successors(world, at)
                .iter()
                .any(|e| e.target == step.target && e.kind == step.kind);
            if !possible {
                return false;
            }
            at = step.target;
        }
        true
    }

    /// The blocks the rest of the path breaks, for drawing.
    pub fn blocks_to_break(&self, world: &dyn BlockView) -> Vec<BlockPos> {
        let mut at = self.start;
        let mut blocks = Vec::new();
        for step in &self.steps {
            for block in blocks_to_mine(step.kind, at, step.target) {
                let solid = world.cell(block).is_some_and(|c| !c.passable && !c.water);
                if solid && !blocks.contains(&block) {
                    blocks.push(block);
                }
            }
            at = step.target;
        }
        blocks
    }

    pub fn tick(
        &mut self,
        world: &dyn BlockView,
        player: &PlayerState,
        can_mine: bool,
    ) -> (Controls, Progress) {
        // skip every step already reached, in case one tick covered several
        while let Some(step) = self.steps.front() {
            if !is_reached(step, self.start, player) {
                break;
            }
            self.start = step.target;
            self.steps.pop_front();
            self.ticks_on_step = 0;
        }
        let Some(&step) = self.steps.front() else {
            return (Controls::default(), Progress::Finished);
        };

        let mut ctx = Ctx {
            world,
            player,
            start: self.start,
            target: step.target,
            can_mine,
            controls: Controls::default(),
        };
        ctx.execute(step.kind);

        // mining can take a long time and is progress
        if ctx.controls.mine.is_none() {
            self.ticks_on_step += 1;
        }
        let progress = if self.ticks_on_step > STUCK_TICKS {
            Progress::Stuck
        } else {
            Progress::Moving
        };
        (ctx.controls, progress)
    }
}

/// Whether the player has reached `step` and should go for the next one.
fn is_reached(step: &Step, start: BlockPos, player: &PlayerState) -> bool {
    let target = step.target;
    let node = player.node();
    match step.kind {
        MoveKind::Ascend => {
            let block = BlockPos::containing(player.pos);
            block == target || block == target.down(1)
        }
        MoveKind::Descend => {
            let dest_ahead = BlockPos::new(
                start.x + (target.x - start.x) * 2,
                target.y,
                start.z + (target.z - start.z) * 2,
            );
            if node == target || node == dest_ahead {
                player.pos.y - f64::from(target.y) < 0.5
            } else {
                node.up(1) == target && player.on_ground
            }
        }
        MoveKind::Forward | MoveKind::Diagonal | MoveKind::Downward => {
            // going over the target while swimming is fine
            node == target || (player.in_water && node.down(1) == target)
        }
    }
}

struct Ctx<'a> {
    world: &'a dyn BlockView,
    player: &'a PlayerState,
    start: BlockPos,
    target: BlockPos,
    can_mine: bool,
    controls: Controls,
}

impl Ctx<'_> {
    fn execute(&mut self, kind: MoveKind) {
        match kind {
            MoveKind::Forward => self.forward(),
            MoveKind::Ascend => self.ascend(),
            MoveKind::Descend => self.descend(),
            MoveKind::Diagonal => self.diagonal(),
            MoveKind::Downward => self.downward(),
        }
    }

    /// Look straight ahead toward `pos`, as Azalea's `look_at` does.
    fn look_at(&mut self, pos: Vec3) {
        let eye = self.player.eye();
        let level = Vec3::new(pos.x, self.player.pos.y + 1.53, pos.z);
        self.controls.look = Some(yaw_pitch(eye, level));
    }

    fn look_at_exact(&mut self, pos: Vec3) {
        self.controls.look = Some(yaw_pitch(self.player.eye(), pos));
    }

    fn walk(&mut self) {
        self.controls.forward = true;
        self.controls.sprint = false;
    }

    fn sprint(&mut self) {
        self.controls.forward = true;
        self.controls.sprint = true;
    }

    fn stop(&mut self) {
        self.controls.forward = false;
        self.controls.sprint = false;
    }

    fn jump_if_in_water(&mut self) {
        if self.player.in_water {
            self.controls.jump = true;
        }
    }

    fn should_mine(&self, block: BlockPos) -> bool {
        self.world
            .cell(block)
            .is_some_and(|cell| !cell.passable && !cell.water)
    }

    /// Mine `block` once standing at the middle of the start node, walking
    /// there first. Whether the block is in the way.
    fn mine_while_at_start(&mut self, block: BlockPos) -> bool {
        if !self.should_mine(block) {
            return false;
        }
        let from_start = self.start.center().horizontal_distance(self.player.pos);
        if self.player.node() == self.start && from_start < 0.25 {
            if self.can_mine {
                self.stop();
                self.look_at_exact(block.center());
                self.controls.mine = Some(block);
            }
        } else {
            self.look_at(self.start.center());
            self.walk();
        }
        true
    }

    fn mine_first(&mut self, kind: MoveKind) -> bool {
        blocks_to_mine(kind, self.start, self.target)
            .into_iter()
            .any(|block| self.mine_while_at_start(block))
    }

    fn forward(&mut self) {
        self.jump_if_in_water();
        if self.mine_first(MoveKind::Forward) {
            return;
        }
        self.look_at(self.target.center());
        self.sprint();
    }

    fn ascend(&mut self) {
        let (start, target, player) = (self.start, self.target, *self.player);
        self.jump_if_in_water();
        if self.mine_first(MoveKind::Ascend) {
            return;
        }

        let center = target.center();
        self.look_at(center);
        self.walk();

        // don't jump while moving too fast the wrong way, or we'd miss
        let x_axis = (target.x - start.x).abs() as f64;
        let z_axis = (target.z - start.z).abs() as f64;
        let flat_distance_to_next =
            x_axis * (center.x - player.pos.x) + z_axis * (center.z - player.pos.z);
        let side_distance =
            z_axis * (center.x - player.pos.x).abs() + x_axis * (center.z - player.pos.z).abs();
        let lateral_motion = x_axis * player.velocity.z + z_axis * player.velocity.x;
        if lateral_motion.abs() > 0.1 || flat_distance_to_next > 1.2 || side_distance > 0.2 {
            return;
        }
        if player.node() == start && f64::from(target.y) - player.pos.y > 0.5 {
            self.controls.jump = true;
        }
    }

    fn descend(&mut self) {
        let (start, target, player) = (self.start, self.target, *self.player);
        if self.mine_first(MoveKind::Descend) {
            return;
        }

        let start_center = start.center();
        let center = target.center();
        let from_target = center.horizontal_distance(player.pos);
        let from_start = start_center.horizontal_distance(player.pos);
        let dest_ahead = Vec3::new(
            start_center.x + (center.x - start_center.x) * 1.5,
            center.y,
            start_center.z + (center.z - start_center.z) * 1.5,
        );
        let column = BlockPos::containing(player.pos);
        if column.x != target.x || column.z != target.z || from_target > 0.25 {
            // aiming past the edge avoids spinning around while falling
            self.look_at(if from_start < 1.25 { dest_ahead } else { center });
            self.walk();
        } else {
            self.stop();
        }
    }

    fn diagonal(&mut self) {
        self.jump_if_in_water();
        self.look_at(self.target.center());
        self.sprint();
    }

    fn downward(&mut self) {
        let target = self.target;
        let center = target.center();
        if center.horizontal_distance(self.player.pos) > 0.25 {
            self.look_at(center);
            self.walk();
        } else if self.mine_while_at_start(target) {
            self.stop();
            if self.controls.mine.is_none() {
                // not allowed to mine: stand still rather than walk off
                self.controls.look = None;
            }
        } else if BlockPos::containing(self.player.pos) != target {
            self.look_at(center);
            self.walk();
        } else {
            self.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_world::TestWorld;

    fn player_at(x: f64, y: f64, z: f64) -> PlayerState {
        PlayerState {
            pos: Vec3::new(x, y, z),
            on_ground: true,
            ..PlayerState::default()
        }
    }

    fn path(steps: &[(MoveKind, BlockPos)]) -> Path {
        Path {
            steps: steps
                .iter()
                .map(|&(kind, target)| Step { target, kind })
                .collect(),
            is_partial: false,
            cost: 0.,
        }
    }

    #[test]
    fn sprints_toward_the_next_node() {
        let world = TestWorld::flat(5);
        let mut exec = Executor::new(
            path(&[(MoveKind::Forward, BlockPos::new(1, 64, 0))]),
            BlockPos::new(0, 64, 0),
        );
        let (controls, progress) = exec.tick(&world, &player_at(0.5, 64.0, 0.5), true);
        assert_eq!(progress, Progress::Moving);
        assert!(controls.forward && controls.sprint && !controls.jump);
        // +x is yaw -90
        assert_eq!(controls.look.unwrap().0.round(), -90.0);
        assert_eq!(controls.mine, None);
    }

    #[test]
    fn finishes_when_the_last_node_is_reached() {
        let world = TestWorld::flat(5);
        let mut exec = Executor::new(
            path(&[
                (MoveKind::Forward, BlockPos::new(1, 64, 0)),
                (MoveKind::Forward, BlockPos::new(2, 64, 0)),
            ]),
            BlockPos::new(0, 64, 0),
        );
        let (_, progress) = exec.tick(&world, &player_at(1.5, 64.0, 0.5), true);
        assert_eq!(progress, Progress::Moving);
        let (controls, progress) = exec.tick(&world, &player_at(2.5, 64.0, 0.5), true);
        assert_eq!(progress, Progress::Finished);
        assert_eq!(controls, Controls::default());
    }

    #[test]
    fn mines_the_head_block_first_from_the_middle_of_the_start() {
        let mut world = TestWorld::flat(5);
        world.stone(BlockPos::new(1, 64, 0));
        world.stone(BlockPos::new(1, 65, 0));
        let mut exec = Executor::new(
            path(&[(MoveKind::Forward, BlockPos::new(1, 64, 0))]),
            BlockPos::new(0, 64, 0),
        );
        // off center: walk back to the middle first
        let (controls, _) = exec.tick(&world, &player_at(0.9, 64.0, 0.5), true);
        assert!(controls.forward);
        assert_eq!(controls.mine, None);
        let (controls, _) = exec.tick(&world, &player_at(0.5, 64.0, 0.5), true);
        assert!(!controls.forward);
        assert_eq!(controls.mine, Some(BlockPos::new(1, 65, 0)));
        world.air(BlockPos::new(1, 65, 0));
        let (controls, _) = exec.tick(&world, &player_at(0.5, 64.0, 0.5), true);
        assert_eq!(controls.mine, Some(BlockPos::new(1, 64, 0)));
        // looking down a bit at the feet block
        assert!(controls.look.unwrap().1 > 0.0);
    }

    #[test]
    fn jumps_up_a_step_once_close() {
        let mut world = TestWorld::flat(5);
        world.stone(BlockPos::new(1, 64, 0));
        let mut exec = Executor::new(
            path(&[(MoveKind::Ascend, BlockPos::new(1, 65, 0))]),
            BlockPos::new(0, 64, 0),
        );
        let (controls, _) = exec.tick(&world, &player_at(0.5, 64.0, 0.5), true);
        assert!(controls.forward && controls.jump);
        // drifting sideways: don't jump yet
        let (controls, _) = exec.tick(&world, &player_at(0.5, 64.0, 0.9), true);
        assert!(!controls.jump);
        // up there
        let (_, progress) = exec.tick(&world, &player_at(1.5, 65.0, 0.5), true);
        assert_eq!(progress, Progress::Finished);
    }

    #[test]
    fn descend_is_reached_only_after_landing() {
        let start = BlockPos::new(0, 64, 0);
        let step = Step {
            target: BlockPos::new(1, 62, 0),
            kind: MoveKind::Descend,
        };
        let mut falling = player_at(1.5, 63.4, 0.5);
        falling.on_ground = false;
        assert!(!is_reached(&step, start, &falling));
        assert!(is_reached(&step, start, &player_at(1.5, 62.0, 0.5)));
        // overshot by one block
        assert!(is_reached(&step, start, &player_at(2.5, 62.0, 0.5)));
    }

    #[test]
    fn swims_up_in_water() {
        let world = TestWorld::flat(5);
        let mut exec = Executor::new(
            path(&[(MoveKind::Forward, BlockPos::new(1, 64, 0))]),
            BlockPos::new(0, 64, 0),
        );
        let mut player = player_at(0.5, 64.0, 0.5);
        player.in_water = true;
        let (controls, _) = exec.tick(&world, &player, true);
        assert!(controls.jump);
    }

    #[test]
    fn stuck_after_too_long_on_one_step() {
        let world = TestWorld::flat(5);
        let mut exec = Executor::new(
            path(&[(MoveKind::Forward, BlockPos::new(1, 64, 0))]),
            BlockPos::new(0, 64, 0),
        );
        let player = player_at(0.5, 64.0, 0.5);
        for _ in 0..STUCK_TICKS {
            assert_eq!(exec.tick(&world, &player, true).1, Progress::Moving);
        }
        assert_eq!(exec.tick(&world, &player, true).1, Progress::Stuck);
    }

    #[test]
    fn mining_does_not_count_as_stuck() {
        let mut world = TestWorld::flat(5);
        world.stone(BlockPos::new(1, 65, 0));
        let mut exec = Executor::new(
            path(&[(MoveKind::Forward, BlockPos::new(1, 64, 0))]),
            BlockPos::new(0, 64, 0),
        );
        let player = player_at(0.5, 64.0, 0.5);
        for _ in 0..STUCK_TICKS * 2 {
            assert_eq!(exec.tick(&world, &player, true).1, Progress::Moving);
        }
    }

    #[test]
    fn a_block_placed_in_the_way_invalidates_the_path() {
        let mut world = TestWorld::flat(5);
        let exec = Executor::new(
            path(&[
                (MoveKind::Forward, BlockPos::new(1, 64, 0)),
                (MoveKind::Forward, BlockPos::new(2, 64, 0)),
            ]),
            BlockPos::new(0, 64, 0),
        );
        assert!(exec.is_still_valid(&CachedWorld::new(&world, false), 5));
        world.stone(BlockPos::new(2, 64, 0));
        world.stone(BlockPos::new(2, 65, 0));
        assert!(!exec.is_still_valid(&CachedWorld::new(&world, false), 5));
        // only looking one step ahead, it's still fine
        assert!(exec.is_still_valid(&CachedWorld::new(&world, false), 1));
    }

    #[test]
    fn lists_the_blocks_to_break() {
        let mut world = TestWorld::flat(5);
        world.stone(BlockPos::new(1, 65, 0));
        let exec = Executor::new(
            path(&[
                (MoveKind::Forward, BlockPos::new(1, 64, 0)),
                (MoveKind::Downward, BlockPos::new(1, 63, 0)),
            ]),
            BlockPos::new(0, 64, 0),
        );
        assert_eq!(
            exec.blocks_to_break(&world),
            vec![BlockPos::new(1, 65, 0), BlockPos::new(1, 63, 0)]
        );
        assert_eq!(exec.end(), BlockPos::new(1, 63, 0));
    }

    #[test]
    fn mines_straight_down() {
        let world = TestWorld::flat(5);
        let mut exec = Executor::new(
            path(&[(MoveKind::Downward, BlockPos::new(0, 63, 0))]),
            BlockPos::new(0, 64, 0),
        );
        let (controls, _) = exec.tick(&world, &player_at(0.5, 64.0, 0.5), true);
        assert_eq!(controls.mine, Some(BlockPos::new(0, 63, 0)));
        assert_eq!(controls.look.unwrap().1.round(), 90.0);
    }
}
