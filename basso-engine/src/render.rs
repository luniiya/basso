//! What to draw in the world while basso works, after Baritone's
//! `PathRenderer`: the path as a red line through the node centres, red boxes
//! around blocks to break, and the goal as a green box whose rings bob. All
//! of it is lines, drawn through walls as Baritone draws them.

use crate::{
    goals::Goal,
    pos::{BlockPos, Vec3},
};

/// Baritone's `colorCurrentPath` at its line alpha 0.4.
pub const PATH_COLOR: u32 = 0x66FF_0000;
/// Baritone's `colorBlocksToBreak` at alpha 0.4.
pub const BREAK_COLOR: u32 = 0x66FF_0000;
/// Baritone's `colorGoalBox` at alpha 0.4.
pub const GOAL_COLOR: u32 = 0x6600_FF00;
/// Baritone's `colorBestPathSoFar`, shown while a search runs.
pub const BEST_SO_FAR_COLOR: u32 = 0x6600_00FF;
/// Baritone's `colorMostRecentConsidered`.
pub const RECENT_COLOR: u32 = 0x6600_FFFF;
/// Baritone's `pathRenderLineWidthPixels`.
pub const PATH_WIDTH: f32 = 5.0;
/// Baritone's `goalRenderLineWidthPixels`.
pub const GOAL_WIDTH: f32 = 3.0;
/// Baritone's `yLevelBoxSize`.
const Y_LEVEL_BOX_SIZE: f64 = 15.0;
/// How far boxes grow past the block, as Baritone's 0.002.
const EXPAND: f64 = 0.002;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub from: Vec3,
    pub to: Vec3,
    pub argb: u32,
    pub width: f32,
}

/// Everything to draw this frame. `seconds` is any steady clock, for the
/// goal's bobbing rings; `player` places a y-level goal; `min_y`/`max_y` are
/// the world's height limits for a column goal.
pub struct Scene<'a> {
    pub path: &'a [BlockPos],
    /// A search in progress: its best path so far and its newest node's path.
    pub searching: Option<(&'a [BlockPos], &'a [BlockPos])>,
    pub to_break: &'a [BlockPos],
    pub goal: Option<&'a Goal>,
    pub player: Vec3,
    pub seconds: f64,
    pub min_y: i32,
    pub max_y: i32,
}

pub fn lines(scene: &Scene) -> Vec<Line> {
    let mut out = Vec::new();
    path(scene.path, PATH_COLOR, &mut out);
    if let Some((best, recent)) = scene.searching {
        path(best, BEST_SO_FAR_COLOR, &mut out);
        path(recent, RECENT_COLOR, &mut out);
    }
    for &block in scene.to_break {
        let (min, max) = block_box(block);
        cuboid(min, max, BREAK_COLOR, PATH_WIDTH, &mut out);
    }
    if let Some(goal) = scene.goal {
        goal_box(goal, scene, &mut out);
    }
    out
}

/// The path through the node centres, straight runs merged, and again a
/// little higher as Baritone draws it when `renderPathAsLine` is off.
fn path(nodes: &[BlockPos], argb: u32, out: &mut Vec<Line>) {
    let corners = merged(nodes);
    for pair in corners.windows(2) {
        for lift in [0.5, 0.53] {
            let at = |p: BlockPos| Vec3::new(f64::from(p.x) + 0.5, f64::from(p.y) + lift, f64::from(p.z) + 0.5);
            out.push(Line {
                from: at(pair[0]),
                to: at(pair[1]),
                argb,
                width: PATH_WIDTH,
            });
        }
    }
}

/// The nodes where the path turns: runs along one direction become one
/// segment, as Baritone's `drawPath` joins them.
fn merged(nodes: &[BlockPos]) -> Vec<BlockPos> {
    let mut out: Vec<BlockPos> = Vec::new();
    for &node in nodes {
        if out.last() == Some(&node) {
            continue;
        }
        if let [.., a, b] = out[..] {
            let d1 = [b.x - a.x, b.y - a.y, b.z - a.z];
            let d2 = [node.x - b.x, node.y - b.y, node.z - b.z];
            // same direction: no cross product, and not turning back
            let cross = [
                d1[1] * d2[2] - d1[2] * d2[1],
                d1[2] * d2[0] - d1[0] * d2[2],
                d1[0] * d2[1] - d1[1] * d2[0],
            ];
            let dot = d1[0] * d2[0] + d1[1] * d2[1] + d1[2] * d2[2];
            if cross == [0, 0, 0] && dot > 0 {
                *out.last_mut().expect("two nodes") = node;
                continue;
            }
        }
        out.push(node);
    }
    out
}

fn block_box(p: BlockPos) -> (Vec3, Vec3) {
    let min = Vec3::new(f64::from(p.x) - EXPAND, f64::from(p.y) - EXPAND, f64::from(p.z) - EXPAND);
    let max = Vec3::new(
        f64::from(p.x) + 1.0 + EXPAND,
        f64::from(p.y) + 1.0 + EXPAND,
        f64::from(p.z) + 1.0 + EXPAND,
    );
    (min, max)
}

/// The 12 edges of a box.
fn cuboid(min: Vec3, max: Vec3, argb: u32, width: f32, out: &mut Vec<Line>) {
    ring(min.x, min.z, max.x, max.z, min.y, argb, width, out);
    ring(min.x, min.z, max.x, max.z, max.y, argb, width, out);
    pillars(min.x, min.z, max.x, max.z, min.y, max.y, argb, width, out);
}

/// A horizontal rectangle at `y`.
#[allow(clippy::too_many_arguments)]
fn ring(x0: f64, z0: f64, x1: f64, z1: f64, y: f64, argb: u32, width: f32, out: &mut Vec<Line>) {
    let c = [(x0, z0), (x1, z0), (x1, z1), (x0, z1)];
    for i in 0..4 {
        let (a, b) = (c[i], c[(i + 1) % 4]);
        out.push(Line {
            from: Vec3::new(a.0, y, a.1),
            to: Vec3::new(b.0, y, b.1),
            argb,
            width,
        });
    }
}

/// The four vertical edges between `y0` and `y1`.
#[allow(clippy::too_many_arguments)]
fn pillars(x0: f64, z0: f64, x1: f64, z1: f64, y0: f64, y1: f64, argb: u32, width: f32, out: &mut Vec<Line>) {
    for (x, z) in [(x0, z0), (x1, z0), (x1, z1), (x0, z1)] {
        out.push(Line {
            from: Vec3::new(x, y0, z),
            to: Vec3::new(x, y1, z),
            argb,
            width,
        });
    }
}

/// Baritone's bobbing offset: a cosine over two seconds, -1..1.
pub fn bob(seconds: f64) -> f64 {
    (seconds.rem_euclid(2.0) / 2.0 * std::f64::consts::TAU).cos()
}

/// Baritone's `drawDankLitGoalBox`.
fn goal_box(goal: &Goal, scene: &Scene, out: &mut Vec<Line>) {
    let y = bob(scene.seconds);
    if let Goal::Composite(goals) = goal {
        for goal in goals {
            goal_box(goal, scene, out);
        }
        return;
    }
    let column = |x: i32, z: i32| {
        let x0 = f64::from(x) + EXPAND;
        let z0 = f64::from(z) + EXPAND;
        (x0, z0, f64::from(x) + 1.0 - EXPAND, f64::from(z) + 1.0 - EXPAND)
    };
    let (x0, z0, x1, z1, min_y, max_y, rings) = match *goal {
        // the player stands in it: two blocks tall, rings around the middle
        Goal::Block(p) => {
            let (x0, z0, x1, z1) = column(p.x, p.z);
            let mid = f64::from(p.y) + 1.0;
            (x0, z0, x1, z1, f64::from(p.y), f64::from(p.y) + 2.0, Some((mid + y, mid - y)))
        }
        Goal::TwoBlocks(p) | Goal::GetToBlock(p) => {
            let (x0, z0, x1, z1) = column(p.x, p.z);
            let mid = f64::from(p.y) + 0.5;
            let y = y / 2.0;
            (x0, z0, x1, z1, f64::from(p.y), f64::from(p.y) + 1.0, Some((mid + y, mid - y)))
        }
        Goal::Radius { pos, .. } => {
            let p = BlockPos::containing(pos);
            let (x0, z0, x1, z1) = column(p.x, p.z);
            let mid = f64::from(p.y) + 1.0;
            (x0, z0, x1, z1, f64::from(p.y), f64::from(p.y) + 2.0, Some((mid + y, mid - y)))
        }
        Goal::XZ { x, z } => {
            let (x0, z0, x1, z1) = column(x, z);
            (x0, z0, x1, z1, f64::from(scene.min_y), f64::from(scene.max_y), None)
        }
        Goal::Composite(_) => unreachable!("drawn above"),
        Goal::Y(level) => {
            let p = scene.player;
            let level = f64::from(level);
            let mid = level + 1.0;
            (
                p.x - Y_LEVEL_BOX_SIZE,
                p.z - Y_LEVEL_BOX_SIZE,
                p.x + Y_LEVEL_BOX_SIZE,
                p.z + Y_LEVEL_BOX_SIZE,
                level,
                level + 2.0,
                Some((mid + y, mid - y)),
            )
        }
    };
    if let Some((top, bottom)) = rings {
        ring(x0, z0, x1, z1, top, GOAL_COLOR, GOAL_WIDTH, out);
        ring(x0, z0, x1, z1, bottom, GOAL_COLOR, GOAL_WIDTH, out);
    }
    pillars(x0, z0, x1, z1, min_y, max_y, GOAL_COLOR, GOAL_WIDTH, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene<'a>(path: &'a [BlockPos], to_break: &'a [BlockPos], goal: Option<&'a Goal>) -> Scene<'a> {
        Scene {
            path,
            searching: None,
            to_break,
            goal,
            player: Vec3::new(0.5, 64.0, 0.5),
            seconds: 0.0,
            min_y: -64,
            max_y: 320,
        }
    }

    #[test]
    fn straight_runs_merge() {
        let nodes: Vec<_> = (0..5).map(|x| BlockPos::new(x, 64, 0)).collect();
        assert_eq!(merged(&nodes), [BlockPos::new(0, 64, 0), BlockPos::new(4, 64, 0)]);
        let turn = [
            BlockPos::new(0, 64, 0),
            BlockPos::new(1, 64, 0),
            BlockPos::new(2, 64, 0),
            BlockPos::new(2, 64, 1),
            BlockPos::new(2, 64, 2),
        ];
        assert_eq!(
            merged(&turn),
            [BlockPos::new(0, 64, 0), BlockPos::new(2, 64, 0), BlockPos::new(2, 64, 2)]
        );
        let repeated = [BlockPos::new(0, 64, 0), BlockPos::new(0, 64, 0)];
        assert_eq!(merged(&repeated), [BlockPos::new(0, 64, 0)]);
        assert!(merged(&[]).is_empty());
    }

    #[test]
    fn path_goes_through_node_centres_twice() {
        let nodes = [BlockPos::new(0, 64, 0), BlockPos::new(1, 64, 0)];
        let lines = lines(&scene(&nodes, &[], None));
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].from, Vec3::new(0.5, 64.5, 0.5));
        assert_eq!(lines[0].to, Vec3::new(1.5, 64.5, 0.5));
        assert_eq!(lines[1].from.y, 64.53);
        assert!(lines.iter().all(|l| l.argb == PATH_COLOR && l.width == PATH_WIDTH));
    }

    #[test]
    fn a_single_node_draws_no_path() {
        assert!(lines(&scene(&[BlockPos::new(0, 64, 0)], &[], None)).is_empty());
    }

    #[test]
    fn blocks_to_break_get_a_slightly_larger_box() {
        let lines = lines(&scene(&[], &[BlockPos::new(2, 64, 0)], None));
        assert_eq!(lines.len(), 12);
        assert!(lines.iter().all(|l| l.argb == BREAK_COLOR));
        let min_x = lines.iter().map(|l| l.from.x.min(l.to.x)).fold(f64::MAX, f64::min);
        let max_y = lines.iter().map(|l| l.from.y.max(l.to.y)).fold(f64::MIN, f64::max);
        assert!((min_x - (2.0 - EXPAND)).abs() < 1e-9);
        assert!((max_y - (65.0 + EXPAND)).abs() < 1e-9);
    }

    #[test]
    fn block_goal_is_two_tall_with_two_rings() {
        let goal = Goal::Block(BlockPos::new(3, 64, 0));
        let lines = lines(&scene(&[], &[], Some(&goal)));
        // two rings of four, four pillars
        assert_eq!(lines.len(), 12);
        assert!(lines.iter().all(|l| l.argb == GOAL_COLOR && l.width == GOAL_WIDTH));
        let top = lines.iter().map(|l| l.to.y.max(l.from.y)).fold(f64::MIN, f64::max);
        let bottom = lines.iter().map(|l| l.to.y.min(l.from.y)).fold(f64::MAX, f64::min);
        assert_eq!((bottom, top), (64.0, 66.0));
        // at time 0 the rings sit at the top and bottom
        assert!(lines.iter().any(|l| l.from.y == 66.0 && l.to.y == 66.0));
        assert!(lines.iter().any(|l| l.from.y == 64.0 && l.to.y == 64.0));
    }

    #[test]
    fn rings_bob_through_the_middle() {
        assert_eq!(bob(0.0), 1.0);
        assert!((bob(1.0) + 1.0).abs() < 1e-9);
        assert!(bob(0.5).abs() < 1e-9);
        assert_eq!(bob(2.0), bob(0.0));
        let goal = Goal::Block(BlockPos::new(0, 64, 0));
        let mut s = scene(&[], &[], Some(&goal));
        s.seconds = 0.5;
        let lines = lines(&s);
        assert!(lines.iter().filter(|l| (l.from.y - 65.0).abs() < 1e-9 && l.from.y == l.to.y).count() == 8);
    }

    #[test]
    fn block_targets_are_one_tall() {
        for goal in [Goal::TwoBlocks(BlockPos::new(0, 64, 0)), Goal::GetToBlock(BlockPos::new(0, 64, 0))] {
            let lines = lines(&scene(&[], &[], Some(&goal)));
            let top = lines.iter().map(|l| l.to.y.max(l.from.y)).fold(f64::MIN, f64::max);
            assert_eq!(top, 65.0, "{goal:?}");
        }
    }

    #[test]
    fn column_goal_spans_the_world() {
        let goal = Goal::XZ { x: 5, z: 5 };
        let lines = lines(&scene(&[], &[], Some(&goal)));
        assert_eq!(lines.len(), 4);
        assert!(lines.iter().all(|l| l.from.y == -64.0 && l.to.y == 320.0));
    }

    #[test]
    fn y_goal_surrounds_the_player() {
        let goal = Goal::Y(12);
        let lines = lines(&scene(&[], &[], Some(&goal)));
        assert_eq!(lines.len(), 12);
        let min_x = lines.iter().map(|l| l.from.x.min(l.to.x)).fold(f64::MAX, f64::min);
        assert_eq!(min_x, 0.5 - Y_LEVEL_BOX_SIZE);
    }

    #[test]
    fn a_running_search_is_drawn_in_its_own_colors() {
        let best = [BlockPos::new(0, 64, 0), BlockPos::new(3, 64, 0)];
        let recent = [BlockPos::new(0, 64, 0), BlockPos::new(0, 64, 2)];
        let mut s = scene(&[], &[], None);
        s.searching = Some((&best, &recent));
        let lines = lines(&s);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines.iter().filter(|l| l.argb == BEST_SO_FAR_COLOR).count(), 2);
        assert_eq!(lines.iter().filter(|l| l.argb == RECENT_COLOR).count(), 2);
    }
}
