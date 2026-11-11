//! The free camera (setting `freecam`): while basso plays, the view leaves the
//! player and flies on its own, as the Freecam mod does. The player's walking
//! keys fly it, the mouse turns it and the wheel changes its speed; it goes
//! through blocks.

use crate::{
    pos::{BlockPos, Vec3},
    world::BlockView,
};

/// Vanilla's mouse sensitivity factor (`Entity.turn`).
const TURN_SCALE: f64 = 0.15;
pub const MIN_SPEED: f64 = 0.05;
pub const MAX_SPEED: f64 = 5.0;
/// Blocks a tick at the start.
const START_SPEED: f64 = 0.6;
/// How much a wheel notch speeds it up or slows it down.
const SPEED_STEP: f64 = 1.25;
/// How far behind the player it starts.
const START_DISTANCE: f64 = 4.0;
/// How far in front of a wall it starts.
const WALL_MARGIN: f64 = 0.2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FreeCam {
    pub position: Vec3,
    /// Where it was the tick before, to draw between the two.
    pub previous: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// Blocks a tick.
    pub speed: f64,
}

/// The player's walking keys this tick, as `movement_input` hands them over:
/// `forward` and `strafe` from -1 to 1 (strafe positive is left).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FlyKeys {
    pub forward: f32,
    pub strafe: f32,
    pub up: bool,
    pub down: bool,
}

impl FreeCam {
    /// Behind and above eyes at `eye` looking `yaw`, in front of any wall
    /// in between, so the player is in sight at the start.
    pub fn behind(eye: Vec3, yaw: f32, world: &dyn BlockView) -> Self {
        let pitch = 30.0;
        let dir = look(yaw, pitch);
        let step = 0.1;
        let mut reach = 0.0;
        while reach < START_DISTANCE {
            let next = (reach + step).min(START_DISTANCE);
            match world.cell(BlockPos::containing(along(eye, dir, -next))) {
                Some(cell) if !cell.passable && !cell.water => break,
                _ => reach = next,
            }
        }
        let position = along(eye, dir, -(reach - WALL_MARGIN).max(0.0));
        Self {
            position,
            previous: position,
            yaw,
            pitch,
            speed: START_SPEED,
        }
    }

    /// Mouse movement, as `Entity.turn` takes it.
    pub fn turn(&mut self, dx: f64, dy: f64) {
        self.yaw = (f64::from(self.yaw) + dx * TURN_SCALE) as f32;
        self.pitch = ((f64::from(self.pitch) + dy * TURN_SCALE) as f32).clamp(-90.0, 90.0);
    }

    /// Wheel notches: up (positive) flies faster.
    pub fn change_speed(&mut self, notches: f64) {
        self.speed = (self.speed * SPEED_STEP.powf(notches)).clamp(MIN_SPEED, MAX_SPEED);
    }

    /// One tick of flying, as creative flight moves: forward and strafe
    /// along the yaw, up and down straight.
    pub fn fly(&mut self, keys: FlyKeys) {
        self.previous = self.position;
        let (forward, strafe) = (f64::from(keys.forward), f64::from(keys.strafe));
        let vertical = f64::from(i8::from(keys.up) - i8::from(keys.down));
        let length = (forward * forward + strafe * strafe + vertical * vertical).sqrt();
        if length < 1e-6 {
            return;
        }
        // diagonals are no faster than straight on
        let scale = self.speed / length.max(1.0);
        let (sin, cos) = f64::from(self.yaw).to_radians().sin_cos();
        let dx = strafe * cos - forward * sin;
        let dz = forward * cos + strafe * sin;
        self.position = Vec3::new(
            self.position.x + dx * scale,
            self.position.y + vertical * scale,
            self.position.z + dz * scale,
        );
    }

    /// Where it is `partial` of the way into this tick.
    pub fn at(&self, partial: f64) -> Vec3 {
        let (a, b) = (self.previous, self.position);
        Vec3::new(
            a.x + (b.x - a.x) * partial,
            a.y + (b.y - a.y) * partial,
            a.z + (b.z - a.z) * partial,
        )
    }
}

fn along(from: Vec3, dir: Vec3, t: f64) -> Vec3 {
    Vec3::new(from.x + dir.x * t, from.y + dir.y * t, from.z + dir.z * t)
}

/// The unit vector a yaw and pitch look along, as `Entity.calculateViewVector`.
pub fn look(yaw: f32, pitch: f32) -> Vec3 {
    let (yaw, pitch) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
    Vec3::new(-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_world::TestWorld;

    fn close(a: Vec3, b: Vec3) -> bool {
        a.distance_squared(b) < 1e-6
    }

    fn cam_at(x: f64, y: f64, z: f64, yaw: f32) -> FreeCam {
        let p = Vec3::new(x, y, z);
        FreeCam {
            position: p,
            previous: p,
            yaw,
            pitch: 0.0,
            speed: 1.0,
        }
    }

    #[test]
    fn look_follows_minecraft() {
        assert!(close(look(0.0, 0.0), Vec3::new(0.0, 0.0, 1.0)));
        assert!(close(look(90.0, 0.0), Vec3::new(-1.0, 0.0, 0.0)));
        assert!(close(look(0.0, 90.0), Vec3::new(0.0, -1.0, 0.0)));
    }

    #[test]
    fn turning_scales_and_clamps() {
        let mut c = cam_at(0.0, 0.0, 0.0, 0.0);
        c.turn(100.0, 0.0);
        assert_eq!(c.yaw, 15.0);
        c.turn(0.0, 10_000.0);
        assert_eq!(c.pitch, 90.0);
        c.turn(0.0, -100_000.0);
        assert_eq!(c.pitch, -90.0);
    }

    #[test]
    fn speed_stays_in_range() {
        let mut c = cam_at(0.0, 0.0, 0.0, 0.0);
        c.change_speed(1.0);
        assert!((c.speed - 1.25).abs() < 1e-9);
        c.change_speed(-1.0);
        assert!((c.speed - 1.0).abs() < 1e-9);
        c.change_speed(100.0);
        assert_eq!(c.speed, MAX_SPEED);
        c.change_speed(-100.0);
        assert_eq!(c.speed, MIN_SPEED);
    }

    #[test]
    fn forward_flies_where_it_looks() {
        // yaw 0 looks south (+z), yaw 90 west (-x)
        let mut c = cam_at(0.0, 70.0, 0.0, 0.0);
        c.fly(FlyKeys { forward: 1.0, ..FlyKeys::default() });
        assert!(close(c.position, Vec3::new(0.0, 70.0, 1.0)), "{:?}", c.position);
        let mut c = cam_at(0.0, 70.0, 0.0, 90.0);
        c.fly(FlyKeys { forward: 1.0, ..FlyKeys::default() });
        assert!(close(c.position, Vec3::new(-1.0, 70.0, 0.0)), "{:?}", c.position);
    }

    #[test]
    fn strafe_left_is_to_the_left() {
        // looking south (+z), left is east (+x)
        let mut c = cam_at(0.0, 70.0, 0.0, 0.0);
        c.fly(FlyKeys { strafe: 1.0, ..FlyKeys::default() });
        assert!(close(c.position, Vec3::new(1.0, 70.0, 0.0)), "{:?}", c.position);
    }

    #[test]
    fn looking_down_does_not_fly_down() {
        let mut c = cam_at(0.0, 70.0, 0.0, 0.0);
        c.pitch = 80.0;
        c.fly(FlyKeys { forward: 1.0, ..FlyKeys::default() });
        assert_eq!(c.position.y, 70.0);
    }

    #[test]
    fn up_and_down_go_straight() {
        let mut c = cam_at(0.0, 70.0, 0.0, 45.0);
        c.fly(FlyKeys { up: true, ..FlyKeys::default() });
        assert!(close(c.position, Vec3::new(0.0, 71.0, 0.0)));
        c.fly(FlyKeys { down: true, ..FlyKeys::default() });
        assert!(close(c.position, Vec3::new(0.0, 70.0, 0.0)));
        c.fly(FlyKeys { up: true, down: true, ..FlyKeys::default() });
        assert!(close(c.position, Vec3::new(0.0, 70.0, 0.0)));
    }

    #[test]
    fn diagonals_are_no_faster() {
        let mut c = cam_at(0.0, 70.0, 0.0, 0.0);
        c.fly(FlyKeys { forward: 1.0, strafe: 1.0, up: true, down: false });
        let moved = c.position.distance_squared(c.previous).sqrt();
        assert!((moved - 1.0).abs() < 1e-9, "{moved}");
    }

    #[test]
    fn no_keys_keeps_it_still_and_smooth() {
        let mut c = cam_at(0.0, 70.0, 0.0, 0.0);
        c.fly(FlyKeys { forward: 1.0, ..FlyKeys::default() });
        assert!(close(c.at(0.5), Vec3::new(0.0, 70.0, 0.5)));
        c.fly(FlyKeys::default());
        assert!(close(c.at(0.0), c.position) && close(c.at(1.0), c.position));
    }

    #[test]
    fn it_starts_behind_and_above_in_the_open() {
        let world = TestWorld::flat(20);
        let eye = Vec3::new(0.5, 65.62, 0.5);
        let c = FreeCam::behind(eye, 0.0, &world);
        assert!(c.position.z < eye.z - 2.0, "{:?}", c.position);
        assert!(c.position.y > eye.y, "{:?}", c.position);
        assert_eq!(c.position, c.previous);
    }

    #[test]
    fn it_starts_in_front_of_a_wall() {
        let mut world = TestWorld::flat(20);
        for y in 65..70 {
            world.stone(BlockPos::new(0, y, -1));
        }
        let c = FreeCam::behind(Vec3::new(0.5, 65.62, 0.5), 0.0, &world);
        assert!(c.position.z > 0.0, "{:?}", c.position);
    }
}
