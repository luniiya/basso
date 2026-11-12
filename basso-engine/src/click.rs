//! Baritone's `#click`: point at a block, click, and go there. A left click
//! walks into the block, a right click walks onto it.

use crate::{
    goals::Goal,
    pos::{BlockPos, Vec3},
    world::BlockView,
};

/// How far the pointer reaches, as far as a loaded world goes.
pub const REACH: f64 = 256.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

/// The unit vector a view with this yaw and pitch (degrees) looks along.
pub fn look_dir(yaw: f32, pitch: f32) -> Vec3 {
    let (yaw, pitch) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
    Vec3::new(-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos())
}

/// The first block along the ray from `origin` that a click lands on: not air
/// and not fluid. `None` past `reach` or at the edge of the loaded world.
pub fn pick_block(world: &dyn BlockView, origin: Vec3, dir: Vec3, reach: f64) -> Option<BlockPos> {
    let mut cell = BlockPos::containing(origin);
    let step = |d: f64| if d > 0.0 { 1 } else { -1 };
    let (sx, sy, sz) = (step(dir.x), step(dir.y), step(dir.z));
    let next = |o: f64, c: i32, d: f64| {
        if d == 0.0 {
            f64::INFINITY
        } else {
            let edge = if d > 0.0 { f64::from(c) + 1.0 } else { f64::from(c) };
            (edge - o) / d
        }
    };
    let per = |d: f64| if d == 0.0 { f64::INFINITY } else { (1.0 / d).abs() };
    let (mut tx, mut ty, mut tz) = (next(origin.x, cell.x, dir.x), next(origin.y, cell.y, dir.y), next(origin.z, cell.z, dir.z));
    let (dx, dy, dz) = (per(dir.x), per(dir.y), per(dir.z));
    loop {
        let c = world.cell(cell)?;
        if !c.liquid && (c.solid || c.standable || !c.passable) {
            return Some(cell);
        }
        let t = tx.min(ty).min(tz);
        if t > reach {
            return None;
        }
        if tx <= ty && tx <= tz {
            cell.x += sx;
            tx += dx;
        } else if ty <= tz {
            cell.y += sy;
            ty += dy;
        } else {
            cell.z += sz;
            tz += dz;
        }
    }
}

/// Baritone's `mouseReleased`: left is the block, right the block above it.
pub fn goal(button: Button, block: BlockPos) -> Goal {
    match button {
        Button::Left => Goal::Block(block),
        Button::Right => Goal::Block(block.up(1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Cell, test_world::TestWorld};

    #[test]
    fn looking_directions() {
        // yaw 0 faces +z, 90 faces -x, pitch 90 is straight down
        let d = look_dir(0.0, 0.0);
        assert!((d.z - 1.0).abs() < 1e-9 && d.x.abs() < 1e-9);
        let d = look_dir(90.0, 0.0);
        assert!((d.x + 1.0).abs() < 1e-9);
        let d = look_dir(0.0, 90.0);
        assert!((d.y + 1.0).abs() < 1e-9);
    }

    #[test]
    fn picks_the_first_solid_block_along_the_ray() {
        let mut world = TestWorld::flat(20);
        world.stone(BlockPos::new(0, 64, 5));
        world.stone(BlockPos::new(0, 64, 9));
        let at = pick_block(&world, Vec3::new(0.5, 64.5, 0.5), look_dir(0.0, 0.0), REACH);
        assert_eq!(at, Some(BlockPos::new(0, 64, 5)));
    }

    #[test]
    fn looking_down_picks_the_floor() {
        let world = TestWorld::flat(20);
        let at = pick_block(&world, Vec3::new(3.5, 70.0, 3.5), look_dir(0.0, 90.0), REACH);
        assert_eq!(at, Some(BlockPos::new(3, 63, 3)));
    }

    #[test]
    fn a_slanted_ray_hits_a_far_block() {
        let world = TestWorld::flat(40);
        // from 10 up looking 45 degrees down: lands about 10 ahead
        let at = pick_block(&world, Vec3::new(0.5, 74.0, 0.5), look_dir(0.0, 45.0), REACH).unwrap();
        assert_eq!(at.y, 63);
        assert!((9..=11).contains(&at.z), "{at:?}");
    }

    #[test]
    fn fluid_is_looked_through() {
        let mut world = TestWorld::flat(20);
        world.set(BlockPos::new(0, 64, 3), Cell::WATER, "water");
        world.stone(BlockPos::new(0, 64, 6));
        let at = pick_block(&world, Vec3::new(0.5, 64.5, 0.5), look_dir(0.0, 0.0), REACH);
        assert_eq!(at, Some(BlockPos::new(0, 64, 6)));
    }

    #[test]
    fn nothing_in_reach_or_past_the_loaded_world() {
        let world = TestWorld::flat(5);
        // off into unloaded chunks
        assert_eq!(pick_block(&world, Vec3::new(0.5, 66.0, 0.5), look_dir(0.0, -10.0), REACH), None);
        // too short a reach
        assert_eq!(pick_block(&world, Vec3::new(0.5, 66.0, 0.5), look_dir(0.0, 90.0), 1.0), None);
    }

    #[test]
    fn left_goes_into_the_block_and_right_onto_it() {
        let b = BlockPos::new(4, 70, -2);
        assert_eq!(goal(Button::Left, b), Goal::Block(b));
        assert_eq!(goal(Button::Right, b), Goal::Block(BlockPos::new(4, 71, -2)));
    }
}
