//! The orders `/basso` gives (go somewhere, mine, follow) and what each tick
//! does about them, after `basso::commands::process` and Baritone's
//! processes.

use std::collections::HashSet;

use crate::{
    astar::{Budget, a_star},
    commands::{self, Command, FollowTarget},
    executor::{Controls, Executor, PlayerState, Progress},
    goals::Goal,
    pos::{BlockPos, Vec3, yaw_pitch},
    world::{BlockView, CachedWorld},
};

/// How far around the player block searches look, in blocks.
pub const SEARCH_RADIUS: i32 = 48;
/// How far the player can reach to break a block.
const REACH: f64 = 4.4;
/// Baritone's `followRadius`.
const FOLLOW_RADIUS: f32 = 3.0;
/// Plan again for a followed entity once it moved this far from the goal.
const FOLLOW_REPLAN_DISTANCE_SQUARED: f64 = 4.0;
/// How many failed searches before giving a target up.
const MAX_FAILURES: u32 = 3;
/// Ticks between path validity checks.
const VALIDATE_INTERVAL: u64 = 10;
/// Ticks to wait before searching again after a search failed.
const RETRY_TICKS: u64 = 40;

/// An entity the game sees, for following.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityInfo {
    pub id: i32,
    /// Its type, such as `minecraft:zombie`.
    pub kind: String,
    /// The player name, for players.
    pub name: Option<String>,
    pub is_player: bool,
    pub living: bool,
    pub pos: Vec3,
}

#[derive(Clone, Debug)]
enum Order {
    Goto(Goal),
    GotoBlock {
        blocks: Vec<String>,
        target: Option<BlockPos>,
        blacklist: HashSet<BlockPos>,
    },
    Mine {
        blocks: Vec<String>,
        count: Option<u32>,
        mined: u32,
        target: Option<BlockPos>,
        blacklist: HashSet<BlockPos>,
    },
    Follow {
        target: FollowTarget,
        entity: Option<i32>,
        goal_pos: Option<Vec3>,
        said_nothing_found: bool,
    },
}

/// What [`Bot::drive`] made of a tick.
enum Drive {
    Controls(Controls),
    /// No path after several tries.
    Failed,
}

/// basso's state in one game.
#[derive(Debug, Default)]
pub struct Bot {
    order: Option<Order>,
    paused: bool,
    exec: Option<Executor>,
    /// The goal `exec` was planned for.
    goal: Option<Goal>,
    allow_mining: bool,
    failures: u32,
    retry_at: u64,
    ticks: u64,
    messages: Vec<String>,
    pub budget: Budget,
}

impl Bot {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self) -> bool {
        self.order.is_some() && !self.paused
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Chat lines to show since the last call.
    pub fn take_messages(&mut self) -> Vec<String> {
        std::mem::take(&mut self.messages)
    }

    /// The goal being walked to, for drawing.
    pub fn goal(&self) -> Option<&Goal> {
        self.goal.as_ref().filter(|_| self.is_active())
    }

    /// The nodes of the path being walked, from the last node reached.
    pub fn path_nodes(&self) -> Vec<BlockPos> {
        match &self.exec {
            Some(exec) if self.is_active() => std::iter::once(exec.start())
                .chain(exec.steps().map(|s| s.target))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The blocks the path will break, for drawing.
    pub fn blocks_to_break(&self, world: &dyn BlockView) -> Vec<BlockPos> {
        match &self.exec {
            Some(exec) if self.is_active() => exec.blocks_to_break(world),
            _ => Vec::new(),
        }
    }

    /// Run a `/basso` command line. Returns the lines to show.
    pub fn command(&mut self, line: &str, world: &dyn BlockView, player: &PlayerState) -> Vec<String> {
        let command = match commands::parse(line, player.node()) {
            Ok(command) => command,
            Err(err) => return vec![err],
        };
        match command {
            Command::Help(command) => commands::help(command.as_deref()),
            Command::Goto(goal) => {
                let line = format!("Going to {goal}.");
                self.start(Order::Goto(goal));
                vec![line]
            }
            Command::GotoBlock(block) => {
                if !world.knows_block(&block) {
                    return vec![format!("Unknown block `{block}`.")];
                }
                let line = format!("Going to the nearest {block}.");
                self.start(Order::GotoBlock {
                    blocks: vec![block],
                    target: None,
                    blacklist: HashSet::new(),
                });
                vec![line]
            }
            Command::Mine { count, blocks } => {
                if let Some(unknown) = blocks.iter().find(|b| !world.knows_block(b)) {
                    return vec![format!("Unknown block `{unknown}`.")];
                }
                let line = match count {
                    Some(count) => format!("Mining {count} {}.", blocks.join(", ")),
                    None => format!("Mining all {}.", blocks.join(", ")),
                };
                self.start(Order::Mine {
                    blocks,
                    count,
                    mined: 0,
                    target: None,
                    blacklist: HashSet::new(),
                });
                vec![line]
            }
            Command::Follow(target) => {
                let line = format!("Following {target}.");
                self.start(Order::Follow {
                    target,
                    entity: None,
                    goal_pos: None,
                    said_nothing_found: false,
                });
                vec![line]
            }
            Command::Pause => {
                if self.order.is_none() {
                    vec!["Nothing to pause.".to_owned()]
                } else if self.paused {
                    vec!["Already paused.".to_owned()]
                } else {
                    self.paused = true;
                    self.exec = None;
                    vec!["Paused.".to_owned()]
                }
            }
            Command::Resume => {
                if self.paused {
                    self.paused = false;
                    vec!["Resumed.".to_owned()]
                } else {
                    vec!["Not paused.".to_owned()]
                }
            }
            Command::Cancel => {
                if self.order.is_none() {
                    vec!["Nothing to cancel.".to_owned()]
                } else {
                    self.stop();
                    vec!["Cancelled.".to_owned()]
                }
            }
            Command::Paused => vec![if self.paused {
                "basso is paused.".to_owned()
            } else {
                "basso is not paused.".to_owned()
            }],
        }
    }

    fn start(&mut self, order: Order) {
        self.stop();
        self.order = Some(order);
    }

    fn stop(&mut self) {
        self.order = None;
        self.paused = false;
        self.exec = None;
        self.goal = None;
        self.failures = 0;
        self.retry_at = 0;
    }

    fn finish(&mut self, message: String) {
        self.messages.push(message);
        self.stop();
    }

    /// One game tick. `None` when basso has nothing to do and the player's
    /// keys should be let go.
    pub fn tick(
        &mut self,
        world: &dyn BlockView,
        player: &PlayerState,
        entities: &[EntityInfo],
    ) -> Option<Controls> {
        self.ticks += 1;
        if self.paused {
            return None;
        }
        match self.order.take()? {
            Order::Goto(goal) => self.tick_goto(goal, world, player),
            Order::GotoBlock {
                blocks,
                target,
                blacklist,
            } => self.tick_goto_block(blocks, target, blacklist, world, player),
            Order::Mine {
                blocks,
                count,
                mined,
                target,
                blacklist,
            } => self.tick_mine(blocks, count, mined, target, blacklist, world, player),
            Order::Follow {
                target,
                entity,
                goal_pos,
                said_nothing_found,
            } => self.tick_follow(target, entity, goal_pos, said_nothing_found, world, player, entities),
        }
    }

    fn tick_goto(&mut self, goal: Goal, world: &dyn BlockView, player: &PlayerState) -> Option<Controls> {
        if goal.success(player.node()) && (player.on_ground || player.in_water) {
            self.finish(format!("Arrived at {goal}."));
            return None;
        }
        self.order = Some(Order::Goto(goal.clone()));
        match self.drive(goal.clone(), true, world, player) {
            Drive::Controls(controls) => Some(controls),
            Drive::Failed => {
                self.finish(format!("Couldn't find a path to {goal}."));
                None
            }
        }
    }

    fn tick_goto_block(
        &mut self,
        blocks: Vec<String>,
        mut target: Option<BlockPos>,
        mut blacklist: HashSet<BlockPos>,
        world: &dyn BlockView,
        player: &PlayerState,
    ) -> Option<Controls> {
        if target.is_some_and(|t| world.is_block(t, &blocks) == Some(false)) {
            target = None;
        }
        let Some(t) = target.or_else(|| find_nearest(world, player.node(), &blocks, &blacklist))
        else {
            self.finish(format!("No {} nearby.", blocks.join(", ")));
            return None;
        };
        let goal = Goal::GetToBlock(t);
        if goal.success(player.node()) && (player.on_ground || player.in_water) {
            self.finish(format!("Arrived at the {} at {t}.", blocks.join(", ")));
            return None;
        }
        let controls = match self.drive(goal, true, world, player) {
            Drive::Controls(controls) => controls,
            Drive::Failed => {
                // try the next nearest one
                blacklist.insert(t);
                self.reset_search();
                Controls::default()
            }
        };
        self.order = Some(Order::GotoBlock {
            blocks,
            target: (!blacklist.contains(&t)).then_some(t),
            blacklist,
        });
        Some(controls)
    }

    #[allow(clippy::too_many_arguments)]
    fn tick_mine(
        &mut self,
        blocks: Vec<String>,
        count: Option<u32>,
        mut mined: u32,
        mut target: Option<BlockPos>,
        mut blacklist: HashSet<BlockPos>,
        world: &dyn BlockView,
        player: &PlayerState,
    ) -> Option<Controls> {
        let names = blocks.join(", ");
        if let Some(t) = target
            && world.is_block(t, &blocks) == Some(false)
        {
            mined += 1;
            target = None;
            self.reset_search();
            match count {
                Some(count) if mined >= count => {
                    self.finish(format!("Mined {mined} {names}. Done."));
                    return None;
                }
                Some(count) => self.messages.push(format!("Mined {mined}/{count} {names}.")),
                None => self.messages.push(format!("Mined {mined} {names}.")),
            }
        }

        let Some(t) = target.or_else(|| find_nearest(world, player.node(), &blocks, &blacklist))
        else {
            self.finish(format!("No more {names} nearby. Mined {mined}."));
            return None;
        };

        let mut controls = None;
        let breakable = CachedWorld::new(world, true)
            .cost_for_breaking_block(t)
            .is_finite();
        if !breakable {
            // next to a fluid or unbreakable: leave it
            blacklist.insert(t);
            target = None;
            self.reset_search();
        } else if can_reach(world, player, t) {
            self.exec = None;
            self.goal = None;
            controls = Some(Controls {
                look: Some(yaw_pitch(player.eye(), t.center())),
                mine: Some(t),
                ..Controls::default()
            });
            target = Some(t);
        } else {
            target = Some(t);
            match self.drive(Goal::TwoBlocks(t), true, world, player) {
                Drive::Controls(c) => controls = Some(c),
                Drive::Failed => {
                    blacklist.insert(t);
                    target = None;
                    self.reset_search();
                }
            }
        }
        self.order = Some(Order::Mine {
            blocks,
            count,
            mined,
            target,
            blacklist,
        });
        Some(controls.unwrap_or_default())
    }

    #[allow(clippy::too_many_arguments)]
    fn tick_follow(
        &mut self,
        target: FollowTarget,
        mut entity: Option<i32>,
        mut goal_pos: Option<Vec3>,
        mut said_nothing_found: bool,
        world: &dyn BlockView,
        player: &PlayerState,
        entities: &[EntityInfo],
    ) -> Option<Controls> {
        let current = entity
            .and_then(|id| entities.iter().find(|e| e.id == id))
            .filter(|e| follows(&target, e));
        let found = current.or_else(|| {
            entities
                .iter()
                .filter(|e| follows(&target, e))
                .min_by(|a, b| {
                    let da = a.pos.distance_squared(player.pos);
                    let db = b.pos.distance_squared(player.pos);
                    da.total_cmp(&db)
                })
        });

        let mut controls = Controls::default();
        match found {
            None => {
                if !said_nothing_found {
                    self.messages.push(format!("Nothing to follow ({target}) nearby."));
                    said_nothing_found = true;
                }
                entity = None;
                goal_pos = None;
                self.exec = None;
                self.goal = None;
            }
            Some(e) => {
                said_nothing_found = false;
                entity = Some(e.id);
                let moved = goal_pos.is_none_or(|p| p.distance_squared(e.pos) > FOLLOW_REPLAN_DISTANCE_SQUARED);
                if moved {
                    goal_pos = Some(e.pos);
                }
                let goal = Goal::Radius {
                    pos: goal_pos.unwrap_or(e.pos),
                    radius: FOLLOW_RADIUS,
                };
                if goal.success(player.node()) {
                    self.exec = None;
                    self.goal = None;
                } else if let Drive::Controls(c) = self.drive(goal, false, world, player) {
                    controls = c;
                } else {
                    // try again in a bit, it may move somewhere reachable
                    self.reset_search();
                    self.retry_at = self.ticks + RETRY_TICKS;
                }
            }
        }
        self.order = Some(Order::Follow {
            target,
            entity,
            goal_pos,
            said_nothing_found,
        });
        Some(controls)
    }

    fn reset_search(&mut self) {
        self.exec = None;
        self.goal = None;
        self.failures = 0;
    }

    /// Walk toward `goal`, searching when there's no path or the goal changed.
    fn drive(
        &mut self,
        goal: Goal,
        allow_mining: bool,
        world: &dyn BlockView,
        player: &PlayerState,
    ) -> Drive {
        if self.goal.as_ref() != Some(&goal) || self.allow_mining != allow_mining {
            self.exec = None;
            self.goal = Some(goal.clone());
            self.allow_mining = allow_mining;
            self.failures = 0;
        }

        if self.exec.is_none() {
            // plan from the ground, not mid-jump
            if !(player.on_ground || player.in_water) || self.ticks < self.retry_at {
                return Drive::Controls(Controls::default());
            }
            let cached = CachedWorld::new(world, allow_mining);
            let start = player.node();
            let path = a_star(&cached, start, &goal, self.budget);
            if path.steps.is_empty() {
                self.failures += 1;
                if self.failures >= MAX_FAILURES {
                    return Drive::Failed;
                }
                self.retry_at = self.ticks + RETRY_TICKS / 4;
                return Drive::Controls(Controls::default());
            }
            self.exec = Some(Executor::new(path, start));
        }

        let exec = self.exec.as_mut().expect("planned above");
        let (controls, progress) = exec.tick(world, player, allow_mining);
        match progress {
            Progress::Moving => {
                if self.ticks.is_multiple_of(VALIDATE_INTERVAL)
                    && !exec.is_still_valid(&CachedWorld::new(world, allow_mining), 5)
                {
                    self.exec = None;
                }
            }
            Progress::Finished => {
                // a partial path or arriving off the goal: plan the rest
                self.failures = 0;
                self.exec = None;
            }
            Progress::Stuck => {
                self.exec = None;
                self.failures += 1;
                if self.failures >= MAX_FAILURES {
                    return Drive::Failed;
                }
            }
        }
        Drive::Controls(controls)
    }
}

fn follows(target: &FollowTarget, e: &EntityInfo) -> bool {
    match target {
        FollowTarget::Entities => e.living,
        FollowTarget::Players => e.is_player,
        FollowTarget::EntityKinds(kinds) => kinds.iter().any(|k| crate::block_id_matches(k, &e.kind)),
        FollowTarget::PlayerNames(names) => e.is_player
            && e
                .name
                .as_deref()
                .is_some_and(|n| names.iter().any(|w| w.eq_ignore_ascii_case(n))),
    }
}

/// The nearest block that is one of `blocks` within [`SEARCH_RADIUS`],
/// searching outward in growing cubes.
pub fn find_nearest(
    world: &dyn BlockView,
    from: BlockPos,
    blocks: &[String],
    blacklist: &HashSet<BlockPos>,
) -> Option<BlockPos> {
    let mut best: Option<(i64, BlockPos)> = None;
    for r in 0..=SEARCH_RADIUS {
        // nothing further out can beat what was found
        if let Some((d, _)) = best
            && i64::from(r) * i64::from(r) > d
        {
            break;
        }
        for dx in -r..=r {
            for dy in -r..=r {
                let on_face = dx.abs() == r || dy.abs() == r;
                let mut dz = -r;
                while dz <= r {
                    let pos = from.offset(dx, dy, dz);
                    if pos.y >= world.min_y()
                        && !blacklist.contains(&pos)
                        && world.is_block(pos, blocks) == Some(true)
                    {
                        let d = pos.distance_squared(from);
                        if best.is_none_or(|(bd, _)| d < bd) {
                            best = Some((d, pos));
                        }
                    }
                    // inside the cube only the two z faces are new
                    dz += if on_face || dz == r { 1 } else { 2 * r };
                }
            }
        }
    }
    best.map(|(_, pos)| pos)
}

/// Whether the player can break `block` from where they stand: in reach and
/// nothing solid on the line from the eyes to its middle.
pub fn can_reach(world: &dyn BlockView, player: &PlayerState, block: BlockPos) -> bool {
    let eye = player.eye();
    let to = block.center();
    let distance = eye.distance_squared(to).sqrt();
    if distance > REACH {
        return false;
    }
    let steps = (distance / 0.05).ceil() as usize;
    for i in 0..steps {
        let t = i as f64 / steps as f64;
        let point = Vec3::new(
            eye.x + (to.x - eye.x) * t,
            eye.y + (to.y - eye.y) * t,
            eye.z + (to.z - eye.z) * t,
        );
        let at = BlockPos::containing(point);
        if at == block {
            return true;
        }
        match world.cell(at) {
            Some(cell) if cell.passable || cell.water => {}
            _ => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Cell, test_world::TestWorld};

    fn standing(x: f64, y: f64, z: f64) -> PlayerState {
        PlayerState {
            pos: Vec3::new(x, y, z),
            on_ground: true,
            ..PlayerState::default()
        }
    }

    fn names(blocks: &[&str]) -> Vec<String> {
        blocks.iter().map(|b| b.to_string()).collect()
    }

    #[test]
    fn goto_walks_and_arrives() {
        let world = TestWorld::flat(10);
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        assert_eq!(bot.command("basso goto 3 64 0", &world, &player), ["Going to 3 64 0."]);
        assert!(bot.is_active());
        let controls = bot.tick(&world, &player, &[]).unwrap();
        assert!(controls.forward);
        assert_eq!(bot.path_nodes().first(), Some(&BlockPos::new(0, 64, 0)));
        assert_eq!(bot.path_nodes().last(), Some(&BlockPos::new(3, 64, 0)));
        assert_eq!(bot.goal(), Some(&Goal::Block(BlockPos::new(3, 64, 0))));

        let there = standing(3.5, 64.0, 0.5);
        assert_eq!(bot.tick(&world, &there, &[]), None);
        assert!(!bot.is_active());
        assert_eq!(bot.take_messages(), ["Arrived at 3 64 0."]);
        assert!(bot.path_nodes().is_empty());
    }

    #[test]
    fn goto_gives_up_on_an_unreachable_goal() {
        let mut world = TestWorld::flat(10);
        // a stone box around the player, bedrock-like so it can't be mined
        for (x, z) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            for y in 64..=65 {
                world.set(BlockPos::new(x, y, z), Cell::solid(f32::INFINITY), "bedrock");
            }
        }
        world.set(BlockPos::new(0, 66, 0), Cell::solid(f32::INFINITY), "bedrock");
        world.set(BlockPos::new(0, 63, 0), Cell::solid(f32::INFINITY), "bedrock");
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        bot.command("basso goto 5 64 0", &world, &player);
        for _ in 0..200 {
            bot.tick(&world, &player, &[]);
        }
        assert!(!bot.is_active());
        assert!(bot.take_messages()[0].contains("Couldn't find a path"));
    }

    #[test]
    fn pause_resume_cancel() {
        let world = TestWorld::flat(10);
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        assert_eq!(bot.command("basso pause", &world, &player), ["Nothing to pause."]);
        bot.command("basso goto 5 64 0", &world, &player);
        assert_eq!(bot.command("basso pause", &world, &player), ["Paused."]);
        assert_eq!(bot.command("basso paused", &world, &player), ["basso is paused."]);
        assert_eq!(bot.tick(&world, &player, &[]), None);
        assert_eq!(bot.command("basso resume", &world, &player), ["Resumed."]);
        assert!(bot.tick(&world, &player, &[]).unwrap().forward);
        assert_eq!(bot.command("basso resume", &world, &player), ["Not paused."]);
        assert_eq!(bot.command("basso cancel", &world, &player), ["Cancelled."]);
        assert_eq!(bot.tick(&world, &player, &[]), None);
        assert_eq!(bot.command("basso cancel", &world, &player), ["Nothing to cancel."]);
    }

    #[test]
    fn bad_commands_say_why() {
        let world = TestWorld::flat(3);
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        assert!(bot.command("basso dance", &world, &player)[0].contains("Unknown"));
        assert!(!bot.is_active());
        assert!(bot.command("basso help", &world, &player).len() > 5);
    }

    #[test]
    fn goto_block_walks_next_to_it() {
        let mut world = TestWorld::flat(10);
        world.set(BlockPos::new(4, 64, 0), Cell::solid(37.5), "crafting_table");
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        bot.command("basso goto crafting_table", &world, &player);
        let controls = bot.tick(&world, &player, &[]).unwrap();
        assert!(controls.forward);
        assert_eq!(bot.goal(), Some(&Goal::GetToBlock(BlockPos::new(4, 64, 0))));
        assert_eq!(bot.tick(&world, &standing(3.5, 64.0, 0.5), &[]), None);
        assert!(bot.take_messages()[0].starts_with("Arrived at the crafting_table"));
    }

    #[test]
    fn goto_block_says_when_there_is_none() {
        let world = TestWorld::flat(5);
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        bot.command("basso goto crafting_table", &world, &player);
        assert_eq!(bot.tick(&world, &player, &[]), None);
        assert_eq!(bot.take_messages(), ["No crafting_table nearby."]);
    }

    #[test]
    fn mines_a_block_in_reach_and_counts_it() {
        let mut world = TestWorld::flat(10);
        world.set(BlockPos::new(2, 64, 0), Cell::solid(45.0), "coal_ore");
        world.set(BlockPos::new(-2, 64, 0), Cell::solid(45.0), "coal_ore");
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        assert_eq!(bot.command("basso mine 2 coal_ore", &world, &player), ["Mining 2 coal_ore."]);
        let controls = bot.tick(&world, &player, &[]).unwrap();
        let first = controls.mine.unwrap();
        assert!(first == BlockPos::new(2, 64, 0) || first == BlockPos::new(-2, 64, 0));
        assert!(!controls.forward);

        world.air(first);
        let second = bot.tick(&world, &player, &[]).unwrap().mine.unwrap();
        assert_ne!(first, second);
        assert_eq!(bot.take_messages(), ["Mined 1/2 coal_ore."]);
        world.air(second);
        assert_eq!(bot.tick(&world, &player, &[]), None);
        assert_eq!(bot.take_messages(), ["Mined 2 coal_ore. Done."]);
    }

    #[test]
    fn walks_to_a_block_out_of_reach_to_mine_it() {
        let mut world = TestWorld::flat(15);
        world.set(BlockPos::new(10, 64, 0), Cell::solid(45.0), "coal_ore");
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        bot.command("basso mine coal_ore", &world, &player);
        let controls = bot.tick(&world, &player, &[]).unwrap();
        assert!(controls.forward);
        assert_eq!(controls.mine, None);
        assert_eq!(bot.goal(), Some(&Goal::TwoBlocks(BlockPos::new(10, 64, 0))));
    }

    #[test]
    fn leaves_ore_next_to_water() {
        let mut world = TestWorld::flat(10);
        world.set(BlockPos::new(2, 64, 0), Cell::solid(45.0), "coal_ore");
        world.set(BlockPos::new(3, 64, 0), Cell::WATER, "water");
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        bot.command("basso mine coal_ore", &world, &player);
        bot.tick(&world, &player, &[]);
        assert_eq!(bot.tick(&world, &player, &[]), None);
        assert_eq!(bot.take_messages(), ["No more coal_ore nearby. Mined 0."]);
    }

    #[test]
    fn rejects_unknown_blocks_when_the_game_knows_better() {
        struct Picky(TestWorld);
        impl BlockView for Picky {
            fn cell(&self, pos: BlockPos) -> Option<Cell> {
                self.0.cell(pos)
            }
            fn is_block(&self, pos: BlockPos, blocks: &[String]) -> Option<bool> {
                self.0.is_block(pos, blocks)
            }
            fn min_y(&self) -> i32 {
                -64
            }
            fn knows_block(&self, id: &str) -> bool {
                crate::block_id_matches("stone", id)
            }
        }
        let world = Picky(TestWorld::flat(3));
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        assert_eq!(bot.command("basso mine nope", &world, &player), ["Unknown block `nope`."]);
        assert_eq!(bot.command("basso goto nope", &world, &player), ["Unknown block `nope`."]);
        assert!(!bot.is_active());
    }

    fn zombie(id: i32, x: f64) -> EntityInfo {
        EntityInfo {
            id,
            kind: "minecraft:zombie".to_owned(),
            name: None,
            is_player: false,
            living: true,
            pos: Vec3::new(x, 64.0, 0.5),
        }
    }

    #[test]
    fn follows_the_nearest_match_and_stops_close() {
        let world = TestWorld::flat(20);
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        bot.command("basso follow entity zombie", &world, &player);
        let entities = [zombie(1, 15.5), zombie(2, 8.5)];
        let controls = bot.tick(&world, &player, &entities).unwrap();
        assert!(controls.forward);
        match bot.goal() {
            Some(Goal::Radius { pos, .. }) => assert_eq!(pos.x, 8.5),
            other => panic!("{other:?}"),
        }
        // close enough: stand still, but keep following
        let close = standing(6.5, 64.0, 0.5);
        let controls = bot.tick(&world, &close, &entities).unwrap();
        assert!(!controls.forward);
        assert!(bot.is_active());
    }

    #[test]
    fn follow_says_once_when_nothing_matches() {
        let world = TestWorld::flat(5);
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        bot.command("basso follow players", &world, &player);
        bot.tick(&world, &player, &[zombie(1, 3.5)]);
        bot.tick(&world, &player, &[zombie(1, 3.5)]);
        assert_eq!(bot.take_messages(), ["Nothing to follow (players) nearby."]);
        assert!(bot.is_active());
    }

    #[test]
    fn follows_players_by_name() {
        let alice = EntityInfo {
            id: 7,
            kind: "minecraft:player".to_owned(),
            name: Some("Alice".to_owned()),
            is_player: true,
            living: true,
            pos: Vec3::default(),
        };
        assert!(follows(&FollowTarget::PlayerNames(names(&["alice"])), &alice));
        assert!(!follows(&FollowTarget::PlayerNames(names(&["bob"])), &alice));
        assert!(follows(&FollowTarget::Players, &alice));
        assert!(!follows(&FollowTarget::Players, &zombie(1, 0.0)));
        assert!(follows(&FollowTarget::EntityKinds(names(&["zombie"])), &zombie(1, 0.0)));
    }

    #[test]
    fn finds_the_nearest_block() {
        let mut world = TestWorld::flat(20);
        world.set(BlockPos::new(10, 64, 0), Cell::solid(1.0), "gold_block");
        world.set(BlockPos::new(-4, 63, 3), Cell::solid(1.0), "gold_block");
        let blocks = names(&["gold_block"]);
        let from = BlockPos::new(0, 64, 0);
        assert_eq!(
            find_nearest(&world, from, &blocks, &HashSet::new()),
            Some(BlockPos::new(-4, 63, 3))
        );
        let blacklist = HashSet::from([BlockPos::new(-4, 63, 3)]);
        assert_eq!(
            find_nearest(&world, from, &blocks, &blacklist),
            Some(BlockPos::new(10, 64, 0))
        );
        assert_eq!(find_nearest(&world, from, &names(&["diamond_ore"]), &HashSet::new()), None);
    }

    #[test]
    fn find_nearest_checks_every_block_of_the_cube() {
        // one block in each position of a small cube is found from the middle
        for x in -2..=2 {
            for y in 62..=66 {
                for z in -2..=2 {
                    let mut world = TestWorld::flat(5);
                    world.set(BlockPos::new(x, y, z), Cell::solid(1.0), "gold_block");
                    assert_eq!(
                        find_nearest(&world, BlockPos::new(0, 64, 0), &names(&["gold_block"]), &HashSet::new()),
                        Some(BlockPos::new(x, y, z))
                    );
                }
            }
        }
    }

    #[test]
    fn reach_needs_a_clear_line() {
        let mut world = TestWorld::flat(10);
        let player = standing(0.5, 64.0, 0.5);
        world.stone(BlockPos::new(3, 65, 0));
        assert!(can_reach(&world, &player, BlockPos::new(3, 65, 0)));
        world.stone(BlockPos::new(2, 65, 0));
        assert!(!can_reach(&world, &player, BlockPos::new(3, 65, 0)));
        assert!(!can_reach(&world, &player, BlockPos::new(8, 65, 0)));
        // the floor under the feet
        assert!(can_reach(&world, &player, BlockPos::new(0, 63, 0)));
    }
}
