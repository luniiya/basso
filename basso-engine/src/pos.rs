//! Block and world positions.

use std::ops::{Add, Sub};

/// A block position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    pub const fn offset(self, x: i32, y: i32, z: i32) -> Self {
        Self::new(self.x + x, self.y + y, self.z + z)
    }

    pub const fn up(self, n: i32) -> Self {
        self.offset(0, n, 0)
    }

    pub const fn down(self, n: i32) -> Self {
        self.offset(0, -n, 0)
    }

    /// The middle of the block's floor, where a player standing in it is.
    pub fn bottom_center(self) -> Vec3 {
        Vec3::new(
            f64::from(self.x) + 0.5,
            f64::from(self.y),
            f64::from(self.z) + 0.5,
        )
    }

    /// The middle of the block.
    pub fn center(self) -> Vec3 {
        Vec3::new(
            f64::from(self.x) + 0.5,
            f64::from(self.y) + 0.5,
            f64::from(self.z) + 0.5,
        )
    }

    /// The block a player's feet are in, counting a non-full block they stand
    /// on (a slab, soul sand) as the block above it, as Azalea does.
    pub fn of_feet(feet: Vec3) -> Self {
        Self::new(
            feet.x.floor() as i32,
            (feet.y + 0.5).floor() as i32,
            feet.z.floor() as i32,
        )
    }

    /// The block that contains `pos`.
    pub fn containing(pos: Vec3) -> Self {
        Self::new(
            pos.x.floor() as i32,
            pos.y.floor() as i32,
            pos.z.floor() as i32,
        )
    }

    pub fn distance_squared(self, other: Self) -> i64 {
        let d = |a: i32, b: i32| i64::from(a - b).pow(2);
        d(self.x, other.x) + d(self.y, other.y) + d(self.z, other.z)
    }

    /// The six blocks sharing a face with this one.
    pub fn neighbors(self) -> [Self; 6] {
        [
            self.up(1),
            self.down(1),
            self.offset(1, 0, 0),
            self.offset(-1, 0, 0),
            self.offset(0, 0, 1),
            self.offset(0, 0, -1),
        ]
    }
}

impl Add for BlockPos {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        self.offset(o.x, o.y, o.z)
    }
}

impl Sub for BlockPos {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        self.offset(-o.x, -o.y, -o.z)
    }
}

impl std::fmt::Display for BlockPos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {} {}", self.x, self.y, self.z)
    }
}

/// A position in the world.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn distance_squared(self, o: Self) -> f64 {
        (self.x - o.x).powi(2) + (self.y - o.y).powi(2) + (self.z - o.z).powi(2)
    }

    pub fn horizontal_distance(self, o: Self) -> f64 {
        ((self.x - o.x).powi(2) + (self.z - o.z).powi(2)).sqrt()
    }
}

/// Minecraft's yaw and pitch, in degrees, to look from `from` at `to`.
pub fn yaw_pitch(from: Vec3, to: Vec3) -> (f32, f32) {
    let (dx, dy, dz) = (to.x - from.x, to.y - from.y, to.z - from.z);
    let yaw = (-dx).atan2(dz).to_degrees();
    let pitch = (-dy).atan2((dx * dx + dz * dz).sqrt()).to_degrees();
    (yaw as f32, pitch.clamp(-90.0, 90.0) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feet_on_a_slab_count_as_the_block_above() {
        assert_eq!(
            BlockPos::of_feet(Vec3::new(0.3, 64.5, -0.2)),
            BlockPos::new(0, 65, -1)
        );
        assert_eq!(
            BlockPos::of_feet(Vec3::new(0.3, 64.0, 0.2)),
            BlockPos::new(0, 64, 0)
        );
        assert_eq!(
            BlockPos::of_feet(Vec3::new(0.3, 63.875, 0.2)),
            BlockPos::new(0, 64, 0)
        );
    }

    #[test]
    fn yaw_follows_minecraft() {
        let o = Vec3::new(0.0, 0.0, 0.0);
        // yaw 0 faces +z (south), 90 faces -x (west), -90 faces +x (east).
        assert_eq!(yaw_pitch(o, Vec3::new(0.0, 0.0, 1.0)).0.round(), 0.0);
        assert_eq!(yaw_pitch(o, Vec3::new(-1.0, 0.0, 0.0)).0.round(), 90.0);
        assert_eq!(yaw_pitch(o, Vec3::new(1.0, 0.0, 0.0)).0.round(), -90.0);
        // looking down is a positive pitch
        assert_eq!(yaw_pitch(o, Vec3::new(0.0, -1.0, 0.0)).1.round(), 90.0);
    }

    #[test]
    fn neighbors_are_the_six_faces() {
        let p = BlockPos::new(1, 2, 3);
        for n in p.neighbors() {
            assert_eq!(p.distance_squared(n), 1);
        }
    }
}
