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
            min_nodes: 10_000,
            max_nodes: 40_000,
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

pub fn a_star(world: &CachedWorld, start: BlockPos, goal: &Goal, budget: Budget) -> Path {
    let mut nodes = vec![Node {
        pos: start,
        came_from: u32::MAX,
        kind: MoveKind::Forward,
        g_score: 0.,
    }];
    let mut index_of: HashMap<BlockPos, u32> = HashMap::from([(start, 0)]);
    let mut open = BinaryHeap::from([Open {
        f_score: 0.,
        g_score: 0.,
        index: 0,
    }]);

    let mut best_paths = [0u32; 7];
    let mut best_path_scores = [goal.heuristic(start); 7];
    let mut num_nodes = 0usize;

    while let Some(Open { index, g_score, .. }) = open.pop() {
        let node_pos = nodes[index as usize].pos;
        if g_score > nodes[index as usize].g_score {
            continue;
        }
        num_nodes += 1;

        if goal.success(node_pos) {
            return Path {
                steps: reconstruct(&nodes, index),
                is_partial: false,
                cost: g_score,
            };
        }

        for edge in moves::successors(world, node_pos) {
            let tentative = g_score + edge.cost;
            let neighbor_index = match index_of.entry(edge.target) {
                Entry::Occupied(e) => {
                    let i = *e.get();
                    let node = &mut nodes[i as usize];
                    if node.g_score <= tentative {
                        continue;
                    }
                    node.came_from = index;
                    node.kind = edge.kind;
                    node.g_score = tentative;
                    i
                }
                Entry::Vacant(e) => {
                    let i = nodes.len() as u32;
                    e.insert(i);
                    nodes.push(Node {
                        pos: edge.target,
                        came_from: index,
                        kind: edge.kind,
                        g_score: tentative,
                    });
                    i
                }
            };
            let heuristic = goal.heuristic(edge.target);
            open.push(Open {
                f_score: tentative + heuristic,
                g_score: tentative,
                index: neighbor_index,
            });

            for (i, coefficient) in COEFFICIENTS.iter().enumerate() {
                let score = heuristic + tentative / coefficient;
                if best_path_scores[i] - score > MIN_IMPROVEMENT {
                    best_paths[i] = neighbor_index;
                    best_path_scores[i] = score;
                }
            }
        }

        if num_nodes >= budget.max_nodes || (num_nodes >= budget.min_nodes && best_paths[6] != 0)
        {
            break;
        }
    }

    // the first coefficient whose best node isn't the start, so the partial
    // path isn't uselessly short
    let best = best_paths.iter().position(|&n| n != 0).unwrap_or(0);
    Path {
        steps: reconstruct(&nodes, best_paths[best]),
        is_partial: true,
        cost: best_path_scores[best],
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
}
