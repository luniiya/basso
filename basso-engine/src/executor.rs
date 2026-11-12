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
/// Eye height of a sneaking player.
const SNEAK_EYE_HEIGHT: f64 = 1.27;
/// How far past the middle of the start block a bridging player leans out
/// (the edge is at 0.5; sneaking holds them up to 0.8).
const BRIDGE_LEAN: f64 = 0.62;
/// How closely the player must look at the face to click it, in degrees.
const AIM_TOLERANCE: f32 = 1.5;
/// How far past the middle of the start block a parkour jump takes off, in
/// blocks (its edge is at 0.5).
/// Jumps of this many blocks or more need speed.
/// How far off the middle of a ladder's block the player may drift.
const CLIMB_CENTRE: f64 = 0.3;
const PARKOUR_LONG: i32 = 4;
/// Blocks a tick along the jump that already counts as running.
const PARKOUR_RUN_SPEED: f64 = 0.15;
/// The most blocks backed away for a run-up.
const PARKOUR_RUN_UP: i32 = 5;
const PARKOUR_JUMP_AT: f64 = 0.45;
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
    /// Where the player looks now, in degrees.
    pub yaw: f32,
    pub pitch: f32,
    /// Sneaking: the eyes sit lower.
    pub sneaking: bool,
    /// There are blocks in the hotbar to place.
    pub has_blocks: bool,
    /// Blocks fallen so far (vanilla's `fallDistance`).
    pub fall_distance: f64,
    /// A bucket of water and an empty bucket are in the hotbar.
    pub water_bucket: bool,
    pub empty_bucket: bool,
}

impl PlayerState {
    pub fn eye(&self) -> Vec3 {
        let height = if self.sneaking { SNEAK_EYE_HEIGHT } else { EYE_HEIGHT };
        Vec3::new(self.pos.x, self.pos.y + height, self.pos.z)
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
    pub sneak: bool,
    /// Click this block with a block in hand, once: the block is placed on
    /// the face looked at.
    pub place: Option<BlockPos>,
    /// Hold this bucket (its hotbar slot).
    pub bucket: Option<Bucket>,
    /// Click with what is in hand at what is looked at, once.
    pub use_item: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bucket {
    Water,
    Empty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Moving,
    /// Every step is done.
    Finished,
    /// No progress for a while; plan again.
    Stuck,
}

/// Whether the forward key is down, `tick` ticks into a sprint. The game
/// starts a sprint on a second press of forward within a few ticks, as the
/// sprint key does not reach it when the key is a toggle: let go, press (the
/// game notes it), let go, press again, then hold. Forward may have been held
/// before, which is why the first press is part of it.
pub fn sprint_forward(tick: u32) -> bool {
    tick >= 3 || tick % 2 == 1
}

/// A path being walked.
#[derive(Debug)]
pub struct Executor {
    steps: VecDeque<Step>,
    /// The last node reached.
    start: BlockPos,
    pub is_partial: bool,
    ticks_on_step: u32,
    run_up: RunUp,
}

/// Where a long jump is: a standstill has no speed for four blocks, so the
/// player first backs away from the edge to run up.
#[derive(Clone, Copy, Debug, PartialEq)]
enum RunUp {
    Unchecked,
    Backing,
    Ready,
}

impl Executor {
    pub fn new(path: Path, start: BlockPos) -> Self {
        Self {
            steps: path.steps.into(),
            start,
            is_partial: path.is_partial,
            ticks_on_step: 0,
            run_up: RunUp::Unchecked,
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
            if !is_reached(step, self.start, player, world) {
                break;
            }
            self.start = step.target;
            self.steps.pop_front();
            self.ticks_on_step = 0;
            self.run_up = RunUp::Unchecked;
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
            run_up: self.run_up,
            controls: Controls::default(),
        };
        ctx.execute(step.kind);
        self.run_up = ctx.run_up;

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
fn is_reached(step: &Step, start: BlockPos, player: &PlayerState, world: &dyn BlockView) -> bool {
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
        MoveKind::Parkour | MoveKind::Pillar => {
            node == target && (player.on_ground || player.in_water)
        }
        // the cell of the ladder is reached the moment the feet are in it
        MoveKind::Climb => node == target,
        // leaning over the gap is not arriving: the floor must be placed
        MoveKind::Bridge => {
            node == target
                && player.on_ground
                && world.cell(target.down(1)).is_some_and(|c| c.standable)
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
    run_up: RunUp,
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
            MoveKind::Parkour => self.parkour(),
            MoveKind::Bridge => self.bridge(),
            MoveKind::Pillar => self.pillar(),
            MoveKind::Climb => self.climb(),
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

    /// Whether the player looks where `at` is, near enough to click it.
    fn aimed_at(&self, at: Vec3) -> bool {
        let (yaw, pitch) = yaw_pitch(self.player.eye(), at);
        let mut dyaw = (self.player.yaw - yaw) % 360.0;
        if dyaw > 180.0 {
            dyaw -= 360.0;
        } else if dyaw < -180.0 {
            dyaw += 360.0;
        }
        dyaw.abs() <= AIM_TOLERANCE && (self.player.pitch - pitch).abs() <= AIM_TOLERANCE
    }

    /// Baritone's bridging: sneak out to the edge, lean over it and place
    /// the block on the side of the one being stood on, then walk onto it.
    fn bridge(&mut self) {
        let (start, target, player) = (self.start, self.target, *self.player);
        if self.world.cell(target.down(1)).is_some_and(|c| c.standable) {
            // there already: just walk
            self.look_at(target.center());
            self.walk();
            return;
        }
        let (dx, dz) = (
            f64::from((target.x - start.x).signum()),
            f64::from((target.z - start.z).signum()),
        );
        let centre = start.center();
        let along = (player.pos.x - centre.x) * dx + (player.pos.z - centre.z) * dz;
        let side = (player.pos.x - centre.x).abs() * dz.abs() + (player.pos.z - centre.z).abs() * dx.abs();
        self.controls.sneak = true;
        // (leaning out, the feet are already in the next column)
        if side > 0.25 {
            // line up on the middle of the block first
            self.look_at(centre);
            self.walk();
            return;
        }
        if along < BRIDGE_LEAN {
            self.look_at(target.center());
            self.walk();
            return;
        }
        let support = start.down(1);
        let face = Vec3::new(
            support.center().x + dx * 0.5,
            support.center().y,
            support.center().z + dz * 0.5,
        );
        self.look_at_exact(face);
        self.stop();
        if self.aimed_at(face) {
            self.controls.place = Some(support);
        }
    }

    /// Up a ladder holding jump, down it by letting go; either way staying
    /// in the middle of the block so the player doesn't slip off the rungs.
    fn climb(&mut self) {
        let (start, target, player) = (self.start, self.target, *self.player);
        let centre = target.center();
        let off = centre.horizontal_distance(player.pos);
        if off > CLIMB_CENTRE {
            self.look_at(centre);
            self.walk();
        } else {
            self.stop();
        }
        if target.y > start.y {
            self.controls.jump = true;
        }
    }

    /// Baritone's pillar: jump, and under the feet at the top place the
    /// block on the one below.
    fn pillar(&mut self) {
        let (start, player) = (self.start, *self.player);
        let centre = start.center();
        let placed = self.world.cell(start).is_some_and(|c| !c.passable);
        if player.on_ground && !placed && centre.horizontal_distance(player.pos) > 0.2 {
            self.look_at(centre);
            self.walk();
            return;
        }
        self.stop();
        self.controls.look = Some((player.yaw, 90.0));
        if placed {
            return;
        }
        if player.on_ground {
            self.controls.jump = true;
        } else if player.pos.y > f64::from(start.y) + 1.02 && player.pitch > 89.0 {
            self.controls.place = Some(start.down(1));
        }
    }

    /// A sprint jump: run at the target and leave the ground at the far edge
    /// of the start block, as Baritone's `MovementParkour` does.
    fn parkour(&mut self) {
        let (start, target, player) = (self.start, self.target, *self.player);
        self.look_at(target.center());
        self.sprint();
        let (dx, dz) = (
            f64::from((target.x - start.x).signum()),
            f64::from((target.z - start.z).signum()),
        );
        let centre = start.center();
        let along = (player.pos.x - centre.x) * dx + (player.pos.z - centre.z) * dz;
        let side = (player.pos.x - centre.x).abs() * dz.abs() + (player.pos.z - centre.z).abs() * dx.abs();
        if self.run_up == RunUp::Unchecked {
            let long = (target.x - start.x).abs().max((target.z - start.z).abs()) >= PARKOUR_LONG;
            let speed = player.velocity.x * dx + player.velocity.z * dz;
            let slow = speed < PARKOUR_RUN_SPEED && along < PARKOUR_JUMP_AT;
            self.run_up = if long && slow && player.on_ground && self.room_behind(dx, dz) >= 2 {
                RunUp::Backing
            } else {
                RunUp::Ready
            };
        }
        if self.run_up == RunUp::Backing {
            let room = self.room_behind(dx, dz);
            if along <= -(f64::from(room.min(PARKOUR_RUN_UP)) - 0.4) || !player.on_ground {
                self.run_up = RunUp::Ready;
            } else {
                // walk away from the edge, facing the way back
                let back = Vec3::new(centre.x - dx * 5.0, player.pos.y, centre.z - dz * 5.0);
                self.look_at(back);
                self.walk();
                return;
            }
        }
        // a sprinting player covers 0.2 blocks a tick, so the edge can be
        // passed between two ticks: any tick on the ground past it will do
        if player.on_ground && along >= PARKOUR_JUMP_AT && along < 1.0 && side < 0.3 {
            self.controls.jump = true;
        }
    }

    /// How many blocks behind the start, in line with a jump going `(dx, dz)`,
    /// can be stood on in a row.
    fn room_behind(&self, dx: f64, dz: f64) -> i32 {
        (1..=PARKOUR_RUN_UP)
            .take_while(|&k| {
                let at = self.start.offset(-(dx as i32) * k, 0, -(dz as i32) * k);
                self.world.cell(at).is_some_and(|c| c.passable)
                    && self.world.cell(at.down(1)).is_some_and(|c| c.standable)
            })
            .count() as i32
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
        assert!(!is_reached(&step, start, &falling, &TestWorld::flat(5)));
        assert!(is_reached(&step, start, &player_at(1.5, 62.0, 0.5), &TestWorld::flat(5)));
        // overshot by one block
        assert!(is_reached(&step, start, &player_at(2.5, 62.0, 0.5), &TestWorld::flat(5)));
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

    fn parkour_path() -> Executor {
        Executor::new(
            path(&[(MoveKind::Parkour, BlockPos::new(3, 64, 0))]),
            BlockPos::new(0, 64, 0),
        )
    }

    #[test]
    fn parkour_runs_up_and_jumps_at_the_edge() {
        let world = TestWorld::flat(6);
        let mut exec = parkour_path();
        // in the middle of the start block: sprint, don't jump yet
        let (c, _) = exec.tick(&world, &player_at(0.5, 64.0, 0.5), true);
        assert!(c.forward && c.sprint && !c.jump);
        assert_eq!(c.look.unwrap().0.round(), -90.0);
        // at the far edge: jump
        let (c, _) = exec.tick(&world, &player_at(0.96, 64.0, 0.5), true);
        assert!(c.jump && c.sprint);
    }

    #[test]
    fn parkour_jumps_even_when_a_tick_skips_the_edge() {
        let world = TestWorld::flat(6);
        // from 0.7 the next tick would be at 1.2: already off the block,
        // still on the ground this tick
        let mut exec = parkour_path();
        let (c, _) = exec.tick(&world, &player_at(1.0, 64.0, 0.5), true);
        assert!(c.jump);
        let (c, _) = exec.tick(&world, &player_at(1.25, 64.0, 0.5), true);
        assert!(c.jump);
    }

    #[test]
    fn parkour_does_not_jump_in_the_air_or_off_line() {
        let world = TestWorld::flat(6);
        let mut exec = parkour_path();
        let mut flying = player_at(0.96, 64.4, 0.5);
        flying.on_ground = false;
        assert!(!exec.tick(&world, &flying, true).0.jump);
        // far to the side of the jump line
        let (c, _) = exec.tick(&world, &player_at(0.96, 64.0, 0.95), true);
        assert!(!c.jump);
    }

    #[test]
    fn parkour_is_done_on_the_landing_not_in_the_air() {
        let world = TestWorld::flat(6);
        let mut exec = parkour_path();
        let mut flying = player_at(3.5, 64.2, 0.5);
        flying.on_ground = false;
        assert_eq!(exec.tick(&world, &flying, true).1, Progress::Moving);
        assert_eq!(exec.tick(&world, &player_at(3.5, 64.0, 0.5), true).1, Progress::Finished);
    }

    fn long_parkour_path() -> Executor {
        Executor::new(
            path(&[(MoveKind::Parkour, BlockPos::new(4, 64, 0))]),
            BlockPos::new(0, 64, 0),
        )
    }

    fn standing_at(x: f64) -> PlayerState {
        player_at(x, 64.0, 0.5)
    }

    #[test]
    fn a_long_jump_from_a_standstill_backs_up_first() {
        let world = TestWorld::flat(8);
        let mut exec = long_parkour_path();
        let (c, _) = exec.tick(&world, &standing_at(0.5), true);
        // facing away from the gap (yaw 90 is west, so +90 more: east is -90)
        assert!(c.forward && !c.sprint && !c.jump);
        assert_eq!(c.look.unwrap().0.round().abs(), 90.0);
        assert!(c.look.unwrap().0 > 0.0, "{:?}", c.look);
    }

    #[test]
    fn it_runs_at_the_gap_once_far_enough_back() {
        let world = TestWorld::flat(8);
        let mut exec = long_parkour_path();
        exec.tick(&world, &standing_at(0.5), true);
        let (c, _) = exec.tick(&world, &standing_at(-4.3), true);
        assert!(c.forward && c.sprint);
        assert!(c.look.unwrap().0 < 0.0, "{:?}", c.look);
        // and doesn't turn back on the way, with little speed yet
        let (c, _) = exec.tick(&world, &standing_at(-3.5), true);
        assert!(c.sprint, "{c:?}");
    }

    #[test]
    fn a_long_jump_at_speed_goes_straight_away() {
        let world = TestWorld::flat(8);
        let mut exec = long_parkour_path();
        let mut running = standing_at(0.5);
        running.velocity = Vec3::new(0.22, 0.0, 0.0);
        let (c, _) = exec.tick(&world, &running, true);
        assert!(c.sprint);
        assert!(c.look.unwrap().0 < 0.0, "{:?}", c.look);
    }

    #[test]
    fn short_jumps_never_back_up() {
        let world = TestWorld::flat(8);
        let mut exec = parkour_path();
        let (c, _) = exec.tick(&world, &standing_at(0.5), true);
        assert!(c.sprint);
    }

    #[test]
    fn without_room_behind_it_jumps_anyway() {
        let mut world = TestWorld::flat(8);
        world.air(BlockPos::new(-1, 63, 0));
        let mut exec = long_parkour_path();
        let (c, _) = exec.tick(&world, &standing_at(0.5), true);
        assert!(c.sprint);
        assert!(c.look.unwrap().0 < 0.0, "{:?}", c.look);
    }

    fn bridge_path() -> Executor {
        Executor::new(
            path(&[(MoveKind::Bridge, BlockPos::new(1, 64, 0))]),
            BlockPos::new(0, 64, 0),
        )
    }

    fn gap_world() -> TestWorld {
        let mut world = TestWorld::flat(6);
        world.air(BlockPos::new(1, 63, 0));
        world
    }

    #[test]
    fn bridge_sneaks_out_to_the_edge_first() {
        let world = gap_world();
        let mut exec = bridge_path();
        let (c, _) = exec.tick(&world, &player_at(0.5, 64.0, 0.5), true);
        assert!(c.sneak && c.forward && !c.sprint);
        assert_eq!(c.place, None);
        assert_eq!(c.look.unwrap().0.round(), -90.0);
    }

    #[test]
    fn bridge_places_on_the_side_of_the_floor_once_aimed() {
        let world = gap_world();
        let mut exec = bridge_path();
        let mut player = player_at(1.15, 64.0, 0.5);
        player.sneaking = true;
        // leaning out, not yet looking at the face
        let (c, _) = exec.tick(&world, &player, true);
        assert!(c.sneak && !c.forward);
        assert_eq!(c.place, None);
        let face = Vec3::new(1.0, 63.5, 0.5);
        let (yaw, pitch) = c.look.unwrap();
        assert_eq!((yaw, pitch), yaw_pitch(player.eye(), face));
        // aimed: click the floor block under the start
        player.yaw = yaw;
        player.pitch = pitch;
        let (c, _) = exec.tick(&world, &player, true);
        assert_eq!(c.place, Some(BlockPos::new(0, 63, 0)));
        assert!(c.sneak);
    }

    #[test]
    fn bridge_walks_on_when_the_block_is_there() {
        let world = TestWorld::flat(6);
        let mut exec = bridge_path();
        let (c, _) = exec.tick(&world, &player_at(0.9, 64.0, 0.5), true);
        assert!(c.forward && !c.sneak && c.place.is_none());
    }

    #[test]
    fn bridge_lines_up_before_leaning_out() {
        let world = gap_world();
        let mut exec = bridge_path();
        let (c, _) = exec.tick(&world, &player_at(0.5, 64.0, 0.95), true);
        assert!(c.sneak && c.forward && c.place.is_none());
        // heading back to the middle line: toward -z
        assert!(c.look.unwrap().0.abs() < 5.0 || c.look.unwrap().0.abs() > 175.0);
    }

    fn pillar_path() -> Executor {
        Executor::new(
            path(&[(MoveKind::Pillar, BlockPos::new(0, 65, 0))]),
            BlockPos::new(0, 64, 0),
        )
    }

    #[test]
    fn pillar_jumps_looking_down() {
        let world = TestWorld::flat(6);
        let mut exec = pillar_path();
        let (c, _) = exec.tick(&world, &player_at(0.5, 64.0, 0.5), true);
        assert!(c.jump && !c.forward);
        assert_eq!(c.look.unwrap().1, 90.0);
        assert_eq!(c.place, None);
    }

    #[test]
    fn pillar_places_at_the_top_of_the_jump() {
        let world = TestWorld::flat(6);
        let mut exec = pillar_path();
        let mut player = player_at(0.5, 64.5, 0.5);
        player.on_ground = false;
        player.pitch = 90.0;
        // still inside the block's height: no
        assert_eq!(exec.tick(&world, &player, true).0.place, None);
        player.pos.y = 65.1;
        let (c, _) = exec.tick(&world, &player, true);
        assert_eq!(c.place, Some(BlockPos::new(0, 63, 0)));
        // looking elsewhere yet: wait for the turn
        player.pitch = 0.0;
        assert_eq!(exec.tick(&world, &player, true).0.place, None);
    }

    #[test]
    fn pillar_centres_on_the_block_before_jumping_and_ends_on_it() {
        let world = TestWorld::flat(6);
        let mut exec = pillar_path();
        let (c, _) = exec.tick(&world, &player_at(0.85, 64.0, 0.5), true);
        assert!(!c.jump && c.forward);
        let mut placed = TestWorld::flat(6);
        placed.stone(BlockPos::new(0, 64, 0));
        let mut exec = pillar_path();
        let (c, p) = exec.tick(&placed, &player_at(0.5, 65.0, 0.5), true);
        assert_eq!(p, Progress::Finished);
        assert!(!c.jump);
    }

    #[test]
    fn a_sprint_starts_with_a_double_tap_of_forward() {
        let keys: Vec<bool> = (0..5).map(sprint_forward).collect();
        assert_eq!(keys, [false, true, false, true, true]);
    }

    fn climb_path(from: BlockPos, to: BlockPos) -> Executor {
        Executor::new(path(&[(MoveKind::Climb, to)]), from)
    }

    #[test]
    fn climbing_up_holds_jump_and_stays_centred() {
        let world = TestWorld::flat(6);
        let mut exec = climb_path(BlockPos::new(0, 64, 0), BlockPos::new(0, 65, 0));
        let (c, p) = exec.tick(&world, &player_at(0.5, 64.0, 0.5), true);
        assert!(c.jump && !c.forward);
        assert_eq!(p, Progress::Moving);
    }

    #[test]
    fn climbing_down_lets_go() {
        let world = TestWorld::flat(6);
        let mut exec = climb_path(BlockPos::new(0, 66, 0), BlockPos::new(0, 65, 0));
        let (c, _) = exec.tick(&world, &player_at(0.5, 66.0, 0.5), true);
        assert!(!c.jump && !c.forward);
    }

    #[test]
    fn a_climb_walks_back_to_the_middle_when_it_has_slipped_off() {
        let world = TestWorld::flat(6);
        let mut exec = climb_path(BlockPos::new(0, 64, 0), BlockPos::new(0, 65, 0));
        let (c, _) = exec.tick(&world, &player_at(0.95, 64.2, 0.5), true);
        assert!(c.forward && c.jump);
    }

    #[test]
    fn a_climb_is_done_when_the_feet_are_in_the_next_block() {
        let world = TestWorld::flat(6);
        let mut exec = climb_path(BlockPos::new(0, 64, 0), BlockPos::new(0, 65, 0));
        let mut player = player_at(0.5, 65.0, 0.5);
        player.on_ground = false;
        let (_, p) = exec.tick(&world, &player, true);
        assert_eq!(p, Progress::Finished);
    }
}
