//! A* over block positions, from Azalea's `astar` (itself after Baritone's
//! `AbstractNodeCostSearch`), with a node budget instead of a time limit so a
//! search on the game thread stays short.

use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashMap, hash_map::Entry},
};

use crate::{
    goals::Goal,
    moves::{self, MoveKind},
    pos::BlockPos,
    world::CachedWorld,
};

/// For a better partial path when the search runs out, as Baritone does.
const COEFFICIENTS: [f32; 7] = [1.5, 2., 2.5, 3., 4., 5., 15.];
const MIN_IMPROVEMENT: f32 = 0.01;
/// Baritone's `MIN_DIST_PATH`: a partial path must end farther than this
/// from the start, or it is too short to be worth walking.
const MIN_DIST_PATH: i32 = 5;

/// One step of a path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Step {
    pub target: BlockPos,
    pub kind: MoveKind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub steps: Vec<Step>,
    /// The search ran out before reaching the goal; this is the best start.
    pub is_partial: bool,
    pub cost: f32,
}

/// How many nodes a search may look at.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    /// Stop after this many if a partial path is good enough.
    pub min_nodes: usize,
    /// Never look at more than this many.
    pub max_nodes: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            min_nodes: 30_000,
            max_nodes: 120_000,
        }
    }
}

struct Node {
    pos: BlockPos,
    came_from: u32,
    kind: MoveKind,
    g_score: f32,
}

#[derive(PartialEq)]
struct Open {
    f_score: f32,
    g_score: f32,
    index: u32,
}

impl Eq for Open {}

impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        // a min-heap on f, then preferring the deeper node
        other
            .f_score
            .total_cmp(&self.f_score)
            .then_with(|| self.g_score.total_cmp(&other.g_score))
    }
}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A search that can run a slice at a time, so the game can draw it while it
/// works (Baritone's best-path-so-far preview). The world is looked at again
/// each slice.
pub struct AStar {
    start: BlockPos,
    goal: Goal,
    budget: Budget,
    nodes: Vec<Node>,
    index_of: HashMap<BlockPos, u32>,
    open: BinaryHeap<Open>,
    best_paths: [u32; 7],
    best_path_scores: [f32; 7],
    num_nodes: usize,
    /// The node looked at last.
    recent: u32,
}

impl AStar {
    pub fn new(start: BlockPos, goal: Goal, budget: Budget) -> Self {
        let start_score = goal.heuristic(start);
        Self {
            start,
            goal,
            budget,
            nodes: vec![Node {
                pos: start,
                came_from: u32::MAX,
                kind: MoveKind::Forward,
                g_score: 0.,
            }],
            index_of: HashMap::from([(start, 0)]),
            open: BinaryHeap::from([Open {
                f_score: 0.,
                g_score: 0.,
                index: 0,
            }]),
            best_paths: [0; 7],
            best_path_scores: [start_score; 7],
            num_nodes: 0,
            recent: 0,
        }
    }

    pub fn goal(&self) -> &Goal {
        &self.goal
    }

    pub fn start(&self) -> BlockPos {
        self.start
    }

    /// How many nodes it has looked at.
    pub fn nodes_considered(&self) -> usize {
        self.num_nodes
    }

    /// Look at up to `nodes` more nodes. `Some` once the search is over.
    pub fn run(&mut self, world: &CachedWorld, nodes: usize) -> Option<Path> {
        let stop_at = self.num_nodes.saturating_add(nodes);
        while let Some(Open { index, g_score, .. }) = self.open.pop() {
            let node_pos = self.nodes[index as usize].pos;
            if g_score > self.nodes[index as usize].g_score {
                continue;
            }
            self.num_nodes += 1;
            self.recent = index;

            if self.goal.success(node_pos) {
                return Some(Path {
                    steps: reconstruct(&self.nodes, index),
                    is_partial: false,
                    cost: g_score,
                });
            }

            for edge in moves::successors(world, node_pos) {
                let tentative = g_score + edge.cost;
                let neighbor_index = match self.index_of.entry(edge.target) {
                    Entry::Occupied(e) => {
                        let i = *e.get();
                        let node = &mut self.nodes[i as usize];
                        if node.g_score <= tentative {
                            continue;
                        }
                        node.came_from = index;
                        node.kind = edge.kind;
                        node.g_score = tentative;
                        i
                    }
                    Entry::Vacant(e) => {
                        let i = self.nodes.len() as u32;
                        e.insert(i);
                        self.nodes.push(Node {
                            pos: edge.target,
                            came_from: index,
                            kind: edge.kind,
                            g_score: tentative,
                        });
                        i
                    }
                };
                let heuristic = self.goal.heuristic(edge.target);
                self.open.push(Open {
                    f_score: tentative + heuristic,
                    g_score: tentative,
                    index: neighbor_index,
                });

                for (i, coefficient) in COEFFICIENTS.iter().enumerate() {
                    let score = heuristic + tentative / coefficient;
                    if self.best_path_scores[i] - score > MIN_IMPROVEMENT {
                        self.best_paths[i] = neighbor_index;
                        self.best_path_scores[i] = score;
                    }
                }
            }

            if self.num_nodes >= self.budget.max_nodes
                || (self.num_nodes >= self.budget.min_nodes && self.far_enough().is_some())
            {
                return Some(self.partial());
            }
            if self.num_nodes >= stop_at {
                return None;
            }
        }
        Some(self.partial())
    }

    /// Baritone's `bestSoFar`: the first coefficient whose best node is more
    /// than [`MIN_DIST_PATH`] from the start.
    fn far_enough(&self) -> Option<usize> {
        let min = MIN_DIST_PATH * MIN_DIST_PATH;
        self.best_paths.iter().position(|&n| {
            let pos = self.nodes[n as usize].pos;
            let (dx, dy, dz) = (pos.x - self.start.x, pos.y - self.start.y, pos.z - self.start.z);
            dx * dx + dy * dy + dz * dz > min
        })
    }

    /// The coefficient the partial path follows: one that goes far enough,
    /// else (only when the search had to stop) the first that goes anywhere.
    fn best(&self) -> usize {
        self.far_enough()
            .or_else(|| self.best_paths.iter().position(|&n| n != 0))
            .unwrap_or(0)
    }

    fn partial(&self) -> Path {
        let best = self.best();
        Path {
            steps: reconstruct(&self.nodes, self.best_paths[best]),
            is_partial: true,
            cost: self.best_path_scores[best],
        }
    }

    fn positions(&self, index: u32) -> Vec<BlockPos> {
        std::iter::once(self.start)
            .chain(reconstruct(&self.nodes, index).into_iter().map(|s| s.target))
            .collect()
    }

    /// Baritone's `bestPathSoFar`, from the start, for drawing.
    pub fn best_so_far(&self) -> Vec<BlockPos> {
        self.positions(self.best_paths[self.best()])
    }

    /// Baritone's `pathToMostRecentNodeConsidered`, for drawing.
    pub fn most_recent(&self) -> Vec<BlockPos> {
        self.positions(self.recent)
    }
}

impl std::fmt::Debug for AStar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AStar")
            .field("start", &self.start)
            .field("goal", &self.goal)
            .field("nodes_considered", &self.num_nodes)
            .finish_non_exhaustive()
    }
}

pub fn a_star(world: &CachedWorld, start: BlockPos, goal: &Goal, budget: Budget) -> Path {
    let mut search = AStar::new(start, goal.clone(), budget);
    loop {
        if let Some(path) = search.run(world, usize::MAX) {
            return path;
        }
    }
}

fn reconstruct(nodes: &[Node], mut index: u32) -> Vec<Step> {
    let mut steps = Vec::new();
    while index != u32::MAX {
        let node = &nodes[index as usize];
        if node.came_from == u32::MAX {
            break;
        }
        steps.push(Step {
            target: node.pos,
            kind: node.kind,
        });
        index = node.came_from;
    }
    steps.reverse();
    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_world::TestWorld;

    fn search(world: &TestWorld, start: BlockPos, goal: Goal, mining: bool) -> Path {
        let cached = CachedWorld::new(world, mining);
        a_star(&cached, start, &goal, Budget::default())
    }

    fn assert_connected(start: BlockPos, path: &Path) {
        let mut at = start;
        for step in &path.steps {
            let d = step.target - at;
            assert!(d.x.abs() <= 1 && d.z.abs() <= 1 && d.y.abs() <= 3, "{at} -> {}", step.target);
            at = step.target;
        }
    }

    #[test]
    fn walks_straight_on_flat_ground() {
        let world = TestWorld::flat(20);
        let start = BlockPos::new(0, 64, 0);
        let path = search(&world, start, Goal::Block(BlockPos::new(10, 64, 0)), false);
        assert!(!path.is_partial);
        assert_eq!(path.steps.len(), 10);
        assert_eq!(path.steps.last().unwrap().target, BlockPos::new(10, 64, 0));
        assert_connected(start, &path);
    }

    #[test]
    fn goes_diagonally_when_shorter() {
        let world = TestWorld::flat(20);
        let path = search(
            &world,
            BlockPos::new(0, 64, 0),
            Goal::Block(BlockPos::new(5, 64, 5)),
            false,
        );
        assert_eq!(path.steps.len(), 5);
        assert!(path.steps.iter().all(|s| s.kind == MoveKind::Diagonal));
    }

    #[test]
    fn already_there_is_an_empty_path() {
        let world = TestWorld::flat(5);
        let start = BlockPos::new(0, 64, 0);
        let path = search(&world, start, Goal::Block(start), false);
        assert!(!path.is_partial);
        assert!(path.steps.is_empty());
    }

    #[test]
    fn walks_around_a_wall() {
        let mut world = TestWorld::flat(20);
        for z in -5..=5 {
            for y in 64..=66 {
                world.stone(BlockPos::new(3, y, z));
            }
        }
        let start = BlockPos::new(0, 64, 0);
        let path = search(&world, start, Goal::Block(BlockPos::new(6, 64, 0)), false);
        assert!(!path.is_partial);
        assert_connected(start, &path);
        assert!(path.steps.iter().all(|s| s.target.x != 3 || s.target.z.abs() > 5));
    }

    #[test]
    fn climbs_stairs_of_blocks() {
        let mut world = TestWorld::flat(20);
        for i in 1..=4 {
            for y in 64..64 + i {
                world.stone(BlockPos::new(i, y, 0));
            }
        }
        // walls on the sides so it has to climb
        let start = BlockPos::new(0, 64, 0);
        let goal = BlockPos::new(4, 68, 0);
        let path = search(&world, start, Goal::Block(goal), false);
        assert!(!path.is_partial);
        assert!(path.steps.iter().any(|s| s.kind == MoveKind::Ascend));
        assert_eq!(path.steps.last().unwrap().target, goal);
    }

    #[test]
    fn digs_down_to_reach_a_block_below() {
        let mut world = TestWorld::flat(5);
        for y in 55..63 {
            for x in -5..=5 {
                for z in -5..=5 {
                    world.stone(BlockPos::new(x, y, z));
                }
            }
        }
        let start = BlockPos::new(0, 64, 0);
        let path = search(&world, start, Goal::TwoBlocks(BlockPos::new(0, 60, 0)), true);
        assert!(!path.is_partial, "{path:?}");
        assert_connected(start, &path);
        let end = path.steps.last().unwrap().target;
        assert!(end == BlockPos::new(0, 60, 0) || end == BlockPos::new(0, 59, 0));
    }

    #[test]
    fn unreachable_goal_gives_a_partial_path_toward_it() {
        // the world ends at x = 10
        let world = TestWorld::flat(10);
        let start = BlockPos::new(0, 64, 0);
        let path = search(&world, start, Goal::Block(BlockPos::new(100, 64, 0)), false);
        assert!(path.is_partial);
        assert!(!path.steps.is_empty());
        assert!(path.steps.last().unwrap().target.x > 5);
    }

    #[test]
    fn a_partial_path_goes_farther_than_min_dist() {
        let world = TestWorld::flat(60);
        let cached = CachedWorld::new(&world, false);
        let start = BlockPos::new(0, 64, 0);
        let path = a_star(
            &cached,
            start,
            &Goal::Block(BlockPos::new(55, 64, 40)),
            Budget {
                min_nodes: 50,
                max_nodes: 2_000,
            },
        );
        assert!(path.is_partial);
        let end = path.steps.last().unwrap().target;
        let (dx, dz) = (end.x - start.x, end.z - start.z);
        assert!(dx * dx + dz * dz > MIN_DIST_PATH * MIN_DIST_PATH, "{end:?}");
    }

    #[test]
    fn budget_caps_the_search() {
        let world = TestWorld::flat(60);
        let cached = CachedWorld::new(&world, false);
        let path = a_star(
            &cached,
            BlockPos::new(0, 64, 0),
            &Goal::Y(200),
            Budget {
                min_nodes: 100,
                max_nodes: 200,
            },
        );
        assert!(path.is_partial);
    }

    #[test]
    fn sliced_search_finds_the_same_path() {
        let world = TestWorld::flat(20);
        let start = BlockPos::new(0, 64, 0);
        let goal = Goal::Block(BlockPos::new(12, 64, 7));
        let whole = search(&world, start, goal.clone(), false);
        let cached = CachedWorld::new(&world, false);
        let mut sliced = AStar::new(start, goal, Budget::default());
        let mut slices = 0;
        let path = loop {
            slices += 1;
            if let Some(path) = sliced.run(&cached, 3) {
                break path;
            }
        };
        assert!(slices > 1);
        assert_eq!(path, whole);
    }

    #[test]
    fn previews_start_at_the_start_while_searching() {
        let world = TestWorld::flat(30);
        let start = BlockPos::new(0, 64, 0);
        let cached = CachedWorld::new(&world, false);
        let mut search = AStar::new(start, Goal::Block(BlockPos::new(25, 64, 0)), Budget::default());
        assert_eq!(search.best_so_far(), [start]);
        assert_eq!(search.run(&cached, 10), None);
        assert_eq!(search.nodes_considered(), 10);
        let best = search.best_so_far();
        assert_eq!(best[0], start);
        assert!(best.len() > 1);
        // heading for the goal
        assert!(best.last().unwrap().x > 0);
        assert_eq!(search.most_recent()[0], start);
        assert_eq!(search.start(), start);
        assert_eq!(search.goal(), &Goal::Block(BlockPos::new(25, 64, 0)));
    }

    #[test]
    fn climbs_a_ladder_to_a_goal_high_up() {
        let mut world = TestWorld::flat(10);
        let ladder = crate::world::Cell {
            climbable: true,
            ..crate::world::Cell::AIR
        };
        for y in 64..72 {
            world.stone(BlockPos::new(2, y, 0));
            world.set(BlockPos::new(1, y, 0), ladder, "ladder");
        }
        let start = BlockPos::new(0, 64, 0);
        let goal = BlockPos::new(2, 72, 0);
        let path = search(&world, start, Goal::Block(goal), false);
        assert!(!path.is_partial, "{path:?}");
        assert_connected(start, &path);
        assert!(path.steps.iter().filter(|s| s.kind == MoveKind::Climb).count() >= 6);
        assert_eq!(path.steps.last().unwrap().target, goal);
    }

    fn fence_wall() -> TestWorld {
        let mut world = TestWorld::flat(10);
        let fence = crate::blocks::classify(&crate::blocks::BlockTraits {
            name: "minecraft:oak_fence",
            ..Default::default()
        });
        for z in -3..=3 {
            world.set(BlockPos::new(2, 64, z), fence, "oak_fence");
        }
        world
    }

    #[test]
    fn walks_around_a_fence_wall() {
        let world = fence_wall();
        let start = BlockPos::new(0, 64, 0);
        let goal = BlockPos::new(4, 64, 0);
        let path = search(&world, start, Goal::Block(goal), false);
        assert!(!path.is_partial, "{path:?}");
        assert_connected(start, &path);
        assert!(path.steps.iter().all(|s| !(s.target.x == 2 && (-3..=3).contains(&s.target.z) && s.target.y <= 65)));
        assert_eq!(path.steps.last().unwrap().target, goal);
    }

    #[test]
    fn never_stands_on_top_of_a_fence_even_when_it_may_break_blocks() {
        let world = fence_wall();
        let start = BlockPos::new(0, 64, 0);
        let goal = BlockPos::new(4, 64, 0);
        let path = search(&world, start, Goal::Block(goal), true);
        assert_connected(start, &path);
        assert!(path.steps.iter().all(|s| !(s.target.x == 2 && (-3..=3).contains(&s.target.z) && s.target.y == 65)));
    }

    #[test]
    fn walks_across_a_floor_of_trapdoors_over_the_void() {
        let mut world = TestWorld::default();
        let trapdoor = crate::world::Cell {
            thin_floor: true,
            ..crate::world::Cell::AIR
        };
        world.stone(BlockPos::new(0, 63, 0));
        for x in 1..=6 {
            world.set(BlockPos::new(x, 64, 0), trapdoor, "oak_trapdoor");
        }
        let start = BlockPos::new(0, 64, 0);
        let goal = BlockPos::new(6, 64, 0);
        let path = search(&world, start, Goal::Block(goal), false);
        assert!(!path.is_partial, "{path:?}");
        assert_connected(start, &path);
        assert_eq!(path.steps.last().unwrap().target, goal);
    }
}
