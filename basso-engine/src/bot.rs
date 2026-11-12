//! The orders `/basso` gives (go somewhere, mine, follow) and what each tick
//! does about them, after `basso::commands::process` and Baritone's
//! processes.

use std::collections::{HashMap, HashSet};

use crate::{
    astar::{AStar, Budget, Path},
    commands::{self, Command, FollowTarget},
    executor::{Controls, Executor, PlayerState, Progress},
    goals::Goal,
    mlg::Mlg,
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
/// Nodes a search looks at each tick; the rest waits for the next tick, so
/// the search can be drawn while it runs.
const SEARCH_SLICE: usize = 5_000;
/// Baritone's `mineGoalUpdateInterval`: ticks between rescans.
const MINE_GOAL_UPDATE_INTERVAL: u64 = 5;
/// Baritone's `mineMaxOreLocationsCount`.
pub const MINE_MAX_ORE_LOCATIONS: usize = 64;
/// Baritone's `mineDropLoiterDurationMSThanksLouca` (250 ms), in ticks.
const MINE_DROP_LOITER_TICKS: u64 = 5;
/// How near a known block must be for its vanishing to count as basso's.
const MINED_NEAR: f64 = 6.0;
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
/// How far around the player to look for lower ground before exploring.
const DESCENT_RADIUS: i32 = 24;
/// How far below the feet counts as lower ground worth dropping to.
const DESCENT_MIN_DEPTH: i32 = 8;
/// How far down to look for the ground in a column.
const DESCENT_MAX_DEPTH: i32 = 96;

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
    /// The level to drop to first when the search ran dry up on an island or
    /// a cliff: the ground below is nearer than any waypoint.
    descend_to: Option<i32>,
}

#[derive(Clone, Debug, Default)]
struct Mine {
    /// `blocks` and `blacklist` (Baritone's inaccessible ones); `target` is
    /// the block mined from right below (the shaft), for drawing.
    search: Search,
    count: Option<u32>,
    mined: u32,
    /// Baritone's `knownOreLocations`: the nearest wanted blocks and drops,
    /// up to [`MINE_MAX_ORE_LOCATIONS`], nearest first.
    known: Vec<BlockPos>,
    /// Baritone's `anticipatedDrops`: where a block was just mined, with the
    /// tick until which its drop is waited for.
    anticipated_drops: HashMap<BlockPos, u64>,
    /// Baritone's `tickCount`, for the rescan interval.
    tick_count: u64,
    /// The count is reached; only its drops are left to pick up.
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
    /// `basso click`: the mouse buttons send the bot where the view points.
    click_mode: bool,
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
    /// The player has blocks to place (from the last tick).
    has_blocks: bool,
    failures: u32,
    /// When the search in `pending` began, for the verbose log.
    search_began: u64,
    retry_at: u64,
    /// Going up for air, until it is full again.
    surfacing: bool,
    mlg: Mlg,
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

    pub fn click_mode(&self) -> bool {
        self.click_mode
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
            Command::Click => {
                self.click_mode = !self.click_mode;
                vec![if self.click_mode {
                    "Click on: left click goes into the block you point at, right click onto it. `basso click` again to leave."
                } else {
                    "Click off."
                }
                .to_owned()]
            }
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

    /// Baritone's `#click`: go to the block clicked, as `goto` would.
    pub fn click_goto(&mut self, goal: Goal) -> String {
        let line = format!("Going to {goal}.");
        self.start(Order::Goto(goal));
        line
    }

    /// Say `line` when `verbose` is at least `level`.
    fn say(&mut self, level: u8, line: impl FnOnce() -> String) {
        if self.settings.verbose >= level {
            self.messages.push(format!("[v{level}] {}", line()));
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
        self.has_blocks = player.has_blocks;
        if self.paused {
            return None;
        }
        let order_going = self.order.is_some();
        if let Some(controls) = self.mlg.tick(world, player, self.settings.mlg_water_bucket && order_going) {
            return Some(controls);
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

    /// Baritone's `MineProcess.onTick`.
    fn tick_mine(
        &mut self,
        mut mine: Box<Mine>,
        world: &dyn BlockView,
        player: &PlayerState,
        entities: &[EntityInfo],
    ) -> Option<Controls> {
        let names = describe(&mine.search.blocks);
        let feet = player.node();
        mine.anticipated_drops.retain(|_, until| *until >= self.ticks);

        // known blocks by the player that are gone: basso broke them
        let blocks = mine.search.blocks.clone();
        let mined: Vec<BlockPos> = mine
            .known
            .iter()
            .copied()
            .filter(|&p| {
                p.center().distance_squared(player.eye()) <= MINED_NEAR * MINED_NEAR
                    && world.is_block(p, &blocks) == Some(false)
                    && !mine.anticipated_drops.contains_key(&p)
            })
            .collect();
        for p in mined {
            mine.mined += 1;
            mine.anticipated_drops.insert(p, self.ticks + MINE_DROP_LOITER_TICKS);
            if mine.count.is_some_and(|count| mine.mined >= count) {
                mine.done = true;
            }
        }
        mine.known.retain(|p| !mine.anticipated_drops.contains_key(p));

        let dropped = self.dropped_items_scan(&mine, entities);
        if mine.done && dropped.is_empty() {
            self.finish(format!("Mined {} {names}.", mine.mined));
            return None;
        }

        // rescan
        if mine.tick_count.is_multiple_of(MINE_GOAL_UPDATE_INTERVAL) {
            let mut locs = if mine.done {
                Vec::new()
            } else {
                find_known(world, feet, &blocks, &mine.search.blacklist, MINE_MAX_ORE_LOCATIONS)
            };
            if !mine.done {
                locs.extend(mine.known.iter().copied());
            }
            mine.known = locs;
        }
        mine.tick_count += 1;

        // the shaft: a wanted block straight above, mined from below
        mine.search.target = None;
        if player.on_ground && !mine.done {
            let shaft = mine
                .known
                .iter()
                .copied()
                .filter(|p| p.x == feet.x && p.z == feet.z && p.y >= feet.y)
                .filter(|&p| world.is_block(p, &blocks) == Some(true))
                .min_by_key(|p| p.distance_squared(feet.up(1)));
            if let Some(t) = shaft
                && plausible_to_break(world, t)
                && can_reach(world, player, t)
            {
                mine.search.target = Some(t);
                self.exec = None;
                self.goal = None;
                self.order = Some(Order::Mine(mine));
                return Some(Controls {
                    look: Some(yaw_pitch(player.eye(), t.center())),
                    mine: Some(t),
                    ..Controls::default()
                });
            }
        }

        // updateGoal: the drops lying about are goals too
        let mut wanted = mine.known.clone();
        wanted.extend(dropped.iter().copied().filter(|d| !mine.known.contains(d)));
        let goals = prune(world, player, &wanted, &blocks, &mine.search.blacklist, &dropped);
        // only blocks are kept between ticks; drops are scanned again
        mine.known = goals
            .iter()
            .copied()
            .filter(|&p| world.is_block(p, &blocks) != Some(false))
            .collect();
        if goals.is_empty() {
            if mine.done {
                self.finish(format!("Mined {} {names}.", mine.mined));
                return None;
            }
            return match self.find(&mut mine.search, world, player) {
                Found::Block(t) => {
                    mine.search.target = None;
                    if plausible_to_break(world, t) {
                        mine.known.push(t);
                    } else {
                        // next to a fluid or unbreakable: leave it
                        self.say(1, || format!("Skipping {names} at {t}: next to a fluid or unbreakable."));
                        mine.search.blacklist.insert(t);
                    }
                    self.order = Some(Order::Mine(mine));
                    Some(Controls::default())
                }
                Found::Exploring(controls) => {
                    self.order = Some(Order::Mine(mine));
                    Some(controls)
                }
                Found::Nothing { explored: false } => {
                    self.finish(format!("No more {names} nearby. Mined {}.", mine.mined));
                    None
                }
                Found::Nothing { explored: true } => {
                    self.finish(format!("Explored without finding more {names}. Mined {}.", mine.mined));
                    None
                }
            };
        }
        mine.search.explore = None;
        if mine.tick_count % 40 == 1 {
            let nearest = goals.iter().copied().min_by_key(|p| p.distance_squared(feet));
            self.say(2, || format!("Mining goals: {} blocks, nearest {nearest:?}, standing at {feet}.", goals.len()));
        }
        let goal = Goal::Composite(
            goals
                .iter()
                .map(|&loc| coalesce(world, loc, &goals, &blocks))
                .collect(),
        );
        if goal.success(feet) {
            // standing on a drop, waiting for it to be picked up
            self.exec = None;
            self.goal = Some(goal);
            self.order = Some(Order::Mine(mine));
            return Some(Controls::default());
        }
        let controls = match self.drive(goal, true, world, player) {
            Drive::Controls(controls) => controls,
            Drive::Failed => {
                // blacklistClosestOnFailure
                let closest = mine
                    .known
                    .iter()
                    .copied()
                    .min_by_key(|p| p.distance_squared(feet));
                if let Some(closest) = closest {
                    self.say(1, || format!("Giving up on {names} at {closest}: no path."));
                    mine.search.blacklist.insert(closest);
                    mine.known.retain(|&p| p != closest);
                }
                self.reset_search();
                Controls::default()
            }
        };
        self.order = Some(Order::Mine(mine));
        Some(controls)
    }

    /// Baritone's `droppedItemsScan`: where the wanted items lie, and the
    /// drops still expected from blocks just mined.
    fn dropped_items_scan(&self, mine: &Mine, entities: &[EntityInfo]) -> Vec<BlockPos> {
        if !self.settings.mine_scan_dropped_items {
            return Vec::new();
        }
        let mut dropped: Vec<BlockPos> = entities
            .iter()
            .filter(|e| {
                e.item
                    .as_ref()
                    .is_some_and(|item| mine.search.blocks.iter().any(|b| crate::block_id_matches(b, item)))
            })
            .map(|e| BlockPos::of_feet(e.pos))
            .collect();
        dropped.extend(mine.anticipated_drops.keys().copied());
        dropped
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
                descend_to: descent_level(world, player.node()),
            }
        });
        if scan {
            explore.next_scan = self.ticks + EXPLORE_SCAN_TICKS;
        }
        if let Some(level) = explore.descend_to {
            if player.node().y <= level + 1 {
                explore.descend_to = None;
            } else {
                match self.drive(Goal::Y(level), true, world, player) {
                    Drive::Controls(controls) => return Found::Exploring(controls),
                    Drive::Failed => {
                        explore.descend_to = None;
                        self.reset_search();
                        return Found::Exploring(Controls::default());
                    }
                }
            }
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
    /// The world the searches see: what the settings and the player's
    /// pockets allow.
    fn cached<'a>(
        settings: &Settings,
        has_blocks: bool,
        world: &'a dyn BlockView,
        allow_mining: bool,
    ) -> CachedWorld<'a> {
        CachedWorld::new(world, allow_mining && settings.allow_break)
            .with_moves(settings.allow_parkour, settings.allow_place && has_blocks)
    }

    fn plan_ahead(&mut self, end: BlockPos, goal: &Goal, allow_mining: bool, world: &dyn BlockView) {
        if self.next.is_some() {
            return;
        }
        let search = self
            .ahead
            .get_or_insert_with(|| AStar::new(end, goal.clone(), self.budget));
        let Some(path) = search.run(&Self::cached(&self.settings, self.has_blocks, world, allow_mining), SEARCH_SLICE) else {
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
                self.search_began = self.ticks;
                let (from, to) = (player.node(), goal.clone());
                self.say(2, || format!("Searching from {from} to {to} (mining {allow_mining})."));
            }
            let search = self.pending.as_mut().expect("started above");
            let cached = Self::cached(&self.settings, self.has_blocks, world, allow_mining);
            let Some(path) = search.run(&cached, SEARCH_SLICE) else {
                // not done: stand still and go on next tick
                let nodes = search.nodes_considered();
                self.say(3, || format!("Search slice done, {nodes} nodes so far."));
                return Drive::Controls(Controls::default());
            };
            let start = search.start();
            let nodes = search.nodes_considered();
            let took = self.ticks - self.search_began;
            self.pending = None;
            self.say(1, || {
                format!(
                    "Search over after {took} ticks, {nodes} nodes: {} steps, cost {:.0}{}.",
                    path.steps.len(),
                    path.cost,
                    if path.is_partial { ", partial" } else { "" }
                )
            });
            if path.steps.is_empty() {
                let failure = self.failures + 1;
                self.say(1, || format!("No path from {start}: failure {failure} of {MAX_FAILURES}."));
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
                    && !exec.is_still_valid(&Self::cached(&self.settings, self.has_blocks, world, allow_mining), 5)
                {
                    self.say(2, || "The path is no longer valid, planning again.".to_owned());
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

/// Baritone's `MineProcess.prune`: drops not next to a wanted block, then
/// the wanted blocks still there (or out of sight), breakable and not given
/// up on, nearest first, at most [`MINE_MAX_ORE_LOCATIONS`].
fn prune(
    world: &dyn BlockView,
    player: &PlayerState,
    locs: &[BlockPos],
    blocks: &[String],
    blacklist: &HashSet<BlockPos>,
    dropped: &[BlockPos],
) -> Vec<BlockPos> {
    let wanted = |p: BlockPos| world.is_block(p, blocks) == Some(true) && plausible_to_break(world, p);
    let dropped: Vec<BlockPos> = dropped
        .iter()
        .copied()
        .filter(|&drop| !locs.iter().any(|&p| p.distance_squared(drop) <= 9 && wanted(p)))
        .collect();
    let mut out: Vec<BlockPos> = Vec::new();
    for &p in locs {
        if out.contains(&p) {
            continue;
        }
        let there = match world.is_block(p, blocks) {
            None => true,
            Some(is) => is || dropped.contains(&p),
        };
        if there && plausible_to_break(world, p) && p.y >= world.min_y() && !blacklist.contains(&p) {
            out.push(p);
        }
    }
    let feet = player.node();
    out.sort_by_key(|p| p.distance_squared(feet));
    out.truncate(MINE_MAX_ORE_LOCATIONS);
    out
}

/// Baritone's `plausibleToBreak`: it can be broken at all (not bedrock, not
/// by a fluid).
fn plausible_to_break(world: &dyn BlockView, pos: BlockPos) -> bool {
    CachedWorld::new(world, true).cost_for_breaking_block(pos).is_finite()
}

/// Baritone's `internalMiningGoal`, with `internalMiningAirException`.
fn internal_mining_goal(world: &dyn BlockView, pos: BlockPos, locs: &[BlockPos], blocks: &[String]) -> bool {
    if locs.contains(&pos) {
        return true;
    }
    if world.cell(pos).is_some_and(|c| c.passable && !c.liquid) {
        return true;
    }
    world.is_block(pos, blocks) == Some(true) && plausible_to_break(world, pos)
}

/// Baritone's `MineProcess.coalesce` with `forceInternalMining`: the goal
/// that puts the feet or head into `loc`, making the most of wanted blocks
/// next to it.
fn coalesce(world: &dyn BlockView, loc: BlockPos, locs: &[BlockPos], blocks: &[String]) -> Goal {
    let assume_vertical_shaft_mine = !world.cell(loc.up(1)).is_some_and(|c| c.falling);
    let upward = internal_mining_goal(world, loc.up(1), locs, blocks);
    let downward = internal_mining_goal(world, loc.down(1), locs, blocks);
    let double_downward = internal_mining_goal(world, loc.down(2), locs, blocks);
    if upward == downward {
        if double_downward && assume_vertical_shaft_mine {
            Goal::ThreeBlocks(loc)
        } else {
            Goal::TwoBlocks(loc)
        }
    } else if upward {
        Goal::Block(loc)
    } else if double_downward && assume_vertical_shaft_mine {
        Goal::TwoBlocks(loc.down(1))
    } else {
        Goal::Block(loc.down(1))
    }
}

/// Where to stand on the nearest ground (or water) at least
/// [`DESCENT_MIN_DEPTH`] below `feet`, looking around it. A bot up on a sky
/// island has nothing to search up there: dropping to the ground beats
/// bridging away across the void.
fn descent_level(world: &dyn BlockView, feet: BlockPos) -> Option<i32> {
    let mut best: Option<(i32, i32)> = None;
    for dx in -DESCENT_RADIUS..=DESCENT_RADIUS {
        for dz in -DESCENT_RADIUS..=DESCENT_RADIUS {
            let dist = dx * dx + dz * dz;
            if best.is_some_and(|(d, _)| d <= dist) {
                continue;
            }
            let mut y = feet.y - 1;
            let floor = feet.y - DESCENT_MAX_DEPTH;
            while y >= floor.max(world.min_y()) {
                let Some(cell) = world.cell(BlockPos::new(feet.x + dx, y, feet.z + dz)) else {
                    break;
                };
                if cell.liquid && !cell.water {
                    break; // lava
                }
                let level = if cell.water {
                    y
                } else if cell.solid || cell.standable {
                    y + 1
                } else {
                    y -= 1;
                    continue;
                };
                if feet.y - level >= DESCENT_MIN_DEPTH {
                    best = Some((dist, level));
                }
                break;
            }
        }
    }
    best.map(|(_, level)| level)
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
    find_known(world, from, blocks, blacklist, 1).first().copied()
}

/// The `max` nearest blocks matching `blocks` within [`SEARCH_RADIUS`],
/// nearest first, searched in growing cubes until no nearer one can be left.
pub fn find_known(
    world: &dyn BlockView,
    from: BlockPos,
    blocks: &[String],
    blacklist: &HashSet<BlockPos>,
    max: usize,
) -> Vec<BlockPos> {
    let mut found: Vec<(i64, BlockPos)> = Vec::new();
    for r in 0..=SEARCH_RADIUS {
        // nothing further out can beat the farthest of a full list
        if found.len() >= max {
            found.sort_unstable_by_key(|&(d, p)| (d, p.x, p.y, p.z));
            found.truncate(max);
            if i64::from(r) * i64::from(r) > found[max - 1].0 {
                break;
            }
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
                        found.push((pos.distance_squared(from), pos));
                    }
                    // inside the cube only the two z faces are new
                    dz += if on_face || dz == r { 1 } else { 2 * r };
                }
            }
        }
    }
    found.sort_unstable_by_key(|&(d, p)| (d, p.x, p.y, p.z));
    found.truncate(max);
    found.into_iter().map(|(_, p)| p).collect()
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

    /// Ticks until the bot orders a block mined, `None` if it never does.
    fn until_mining(
        bot: &mut Bot,
        world: &TestWorld,
        player: &PlayerState,
        entities: &[EntityInfo],
    ) -> Option<Controls> {
        (0..40)
            .filter_map(|_| bot.tick(world, player, entities))
            .find(|c| c.mine.is_some())
    }

    /// Ticks until the order is over.
    fn until_done(bot: &mut Bot, world: &TestWorld, player: &PlayerState, entities: &[EntityInfo]) {
        for _ in 0..30 {
            if bot.tick(world, player, entities).is_none() {
                return;
            }
        }
        panic!("still going: {:?}", bot.goal());
    }

    #[test]
    fn mines_a_block_in_reach_and_counts_it() {
        let mut world = TestWorld::flat(10);
        world.set(BlockPos::new(2, 64, 0), Cell::solid(45.0), "coal_ore");
        let mut bot = Bot::new();
        bot.settings.mine_scan_dropped_items = false;
        let player = standing(1.5, 64.0, 0.5);
        assert_eq!(bot.command("basso mine 1 coal_ore", &world, &player), ["Mining 1 coal_ore."]);
        let controls = until_mining(&mut bot, &world, &player, &[]).expect("mines it");
        assert_eq!(controls.mine, Some(BlockPos::new(2, 64, 0)));
        assert!(!controls.forward);
        // no chat line per block
        assert!(bot.take_messages().is_empty());
        world.air(BlockPos::new(2, 64, 0));
        until_done(&mut bot, &world, &player, &[]);
        assert_eq!(bot.take_messages(), ["Mined 1 coal_ore."]);
    }

    #[test]
    fn mines_every_known_block_in_turn() {
        let mut world = TestWorld::flat(10);
        for x in [2, 3] {
            world.set(BlockPos::new(x, 64, 0), Cell::solid(45.0), "coal_ore");
        }
        let mut bot = Bot::new();
        bot.settings.mine_scan_dropped_items = false;
        let player = standing(1.5, 64.0, 0.5);
        bot.command("basso mine 2 coal_ore", &world, &player);
        let first = until_mining(&mut bot, &world, &player, &[]).unwrap().mine.unwrap();
        assert_eq!(first, BlockPos::new(2, 64, 0));
        world.air(first);
        let player = standing(2.5, 64.0, 0.5);
        let second = until_mining(&mut bot, &world, &player, &[]).unwrap().mine.unwrap();
        assert_eq!(second, BlockPos::new(3, 64, 0));
        world.air(second);
        until_done(&mut bot, &world, &player, &[]);
        assert_eq!(bot.take_messages(), ["Mined 2 coal_ore."]);
    }

    #[test]
    fn every_known_block_is_in_the_goal() {
        let mut world = TestWorld::flat(15);
        for x in [8, -9, 5] {
            world.set(BlockPos::new(x, 64, 0), Cell::solid(45.0), "coal_ore");
        }
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        bot.command("basso mine coal_ore", &world, &player);
        bot.tick(&world, &player, &[]);
        let Some(Goal::Composite(goals)) = bot.goal() else {
            panic!("{:?}", bot.goal());
        };
        assert_eq!(goals.len(), 3);
    }

    #[test]
    fn walks_to_a_block_out_of_reach_to_mine_it() {
        let mut world = TestWorld::flat(15);
        world.set(BlockPos::new(10, 64, 0), Cell::solid(45.0), "coal_ore");
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        bot.command("basso mine coal_ore", &world, &player);
        let controls = (0..30)
            .filter_map(|_| bot.tick(&world, &player, &[]))
            .find(|c| c.forward)
            .expect("walks");
        assert_eq!(controls.mine, None);
        // the goal is the ore itself, as Baritone's: the path breaks into it
        assert_eq!(
            bot.goal(),
            Some(&Goal::Composite(vec![Goal::Block(BlockPos::new(10, 64, 0))]))
        );
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
        until_done(&mut bot, &world, &player, &[]);
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
        let player = standing(1.5, 64.0, 0.5);
        assert_eq!(bot.command("basso mine log", &world, &player), ["Mining all log."]);
        let controls = (0..40)
            .filter_map(|_| bot.tick(&world, &player, &[]))
            .find(|c| c.mine.is_some())
            .expect("mines the birch log");
        assert_eq!(controls.mine, Some(BlockPos::new(2, 64, 0)));
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
    fn waits_for_the_drop_of_a_mined_block() {
        let mut world = TestWorld::flat(10);
        world.set(BlockPos::new(2, 64, 0), Cell::solid(45.0), "coal_ore");
        let mut bot = Bot::new();
        let player = standing(1.5, 64.0, 0.5);
        bot.command("basso mine 1 coal_ore", &world, &player);
        until_mining(&mut bot, &world, &player, &[]).expect("mines it");
        world.air(BlockPos::new(2, 64, 0));
        // the ore's drop is expected where it was: go there and wait
        let controls = bot.tick(&world, &player, &[]).unwrap();
        assert!(controls.forward);
        assert!(bot.is_active());
        let there = standing(2.5, 64.0, 0.5);
        assert!(!bot.tick(&world, &there, &[]).unwrap().forward);
        assert!(bot.is_active());
        // the wait is over after a few ticks
        until_done(&mut bot, &world, &there, &[]);
        assert_eq!(bot.take_messages(), ["Mined 1 coal_ore."]);
    }

    #[test]
    fn picks_up_a_matching_item_before_finishing() {
        let mut world = TestWorld::flat(10);
        world.set(BlockPos::new(2, 64, 0), Cell::solid(60.0), "oak_log");
        let mut bot = Bot::new();
        let player = standing(1.5, 64.0, 0.5);
        bot.command("basso mine 1 oak_log", &world, &player);
        until_mining(&mut bot, &world, &player, &[]).expect("mines it");
        world.air(BlockPos::new(2, 64, 0));
        let drop = [item(9, "oak_log", 6.5, 0.5)];
        for _ in 0..20 {
            assert!(bot.tick(&world, &player, &drop).is_some());
        }
        let Some(Goal::Composite(goals)) = bot.goal() else {
            panic!("{:?}", bot.goal());
        };
        assert!(goals.iter().any(|g| matches!(g, Goal::Block(p) if *p == BlockPos::new(6, 64, 0)
            || matches!(g, Goal::TwoBlocks(p) | Goal::ThreeBlocks(p) if *p == BlockPos::new(6, 64, 0)))));
        // picked up: done
        let there = standing(6.5, 64.0, 0.5);
        until_done(&mut bot, &world, &there, &[]);
        assert_eq!(bot.take_messages(), ["Mined 1 oak_log."]);
    }

    #[test]
    fn ignores_unrelated_items_and_can_be_turned_off() {
        let mut world = TestWorld::flat(10);
        world.set(BlockPos::new(2, 64, 0), Cell::solid(45.0), "coal_ore");
        let mut bot = Bot::new();
        let player = standing(1.5, 64.0, 0.5);
        bot.command("basso mine 1 coal_ore", &world, &player);
        until_mining(&mut bot, &world, &player, &[]).unwrap();
        world.air(BlockPos::new(2, 64, 0));
        // a dirt item far off is not wanted: only the ore's own drop is waited for
        let dirt = [item(3, "dirt", -8.5, 0.5)];
        until_done(&mut bot, &world, &player, &dirt);

        let mut world = TestWorld::flat(10);
        world.set(BlockPos::new(2, 64, 0), Cell::solid(45.0), "coal_ore");
        let mut bot = Bot::new();
        bot.settings.mine_scan_dropped_items = false;
        bot.command("basso mine 1 coal_ore", &world, &player);
        until_mining(&mut bot, &world, &player, &[]).unwrap();
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
        let Some(Goal::Composite(goals)) = bot.goal() else {
            panic!("{:?}", bot.goal());
        };
        assert!(goals.contains(&Goal::Block(BlockPos::new(10, 64, 0))));
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
        for _ in 0..EXPLORE_SCAN_TICKS + 2 {
            bot.tick(&world, &player, &[]);
        }
        let Some(Goal::Composite(goals)) = bot.goal() else {
            panic!("{:?}", bot.goal());
        };
        assert_eq!(goals.len(), 1);
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
    fn a_sky_island_drops_to_the_ground_below_before_exploring() {
        let mut world = TestWorld::default();
        // a 5x5 island at 63, ground 30 blocks lower
        for x in -12..=12 {
            for z in -12..=12 {
                world.set(BlockPos::new(x, 33, z), Cell::solid(30.0), "stone");
                if x.abs() <= 2 && z.abs() <= 2 {
                    world.set(BlockPos::new(x, 63, z), Cell::solid(30.0), "stone");
                }
            }
        }
        assert_eq!(descent_level(&world, BlockPos::new(0, 64, 0)), Some(34));
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        bot.command("basso mine gold_block", &world, &player);
        bot.tick(&world, &player, &[]);
        assert_eq!(bot.goal(), Some(&Goal::Y(34)));
    }

    #[test]
    fn water_below_is_a_landing_and_lava_is_not() {
        let mut world = TestWorld::default();
        for x in -3..=3 {
            for z in -3..=3 {
                world.set(BlockPos::new(x, 40, z), Cell::WATER, "water");
            }
        }
        assert_eq!(descent_level(&world, BlockPos::new(0, 64, 0)), Some(40));
        let mut world = TestWorld::default();
        for x in -3..=3 {
            for z in -3..=3 {
                world.set(BlockPos::new(x, 40, z), Cell::LAVA, "lava");
            }
        }
        assert_eq!(descent_level(&world, BlockPos::new(0, 64, 0)), None);
    }

    #[test]
    fn ground_close_below_is_not_worth_a_descent() {
        let world = TestWorld::flat(30);
        assert_eq!(descent_level(&world, BlockPos::new(0, 64, 0)), None);
        assert_eq!(descent_level(&world, BlockPos::new(0, 70, 0)), None);
    }

    fn buried_ore_world() -> TestWorld {
        let mut world = TestWorld::default();
        for x in -20..=20 {
            for z in -20..=20 {
                for y in 40..=63 {
                    world.stone(BlockPos::new(x, y, z));
                }
            }
        }
        world.set(BlockPos::new(5, 50, 3), Cell::solid(60.0), "diamond_ore");
        world
    }

    #[test]
    fn verbose_levels_say_more_and_normal_says_nothing_extra() {
        let player = standing(0.5, 64.0, 0.5);
        for (verbose, expect) in [(0u8, false), (1, true)] {
            let world = buried_ore_world();
            let mut bot = Bot::new();
            bot.command(&format!("basso set verbose {verbose}"), &world, &player);
            bot.command("basso mine diamond_ore", &world, &player);
            bot.take_messages();
            for _ in 0..30 {
                bot.tick(&world, &player, &[]);
            }
            let said = bot.take_messages();
            assert_eq!(said.iter().any(|m| m.starts_with("[v1] Search over")), expect, "{said:?}");
        }
    }

    #[test]
    fn a_buried_ore_gets_a_path_at_once() {
        let world = buried_ore_world();
        let player = standing(0.5, 64.0, 0.5);
        let mut bot = Bot::new();
        bot.command("basso set verbose 2", &world, &player);
        bot.command("basso mine diamond_ore", &world, &player);
        bot.take_messages();
        let mut moved_at = None;
        for tick in 0..60 {
            let controls = bot.tick(&world, &player, &[]).unwrap_or_default();
            if controls != Controls::default() && moved_at.is_none() {
                moved_at = Some(tick);
            }
        }
        let said = bot.take_messages().join("\n");
        assert!(said.contains("Search over"), "{said}");
        assert!(!said.contains("No path"), "{said}");
        assert!(moved_at.is_some_and(|t| t < 30), "{moved_at:?}\n{said}");
    }

    #[test]
    fn click_mode_toggles_and_a_click_sets_a_goto() {
        let world = TestWorld::flat(10);
        let player = standing(0.5, 64.0, 0.5);
        let mut bot = Bot::new();
        assert!(!bot.click_mode());
        assert!(bot.command("basso click", &world, &player)[0].starts_with("Click on"));
        assert!(bot.click_mode());
        assert_eq!(bot.command("basso click", &world, &player), ["Click off."]);
        assert!(!bot.click_mode());
        let line = bot.click_goto(Goal::Block(BlockPos::new(3, 64, 3)));
        assert_eq!(line, "Going to 3 64 3.");
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

    /// A world with the floor cut out for x in `from..=to`, across all z.
    fn with_gap(from: i32, to: i32) -> TestWorld {
        let mut world = TestWorld::flat(15);
        for x in from..=to {
            for z in -15..=15 {
                world.air(BlockPos::new(x, 63, z));
            }
        }
        world
    }

    fn first_controls(bot: &mut Bot, world: &TestWorld, player: &PlayerState) -> Option<Controls> {
        (0..40)
            .filter_map(|_| bot.tick(world, player, &[]))
            .find(|c| c.forward || c.jump || c.sneak)
    }

    #[test]
    fn bridges_a_wide_gap_when_it_has_blocks() {
        let world = with_gap(1, 8);
        let mut bot = Bot::new();
        let player = PlayerState { has_blocks: true, ..standing(0.5, 64.0, 0.5) };
        bot.command("basso goto 10 64 0", &world, &player);
        let controls = first_controls(&mut bot, &world, &player).expect("sets off");
        assert!(controls.sneak, "{controls:?}");
    }

    fn dropping(y: f64) -> PlayerState {
        PlayerState {
            pos: Vec3::new(0.5, y, 0.5),
            velocity: Vec3::new(0.0, -1.6, 0.0),
            on_ground: false,
            pitch: 90.0,
            fall_distance: 12.0,
            water_bucket: true,
            ..PlayerState::default()
        }
    }

    #[test]
    fn a_long_fall_during_an_order_puts_water_down() {
        let world = TestWorld::flat(10);
        let mut bot = Bot::new();
        bot.command("basso goto 5 64 5", &world, &standing(0.5, 90.0, 0.5));
        let c = bot.tick(&world, &dropping(66.0), &[]).expect("takes the fall");
        assert!(c.use_item && c.bucket == Some(crate::executor::Bucket::Water));
        assert_eq!(c.look.unwrap().1, 90.0);
    }

    #[test]
    fn the_clutch_can_be_switched_off_and_needs_an_order() {
        let world = TestWorld::flat(10);
        let mut bot = Bot::new();
        bot.settings.mlg_water_bucket = false;
        bot.command("basso goto 5 64 5", &world, &standing(0.5, 90.0, 0.5));
        let c = bot.tick(&world, &dropping(66.0), &[]);
        assert!(c.is_none_or(|c| c.bucket.is_none() && !c.use_item));
        // no order, no clutch: a player falling by themselves keeps the controls
        let mut idle = Bot::new();
        assert_eq!(idle.tick(&world, &dropping(66.0), &[]), None);
    }

    #[test]
    fn does_not_bridge_without_blocks_or_when_placing_is_off() {
        let world = with_gap(1, 8);
        for (has_blocks, allow_place) in [(false, true), (true, false)] {
            let mut bot = Bot::new();
            bot.settings.allow_place = allow_place;
            let player = PlayerState { has_blocks, ..standing(0.5, 64.0, 0.5) };
            bot.command("basso goto 10 64 0", &world, &player);
            let controls = first_controls(&mut bot, &world, &player);
            assert!(controls.is_none_or(|c| !c.sneak), "{has_blocks} {allow_place}");
        }
    }

    #[test]
    fn jumps_a_small_gap_unless_parkour_is_off() {
        let world = with_gap(1, 1);
        let mut bot = Bot::new();
        let player = standing(0.5, 64.0, 0.5);
        bot.command("basso goto 4 64 0", &world, &player);
        let controls = first_controls(&mut bot, &world, &player).expect("sets off");
        assert!(controls.forward && controls.sprint);
        assert!(bot.path_nodes().iter().any(|n| *n == BlockPos::new(2, 64, 0)));

        let mut bot = Bot::new();
        bot.settings.allow_parkour = false;
        bot.command("basso goto 4 64 0", &world, &player);
        let walked = first_controls(&mut bot, &world, &player);
        // no way over: a path to somewhere short of the gap, or none
        assert!(bot.path_nodes().iter().all(|n| n.x <= 0), "{:?}", bot.path_nodes());
        let _ = walked;
    }

    #[test]
    fn breaking_can_be_switched_off() {
        let mut world = TestWorld::flat(15);
        for y in 64..67 {
            for z in -8..=8 {
                world.stone(BlockPos::new(3, y, z));
            }
        }
        let mut bot = Bot::new();
        bot.settings.allow_break = false;
        let player = standing(0.5, 64.0, 0.5);
        bot.command("basso goto 6 64 0", &world, &player);
        for _ in 0..40 {
            if let Some(c) = bot.tick(&world, &player, &[]) {
                assert_eq!(c.mine, None);
            }
        }
    }
}

