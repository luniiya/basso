//! The orders `/basso` gives (go somewhere, mine, follow) and what each tick
//! does about them, after `basso::commands::process` and Baritone's
//! processes.

use std::collections::HashSet;

use crate::{
    astar::{AStar, Budget, Path},
    commands::{self, Command, FollowTarget},
    executor::{Controls, Executor, PlayerState, Progress},
    goals::Goal,
    pos::{BlockPos, Vec3, yaw_pitch},
    settings::Settings,
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
/// Items this close to a mined block are its drops.
const DROP_RADIUS: f64 = 3.0;
/// How far away basso goes for items.
const ITEM_RANGE: f64 = 24.0;
/// Mined blocks remembered for their drops.
const MINED_MEMORY: usize = 32;
/// Ticks to stand on an item that isn't picked up before leaving it.
const ITEM_PATIENCE: u32 = 40;
/// Nodes a search looks at each tick; the rest waits for the next tick, so
/// the search can be drawn while it runs.
const SEARCH_SLICE: usize = 5_000;
/// Baritone's `SURFACE_AT_AIR`: below this much air under water, swim up.
const SURFACE_AT_AIR: i32 = 100;
/// Vanilla's full air supply.
const MAX_AIR: i32 = 300;
/// Blocks between exploring waypoints: a bit less than a search's diameter,
/// so searches from each waypoint overlap.
pub const EXPLORE_STEP: i32 = 64;
/// Exploring waypoints before giving up (a spiral about 640 blocks across).
const EXPLORE_LEGS: u32 = 60;
/// Ticks between block searches while exploring.
const EXPLORE_SCAN_TICKS: u64 = 20;
/// Close enough to an exploring waypoint to head for the next.
const WAYPOINT_REACHED: f64 = 6.0;

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
    /// For a dropped item, its item id.
    pub item: Option<String>,
}

/// Looking for blocks: which, the one picked, the ones given up, and the
/// walk about when none is loaded.
#[derive(Clone, Debug, Default)]
struct Search {
    blocks: Vec<String>,
    target: Option<BlockPos>,
    blacklist: HashSet<BlockPos>,
    explore: Option<Explore>,
}

/// Baritone's `exploreForBlocks`: a square spiral of waypoints around where
/// the search ran dry, searching again on the way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Explore {
    origin: BlockPos,
    leg: u32,
    next_scan: u64,
}

#[derive(Clone, Debug, Default)]
struct Mine {
    search: Search,
    count: Option<u32>,
    mined: u32,
    /// Where blocks were mined, newest last, to find their drops.
    mined_at: Vec<BlockPos>,
    /// The item entity being picked up, and ticks spent on top of it.
    item: Option<(i32, u32)>,
    item_blacklist: HashSet<i32>,
    /// The count is reached; only drops are left to pick up.
    done: bool,
}

#[derive(Clone, Debug)]
enum Order {
    Goto(Goal),
    GotoBlock(Search),
    Mine(Box<Mine>),
    Follow {
        target: FollowTarget,
        entity: Option<i32>,
        goal_pos: Option<Vec3>,
        said_nothing_found: bool,
    },
}

/// What [`Bot::find`] made of a tick.
enum Found {
    Block(BlockPos),
    /// None loaded yet: walking about.
    Exploring(Controls),
    /// None to be found, after exploring or with exploring off.
    Nothing { explored: bool },
}

impl Search {
    /// Leave `block` for good and look for another.
    fn give_up(&mut self, block: BlockPos) {
        self.blacklist.insert(block);
        self.target = None;
    }
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
    /// The search for the next path, while it runs.
    pending: Option<AStar>,
    /// Told the player there is no path yet, for the current order.
    said_no_path: bool,
    /// The goal `exec` was planned for.
    goal: Option<Goal>,
    allow_mining: bool,
    failures: u32,
    retry_at: u64,
    /// Going up for air, until it is full again.
    surfacing: bool,
    /// The search for the path after the current partial one, run while
    /// walking it (Baritone's plan ahead), and its result.
    ahead: Option<AStar>,
    next: Option<(Path, BlockPos)>,
    ticks: u64,
    messages: Vec<String>,
    pub budget: Budget,
    pub settings: Settings,
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

    /// The search running now: Baritone's best path so far and path to the
    /// node considered last, for drawing.
    pub fn search_preview(&self) -> Option<(Vec<BlockPos>, Vec<BlockPos>)> {
        let search = self.pending.as_ref().filter(|_| self.is_active())?;
        Some((search.best_so_far(), search.most_recent()))
    }

    /// The block being mined or walked to, for drawing.
    pub fn target_block(&self) -> Option<BlockPos> {
        match &self.order {
            Some(Order::Mine(mine)) if self.is_active() => mine.search.target,
            Some(Order::GotoBlock(search)) if self.is_active() => search.target,
            _ => None,
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
            Command::Hint => vec![commands::HINT.to_owned()],
            Command::Help(command) => commands::help(command.as_deref()),
            Command::Set { name, value } => self.settings.set(name.as_deref(), value.as_deref()),
            Command::Toggle(name) => self.settings.toggle(&name),
            Command::Goto(goal) => {
                let line = format!("Going to {goal}.");
                self.start(Order::Goto(goal));
                vec![line]
            }
            Command::GotoBlock(block) => {
                let blocks = match resolve_blocks(world, &[block]) {
                    Ok(blocks) => blocks,
                    Err(err) => return vec![err],
                };
                let line = format!("Going to the nearest {}.", describe(&blocks));
                self.start(Order::GotoBlock(Search {
                    blocks,
                    ..Search::default()
                }));
                vec![line]
            }
            Command::Mine { count, blocks } => {
                let blocks = match resolve_blocks(world, &blocks) {
                    Ok(blocks) => blocks,
                    Err(err) => return vec![err],
                };
                let line = match count {
                    Some(count) => format!("Mining {count} {}.", describe(&blocks)),
                    None => format!("Mining all {}.", describe(&blocks)),
                };
                self.start(Order::Mine(Box::new(Mine {
                    search: Search {
                        blocks,
                        ..Search::default()
                    },
                    count,
                    ..Mine::default()
                })));
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
                    self.pending = None;
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
        self.pending = None;
        self.ahead = None;
        self.next = None;
        self.surfacing = false;
        self.said_no_path = false;
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
        let controls = match self.order.take()? {
            Order::Goto(goal) => self.tick_goto(goal, world, player),
            Order::GotoBlock(search) => self.tick_goto_block(search, world, player),
            Order::Mine(mine) => self.tick_mine(mine, world, player, entities),
            Order::Follow {
                target,
                entity,
                goal_pos,
                said_nothing_found,
            } => self.tick_follow(target, entity, goal_pos, said_nothing_found, world, player, entities),
        };
        controls.map(|c| self.breathe(player, c))
    }

    /// Never drown: in water with nothing to walk to, float up (Baritone
    /// jumps in water); low on air, swim up until it is full again.
    fn breathe(&mut self, player: &PlayerState, mut controls: Controls) -> Controls {
        if player.eye_in_water && player.air < SURFACE_AT_AIR {
            self.surfacing = true;
        }
        if !player.in_water || player.air >= MAX_AIR {
            self.surfacing = false;
        }
        let idle = !controls.forward && controls.mine.is_none();
        if self.surfacing || (player.in_water && idle) {
            controls.jump = true;
        }
        controls
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
                // as Baritone, keep trying: the world may open up
                if !self.said_no_path {
                    self.said_no_path = true;
                    self.messages
                        .push(format!("Couldn't find a path to {goal} yet. Still trying."));
                }
                self.reset_search();
                self.retry_at = self.ticks + RETRY_TICKS;
                Some(Controls::default())
            }
        }
    }

    fn tick_goto_block(&mut self, mut search: Search, world: &dyn BlockView, player: &PlayerState) -> Option<Controls> {
        let names = describe(&search.blocks);
        let t = match self.find(&mut search, world, player) {
            Found::Block(t) => t,
            Found::Exploring(controls) => {
                self.order = Some(Order::GotoBlock(search));
                return Some(controls);
            }
            Found::Nothing { explored: false } => {
                self.finish(format!("No {names} nearby."));
                return None;
            }
            Found::Nothing { explored: true } => {
                self.finish(format!("Explored without finding {names}."));
                return None;
            }
        };
        let goal = Goal::GetToBlock(t);
        if goal.success(player.node()) && (player.on_ground || player.in_water) {
            self.finish(format!("Arrived at the {names} at {t}."));
            return None;
        }
        let controls = match self.drive(goal, true, world, player) {
            Drive::Controls(controls) => controls,
            Drive::Failed => {
                // try the next nearest one
                search.give_up(t);
                self.reset_search();
                Controls::default()
            }
        };
        self.order = Some(Order::GotoBlock(search));
        Some(controls)
    }

    fn tick_mine(
        &mut self,
        mut mine: Box<Mine>,
        world: &dyn BlockView,
        player: &PlayerState,
        entities: &[EntityInfo],
    ) -> Option<Controls> {
        let names = describe(&mine.search.blocks);
        if let Some(t) = mine.search.target
            && world.is_block(t, &mine.search.blocks) == Some(false)
        {
            mine.mined += 1;
            mine.mined_at.push(t);
            if mine.mined_at.len() > MINED_MEMORY {
                mine.mined_at.remove(0);
            }
            mine.search.target = None;
            self.reset_search();
            if mine.count.is_some_and(|count| mine.mined >= count) {
                mine.done = true;
            }
        }

        // drops first, but don't walk off a block being broken
        let breaking = mine.search.target.is_some_and(|t| can_reach(world, player, t));
        if self.settings.mine_scan_dropped_items
            && !breaking
            && let Some(controls) = self.collect_drops(&mut mine, world, player, entities)
        {
            self.order = Some(Order::Mine(mine));
            return Some(controls);
        }
        if mine.done {
            self.finish(format!("Mined {} {names}.", mine.mined));
            return None;
        }

        let t = match self.find(&mut mine.search, world, player) {
            Found::Block(t) => t,
            Found::Exploring(controls) => {
                self.order = Some(Order::Mine(mine));
                return Some(controls);
            }
            Found::Nothing { explored: false } => {
                self.finish(format!("No more {names} nearby. Mined {}.", mine.mined));
                return None;
            }
            Found::Nothing { explored: true } => {
                self.finish(format!("Explored without finding more {names}. Mined {}.", mine.mined));
                return None;
            }
        };

        let mut controls = None;
        let breakable = CachedWorld::new(world, true)
            .cost_for_breaking_block(t)
            .is_finite();
        if !breakable {
            // next to a fluid or unbreakable: leave it
            mine.search.give_up(t);
            self.reset_search();
        } else if can_reach(world, player, t) {
            self.exec = None;
            self.goal = None;
            controls = Some(Controls {
                look: Some(yaw_pitch(player.eye(), t.center())),
                mine: Some(t),
                ..Controls::default()
            });
        } else {
            match self.drive(Goal::TwoBlocks(t), true, world, player) {
                Drive::Controls(c) => controls = Some(c),
                Drive::Failed => {
                    mine.search.give_up(t);
                    self.reset_search();
                }
            }
        }
        self.order = Some(Order::Mine(mine));
        Some(controls.unwrap_or_default())
    }

    /// Baritone's `mineScanDroppedItems`: walk onto the nearest item that is
    /// a wanted block or lies by a block basso mined. `None` when there is
    /// none to pick up.
    fn collect_drops(
        &mut self,
        mine: &mut Mine,
        world: &dyn BlockView,
        player: &PlayerState,
        entities: &[EntityInfo],
    ) -> Option<Controls> {
        let wanted = |e: &&EntityInfo| is_drop(mine, player, e);
        let current = mine
            .item
            .and_then(|(id, _)| entities.iter().find(|e| e.id == id))
            .filter(wanted);
        let item = current
            .or_else(|| {
                entities.iter().filter(wanted).min_by(|a, b| {
                    let da = a.pos.distance_squared(player.pos);
                    let db = b.pos.distance_squared(player.pos);
                    da.total_cmp(&db)
                })
            })
            .cloned();
        let Some(item) = item else {
            mine.item = None;
            return None;
        };
        let waited = match mine.item {
            Some((id, waited)) if id == item.id => waited,
            _ => 0,
        };
        let goal = Goal::Block(BlockPos::of_feet(item.pos));
        let on_it = player.pos.horizontal_distance(item.pos) < 0.8 && (player.pos.y - item.pos.y).abs() < 1.5;
        if on_it || goal.success(player.node()) {
            // standing on it: wait out its pickup delay, then leave it
            self.exec = None;
            self.goal = None;
            if waited >= ITEM_PATIENCE {
                mine.item_blacklist.insert(item.id);
                mine.item = None;
            } else {
                mine.item = Some((item.id, waited + 1));
            }
            return Some(Controls::default());
        }
        mine.item = Some((item.id, waited));
        match self.drive(goal, true, world, player) {
            Drive::Controls(controls) => Some(controls),
            Drive::Failed => {
                mine.item_blacklist.insert(item.id);
                mine.item = None;
                self.reset_search();
                Some(Controls::default())
            }
        }
    }

    /// The search's block, or a walk about while none is loaded.
    fn find(&mut self, search: &mut Search, world: &dyn BlockView, player: &PlayerState) -> Found {
        if search
            .target
            .is_some_and(|t| world.is_block(t, &search.blocks) == Some(false))
        {
            search.target = None;
        }
        if let Some(t) = search.target {
            return Found::Block(t);
        }
        // a full search is costly: while exploring, only now and then
        let scan = search.explore.is_none_or(|e| self.ticks >= e.next_scan);
        if scan && let Some(t) = find_nearest(world, player.node(), &search.blocks, &search.blacklist) {
            if search.explore.take().is_some() {
                self.reset_search();
            }
            search.target = Some(t);
            return Found::Block(t);
        }
        if !self.settings.explore_for_blocks {
            return Found::Nothing { explored: false };
        }
        let explore = search.explore.get_or_insert_with(|| {
            self.messages
                .push(format!("No {} nearby. Exploring.", describe(&search.blocks)));
            Explore {
                origin: player.node(),
                leg: 0,
                next_scan: 0,
            }
        });
        if scan {
            explore.next_scan = self.ticks + EXPLORE_SCAN_TICKS;
        }
        loop {
            if explore.leg >= EXPLORE_LEGS {
                return Found::Nothing { explored: true };
            }
            let (x, z) = explore_waypoint(explore.origin, explore.leg);
            let to = Vec3::new(f64::from(x) + 0.5, player.pos.y, f64::from(z) + 0.5);
            if player.pos.horizontal_distance(to) > WAYPOINT_REACHED {
                break;
            }
            explore.leg += 1;
        }
        let (x, z) = explore_waypoint(explore.origin, explore.leg);
        match self.drive(Goal::XZ { x, z }, true, world, player) {
            Drive::Controls(controls) => Found::Exploring(controls),
            Drive::Failed => {
                explore.leg += 1;
                self.reset_search();
                Found::Exploring(Controls::default())
            }
        }
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

    /// Search on from where the current partial path ends while it is
    /// walked, a slice a tick, so there is no stop between the two.
    fn plan_ahead(&mut self, end: BlockPos, goal: &Goal, allow_mining: bool, world: &dyn BlockView) {
        if self.next.is_some() {
            return;
        }
        let search = self
            .ahead
            .get_or_insert_with(|| AStar::new(end, goal.clone(), self.budget));
        let Some(path) = search.run(&CachedWorld::new(world, allow_mining), SEARCH_SLICE) else {
            return;
        };
        self.ahead = None;
        if !path.steps.is_empty() {
            self.next = Some((path, end));
        }
    }

    fn reset_search(&mut self) {
        self.exec = None;
        self.pending = None;
        self.ahead = None;
        self.next = None;
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
            self.pending = None;
            self.ahead = None;
            self.next = None;
            self.goal = Some(goal.clone());
            self.allow_mining = allow_mining;
            self.failures = 0;
        }

        if self.exec.is_none() {
            if self.ticks < self.retry_at {
                return Drive::Controls(Controls::default());
            }
            if self.pending.is_none() {
                // plan from the ground, not mid-jump
                if !(player.on_ground || player.in_water) {
                    return Drive::Controls(Controls::default());
                }
                self.pending = Some(AStar::new(player.node(), goal.clone(), self.budget));
            }
            let search = self.pending.as_mut().expect("started above");
            let cached = CachedWorld::new(world, allow_mining);
            let Some(path) = search.run(&cached, SEARCH_SLICE) else {
                // not done: stand still and go on next tick
                return Drive::Controls(Controls::default());
            };
            let start = search.start();
            self.pending = None;
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
        let (mut controls, progress) = exec.tick(world, player, allow_mining);
        match progress {
            Progress::Moving => {
                if self.ticks.is_multiple_of(VALIDATE_INTERVAL)
                    && !exec.is_still_valid(&CachedWorld::new(world, allow_mining), 5)
                {
                    self.exec = None;
                    self.ahead = None;
                    self.next = None;
                } else if exec.is_partial {
                    let end = exec.end();
                    self.plan_ahead(end, &goal, allow_mining, world);
                }
            }
            Progress::Finished => {
                // a partial path or arriving off the goal: plan the rest,
                // or walk on at once when it is planned already
                self.failures = 0;
                self.exec = None;
                self.ahead = None;
                if let Some((path, start)) = self.next.take()
                    && start == player.node()
                {
                    let mut exec = Executor::new(path, start);
                    controls = exec.tick(world, player, allow_mining).0;
                    self.exec = Some(exec);
                }
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

/// The blocks a command names, generic words made patterns (`log` is
/// `*_log`), or why one names nothing.
fn resolve_blocks(world: &dyn BlockView, blocks: &[String]) -> Result<Vec<String>, String> {
    blocks
        .iter()
        .map(|b| crate::resolve_block_word(b, |id| world.knows_block(id)).ok_or_else(|| format!("Unknown block `{b}`.")))
        .collect()
}

/// Blocks for chat, patterns shown as the word typed (`*_log` is `log`).
fn describe(blocks: &[String]) -> String {
    blocks
        .iter()
        .map(|b| b.strip_prefix("*_").unwrap_or(b))
        .collect::<Vec<_>>()
        .join(", ")
}

/// An item worth picking up while mining: one of the mined blocks, or lying
/// by a block basso mined, and not given up on.
fn is_drop(mine: &Mine, player: &PlayerState, e: &EntityInfo) -> bool {
    let Some(item) = &e.item else {
        return false;
    };
    !mine.item_blacklist.contains(&e.id)
        && e.pos.distance_squared(player.pos) <= ITEM_RANGE * ITEM_RANGE
        && (mine.search.blocks.iter().any(|b| crate::block_id_matches(b, item))
            || mine
                .mined_at
                .iter()
                .any(|m| m.center().distance_squared(e.pos) <= DROP_RADIUS * DROP_RADIUS))
}

/// The `leg`th waypoint of a square spiral around `origin`, one
/// [`EXPLORE_STEP`] apart: east, south, west twice, north twice, ...
pub fn explore_waypoint(origin: BlockPos, leg: u32) -> (i32, i32) {
    const DIRECTIONS: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];
    let (mut x, mut z) = (0, 0);
    let (mut run, mut turn, mut left) = (1u32, 0usize, leg + 1);
    while left > 0 {
        for _ in 0..2 {
            let (dx, dz) = DIRECTIONS[turn % 4];
            let steps = run.min(left);
            x += dx * steps as i32;
            z += dz * steps as i32;
            left -= steps;
            turn += 1;
            if left == 0 {
                break;
            }
        }
        run += 1;
    }
    (origin.x + x * EXPLORE_STEP, origin.z + z * EXPLORE_STEP)
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
    fn idle_in_water_it_floats_up() {
        let world = TestWorld::flat(10);
        let mut bot = Bot::new();
        let swimmer = PlayerState {
            in_water: true,
            air: MAX_AIR,
            ..standing(0.5, 60.0, 0.5)
        };
        let idle = bot.breathe(&swimmer, Controls::default());
        assert!(idle.jump);
        // walking a path: the executor decides
        let walking = Controls {
            forward: true,
            ..Controls::default()
        };
        assert!(!bot.breathe(&swimmer, walking).jump);
        // on land, nothing changes
        assert!(!bot.breathe(&standing(0.5, 64.0, 0.5), Controls::default()).jump);
        let _ = world;
    }

    #[test]
    fn low_on_air_it_swims_up_until_full() {
        let mut bot = Bot::new();
        let walking = Controls {
            forward: true,
            ..Controls::default()
        };
        let under = |air| PlayerState {
            in_water: true,
            eye_in_water: true,
            air,
            ..standing(0.5, 60.0, 0.5)
        };
        assert!(!bot.breathe(&under(SURFACE_AT_AIR), walking).jump);
        assert!(bot.breathe(&under(SURFACE_AT_AIR - 1), walking).jump);
        // head out but air not full yet: keeps going up
        let head_out = PlayerState {
            eye_in_water: false,
            ..under(150)
        };
        assert!(bot.breathe(&head_out, walking).jump);
        let full = PlayerState {
            eye_in_water: false,
            ..under(MAX_AIR)
        };
        assert!(!bot.breathe(&full, walking).jump);
        assert!(!bot.surfacing);
    }

    #[test]
    fn a_goal_on_the_sea_floor_does_not_drown_it() {
        let mut world = TestWorld::flat(10);
        for y in 64..70 {
            for x in -3..=3 {
                for z in -3..=3 {
                    world.set(BlockPos::new(x, y, z), Cell::WATER, "water");
                }
            }
        }
        let mut bot = Bot::new();
        let player = PlayerState {
            in_water: true,
            eye_in_water: true,
            air: 20,
            ..standing(0.5, 64.0, 0.5)
        };
        // the goal is in the air above the water: unreachable
        bot.command("basso goto 0 80 0", &world, &player);
        for _ in 0..50 {
            let controls = bot.tick(&world, &player, &[]).expect("still going");
            assert!(controls.jump, "{controls:?}");
        }
    }

    #[test]
    fn the_next_path_is_planned_while_walking_a_partial_one() {
        let world = TestWorld::flat(150);
        let mut bot = Bot::new();
        bot.budget = Budget {
            min_nodes: 100,
            max_nodes: 300,
        };
        let player = standing(0.5, 64.0, 0.5);
        bot.command("basso goto 400 64 0", &world, &player);
        let mut ticks = 0;
        while bot.exec.is_none() {
            bot.tick(&world, &player, &[]);
            ticks += 1;
            assert!(ticks < 50);
        }
        assert!(bot.exec.as_ref().unwrap().is_partial);
        let end = bot.exec.as_ref().unwrap().end();
        for _ in 0..20 {
            bot.tick(&world, &player, &[]);
        }
        let (_, start) = bot.next.as_ref().expect("planned ahead");
        assert_eq!(*start, end);
        // walk the first path step by step: at its end it goes straight on
        let at = |p: BlockPos| standing(f64::from(p.x) + 0.5, f64::from(p.y), f64::from(p.z) + 0.5);
        let steps: Vec<BlockPos> = bot.exec.as_ref().unwrap().steps().map(|s| s.target).collect();
        for p in steps {
            let controls = bot.tick(&world, &at(p), &[]).unwrap();
            assert!(controls.forward, "stopped at {p:?}");
        }
        assert_eq!(bot.exec.as_ref().unwrap().start(), end);
        assert!(bot.next.is_none());
    }

    #[test]
    fn a_block_in_the_way_drops_the_plan_ahead() {
        let mut world = TestWorld::flat(150);
        let mut bot = Bot::new();
        bot.budget = Budget {
            min_nodes: 100,
            max_nodes: 300,
        };
        let player = standing(0.5, 64.0, 0.5);
        bot.command("basso goto 400 64 0", &world, &player);
        for _ in 0..30 {
            bot.tick(&world, &player, &[]);
        }
        assert!(bot.next.is_some());
        // a wall goes up right ahead, unbreakable
        let first = bot.exec.as_ref().unwrap().steps().next().unwrap().target;
        world.set(first, Cell::solid(f32::INFINITY), "bedrock");
        world.set(first.up(1), Cell::solid(f32::INFINITY), "bedrock");
        for _ in 0..VALIDATE_INTERVAL {
            bot.tick(&world, &player, &[]);
        }
        let exec_goes_through = bot
            .exec
            .as_ref()
            .is_some_and(|e| e.steps().take(5).any(|s| s.target == first));
        assert!(!exec_goes_through);
    }

    #[test]
    fn goto_keeps_trying_an_unreachable_goal() {
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
        // as Baritone, it says so once and keeps trying
        assert!(bot.is_active());
        assert_eq!(bot.take_messages(), ["Couldn't find a path to 5 64 0 yet. Still trying."]);
        // the way opens: it goes
        world.air(BlockPos::new(1, 64, 0));
        world.air(BlockPos::new(1, 65, 0));
        let mut moved = false;
        for _ in 0..100 {
            moved |= bot.tick(&world, &player, &[]).is_some_and(|c| c.forward);
        }
        assert!(moved);
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
        bot.settings.explore_for_blocks = false;
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
        // no chat line per block
        assert!(bot.take_messages().is_empty());
        world.air(second);
        assert_eq!(bot.tick(&world, &player, &[]), None);
        assert_eq!(bot.take_messages(), ["Mined 2 coal_ore."]);
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
        bot.settings.explore_for_blocks = false;
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

    /// A world that knows only some block ids, as the game does.
    struct Knows(TestWorld, &'static [&'static str]);

    impl BlockView for Knows {
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
            self.1.iter().any(|k| crate::block_id_matches(id, k))
        }
    }

    #[test]
    fn long_searches_span_ticks_and_can_be_drawn() {
        let world = TestWorld::flat(60);
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        // a goal past the world's edge: the search uses its whole budget
        bot.command("basso goto 200 64 0", &world, &player);
        let controls = bot.tick(&world, &player, &[]).unwrap();
        assert!(!controls.forward);
        let (best, recent) = bot.search_preview().expect("still searching");
        assert_eq!(best[0], BlockPos::new(0, 64, 0));
        assert!(best.len() > 1);
        assert_eq!(recent[0], BlockPos::new(0, 64, 0));
        let mut ticks = 1;
        while bot.search_preview().is_some() {
            bot.tick(&world, &player, &[]);
            ticks += 1;
            assert!(ticks < 50);
        }
        assert!(!bot.path_nodes().is_empty());
        bot.command("basso cancel", &world, &player);
        assert!(bot.search_preview().is_none());
    }

    #[test]
    fn bare_basso_only_hints() {
        let world = TestWorld::flat(3);
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        assert_eq!(bot.command("basso", &world, &player), [commands::HINT]);
        assert!(!bot.is_active());
    }

    #[test]
    fn settings_commands_change_the_bot() {
        let world = TestWorld::flat(3);
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        assert_eq!(bot.command("basso set autoTool off", &world, &player), ["autoTool is now off."]);
        assert!(!bot.settings.auto_tool);
        assert_eq!(bot.command("basso toggle autoTool", &world, &player), ["autoTool is now on."]);
        assert!(bot.settings.auto_tool);
        assert!(bot.command("basso set", &world, &player).len() > 2);
    }

    #[test]
    fn generic_words_mine_every_kind() {
        let mut inner = TestWorld::flat(10);
        inner.set(BlockPos::new(2, 64, 0), Cell::solid(60.0), "birch_log");
        let world = Knows(inner, &["stone", "oak_log", "birch_log"]);
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        assert_eq!(bot.command("basso mine log", &world, &player), ["Mining all log."]);
        let controls = bot.tick(&world, &player, &[]).unwrap();
        assert_eq!(controls.mine, Some(BlockPos::new(2, 64, 0)));
        assert_eq!(bot.target_block(), Some(BlockPos::new(2, 64, 0)));
        assert_eq!(bot.command("basso goto log", &world, &player), ["Going to the nearest log."]);
        assert_eq!(bot.command("basso mine nope", &world, &player), ["Unknown block `nope`."]);
    }

    fn item(id: i32, kind: &str, x: f64, z: f64) -> EntityInfo {
        EntityInfo {
            id,
            kind: "minecraft:item".to_owned(),
            name: None,
            is_player: false,
            living: false,
            pos: Vec3::new(x, 64.0, z),
            item: Some(format!("minecraft:{kind}")),
        }
    }

    #[test]
    fn picks_up_drops_before_finishing() {
        let mut world = TestWorld::flat(10);
        world.set(BlockPos::new(2, 64, 0), Cell::solid(45.0), "coal_ore");
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        bot.command("basso mine 1 coal_ore", &world, &player);
        assert!(bot.tick(&world, &player, &[]).unwrap().mine.is_some());
        world.air(BlockPos::new(2, 64, 0));
        // coal, not coal_ore, lies where the ore was: still its drop
        let drop = [item(9, "coal", 4.5, 0.5)];
        let controls = bot.tick(&world, &player, &drop).unwrap();
        assert!(controls.forward);
        assert_eq!(bot.goal(), Some(&Goal::Block(BlockPos::new(4, 64, 0))));
        assert!(bot.is_active());
        // standing on it: wait for the pickup
        let there = standing(4.5, 64.0, 0.5);
        assert!(!bot.tick(&world, &there, &drop).unwrap().forward);
        assert!(bot.is_active());
        // picked up: done
        assert_eq!(bot.tick(&world, &there, &[]), None);
        assert_eq!(bot.take_messages(), ["Mined 1 coal_ore."]);
    }

    #[test]
    fn ignores_unrelated_items_and_can_be_turned_off() {
        let mut world = TestWorld::flat(10);
        world.set(BlockPos::new(2, 64, 0), Cell::solid(45.0), "coal_ore");
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        bot.command("basso mine 1 coal_ore", &world, &player);
        bot.tick(&world, &player, &[]);
        world.air(BlockPos::new(2, 64, 0));
        // far from the mined block and not what is mined
        assert_eq!(bot.tick(&world, &player, &[item(3, "dirt", -8.5, 0.5)]), None);

        let mut world = TestWorld::flat(10);
        world.set(BlockPos::new(2, 64, 0), Cell::solid(45.0), "coal_ore");
        let mut bot = Bot::new();
        bot.settings.mine_scan_dropped_items = false;
        bot.command("basso mine 1 coal_ore", &world, &player);
        bot.tick(&world, &player, &[]);
        world.air(BlockPos::new(2, 64, 0));
        assert_eq!(bot.tick(&world, &player, &[item(9, "coal", 2.5, 0.5)]), None);
    }

    #[test]
    fn matching_items_anywhere_near_are_collected() {
        let mut world = TestWorld::flat(15);
        world.set(BlockPos::new(-2, 64, 0), Cell::solid(60.0), "oak_log");
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        bot.command("basso mine oak_log", &world, &player);
        let logs = [item(4, "oak_log", 10.5, 0.5)];
        bot.tick(&world, &player, &logs);
        assert_eq!(bot.goal(), Some(&Goal::Block(BlockPos::new(10, 64, 0))));
    }

    #[test]
    fn gives_up_on_an_item_it_cannot_pick_up() {
        let mut world = TestWorld::flat(10);
        world.set(BlockPos::new(2, 64, 0), Cell::solid(45.0), "coal_ore");
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        bot.command("basso mine 1 coal_ore", &world, &player);
        bot.tick(&world, &player, &[]);
        world.air(BlockPos::new(2, 64, 0));
        let stuck = [item(9, "coal", 0.5, 0.5)];
        for _ in 0..=ITEM_PATIENCE {
            assert!(bot.tick(&world, &player, &stuck).is_some());
        }
        assert_eq!(bot.tick(&world, &player, &stuck), None);
        assert_eq!(bot.take_messages(), ["Mined 1 coal_ore."]);
    }

    #[test]
    fn explores_when_nothing_is_loaded_nearby() {
        let mut world = TestWorld::flat(80);
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        bot.command("basso mine gold_block", &world, &player);
        let controls = bot.tick(&world, &player, &[]).unwrap();
        assert!(controls.forward);
        assert_eq!(bot.take_messages(), ["No gold_block nearby. Exploring."]);
        assert_eq!(bot.goal(), Some(&Goal::XZ { x: EXPLORE_STEP, z: 0 }));
        assert!(bot.is_active());
        // a block shows up: it goes for it at the next search
        world.set(BlockPos::new(5, 64, 0), Cell::solid(1.0), "gold_block");
        for _ in 0..EXPLORE_SCAN_TICKS {
            bot.tick(&world, &player, &[]);
        }
        assert_eq!(bot.target_block(), Some(BlockPos::new(5, 64, 0)));
        assert_eq!(bot.goal(), Some(&Goal::TwoBlocks(BlockPos::new(5, 64, 0))));
        assert!(bot.take_messages().is_empty());
    }

    #[test]
    fn exploring_moves_on_from_reached_waypoints() {
        let world = TestWorld::flat(80);
        let mut bot = Bot::new();
        bot.command("basso goto gold_block", &world, &standing(0.5, 64.0, 0.5));
        bot.tick(&world, &standing(0.5, 64.0, 0.5), &[]);
        // the first waypoint is reached: on to the second
        bot.tick(&world, &standing(64.5, 64.0, 0.5), &[]);
        assert_eq!(bot.goal(), Some(&Goal::XZ { x: 64, z: 64 }));
    }

    #[test]
    fn explore_spiral() {
        let o = BlockPos::new(0, 64, 0);
        let s = EXPLORE_STEP;
        let legs: Vec<_> = (0..9).map(|leg| explore_waypoint(o, leg)).collect();
        assert_eq!(
            legs,
            [
                (s, 0),
                (s, s),
                (0, s),
                (-s, s),
                (-s, 0),
                (-s, -s),
                (0, -s),
                (s, -s),
                (2 * s, -s),
            ]
        );
        assert_eq!(explore_waypoint(BlockPos::new(10, 0, -3), 0), (10 + s, -3));
        // every waypoint differs
        let many: HashSet<_> = (0..EXPLORE_LEGS).map(|leg| explore_waypoint(o, leg)).collect();
        assert_eq!(many.len(), EXPLORE_LEGS as usize);
    }

    #[test]
    fn describes_patterns_as_words() {
        assert_eq!(describe(&names(&["*_log", "stone"])), "log, stone");
    }

    fn zombie(id: i32, x: f64) -> EntityInfo {
        EntityInfo {
            id,
            kind: "minecraft:zombie".to_owned(),
            name: None,
            is_player: false,
            living: true,
            pos: Vec3::new(x, 64.0, 0.5),
            item: None,
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
            item: None,
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
