//! The water bucket clutch: falling far enough to be hurt, look down and put
//! water on the ground just before landing, then take it back.

use crate::{
    executor::{Bucket, Controls, PlayerState},
    pos::BlockPos,
    world::BlockView,
};

/// Falls up to this many blocks do no damage.
const MIN_FALL: f64 = 3.0;
/// How far above the ground the player is when the click can still reach it.
const REACH_HEIGHT: f64 = 2.8;
/// How many blocks below are looked at for the ground.
const LOOK_DOWN: i32 = 12;
/// Ticks of waiting for the water to show before giving up.
const SETTLE_TICKS: u32 = 4;
/// Ticks before a click that did not take is made again.
const RETRY_TICKS: u32 = 4;
const GIVE_UP_TICKS: u32 = 80;

#[derive(Debug)]
struct Placed {
    /// The block the water is in.
    at: BlockPos,
    ticks: u32,
}

#[derive(Debug, Default)]
pub struct Mlg {
    placed: Option<Placed>,
}

impl Mlg {
    /// Controls while a fall is being clutched or the water is picked up;
    /// `None` when there is nothing to do. `enabled` only keeps a new clutch
    /// from starting: one begun is always finished.
    pub fn tick(&mut self, world: &dyn BlockView, player: &PlayerState, enabled: bool) -> Option<Controls> {
        if let Some(placed) = self.placed.take() {
            return self.recover(placed, world, player);
        }
        if !enabled
            || !player.water_bucket
            || player.on_ground
            || player.in_water
            || player.velocity.y > -0.5
            || player.fall_distance <= MIN_FALL
        {
            return None;
        }
        let feet = player.node();
        let mut ground = None;
        for down in 0..=LOOK_DOWN {
            let at = feet.down(down);
            match world.cell(at) {
                // water or lava below: nothing to clutch (and lava is no place for it)
                Some(cell) if cell.liquid || cell.water => return None,
                Some(cell) if !cell.passable => {
                    ground = Some(at);
                    break;
                }
                _ => {}
            }
        }
        let mut controls = Controls {
            look: Some((player.yaw, 90.0)),
            bucket: Some(Bucket::Water),
            ..Controls::default()
        };
        // still far above anything: be looking down, bucket in hand, when it comes
        let Some(ground) = ground else {
            return Some(controls);
        };
        let height = player.pos.y - f64::from(ground.y + 1);
        if height <= REACH_HEIGHT && player.pitch > 89.0 {
            controls.use_item = true;
            self.placed = Some(Placed { at: ground.up(1), ticks: 0 });
        }
        Some(controls)
    }

    fn recover(&mut self, mut placed: Placed, world: &dyn BlockView, player: &PlayerState) -> Option<Controls> {
        placed.ticks += 1;
        let water = world.cell(placed.at).is_some_and(|c| c.water);
        let landed = player.on_ground || player.in_water;
        if placed.ticks > GIVE_UP_TICKS || (landed && !water && placed.ticks > SETTLE_TICKS) {
            return None;
        }
        let mut controls = Controls {
            look: Some((player.yaw, 90.0)),
            bucket: Some(Bucket::Water),
            ..Controls::default()
        };
        // a click is held for two ticks, so the game sees it, then let go
        let click = placed.ticks % 4 < 2;
        if placed.ticks == 1 && !landed {
            // the second tick of the click that began it
            controls.use_item = true;
        } else if !landed && !water && click && placed.ticks >= RETRY_TICKS && player.water_bucket {
            // the first click missed: still holding the water
            controls.use_item = true;
        } else if landed && water && player.empty_bucket {
            controls.bucket = Some(Bucket::Empty);
            controls.use_item = click;
        }
        self.placed = Some(placed);
        Some(controls)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{pos::Vec3, world::{Cell, test_world::TestWorld}};

    fn falling(y: f64) -> PlayerState {
        PlayerState {
            pos: Vec3::new(0.5, y, 0.5),
            velocity: Vec3::new(0.0, -1.5, 0.0),
            pitch: 90.0,
            fall_distance: 10.0,
            water_bucket: true,
            ..PlayerState::default()
        }
    }

    // the ground is stone at y 63 (top 64)
    fn world() -> TestWorld {
        TestWorld::flat(5)
    }

    #[test]
    fn it_looks_down_while_falling_far() {
        let mut mlg = Mlg::default();
        let c = mlg.tick(&world(), &falling(80.0), true).expect("falls");
        assert_eq!(c.look.unwrap().1, 90.0);
        // with the bucket in hand already, but no click yet
        assert_eq!(c.bucket, Some(Bucket::Water));
        assert!(!c.use_item);
    }

    #[test]
    fn it_places_water_within_reach_of_the_ground() {
        let mut mlg = Mlg::default();
        let c = mlg.tick(&world(), &falling(66.0), true).unwrap();
        assert!(c.use_item && c.bucket == Some(Bucket::Water));
    }

    #[test]
    fn it_clicks_only_once_the_ground_is_in_reach() {
        // the click acts where the player is now: 5 up is out of reach
        let mut mlg = Mlg::default();
        assert!(!mlg.tick(&world(), &falling(69.0), true).unwrap().use_item);
        assert!(mlg.tick(&world(), &falling(66.5), true).unwrap().use_item);
    }

    #[test]
    fn it_waits_for_the_look_before_clicking() {
        let mut mlg = Mlg::default();
        let mut player = falling(66.0);
        player.pitch = 30.0;
        let c = mlg.tick(&world(), &player, true).unwrap();
        assert!(!c.use_item);
    }

    #[test]
    fn short_falls_and_slow_ones_are_left_alone() {
        let mut mlg = Mlg::default();
        let mut short = falling(70.0);
        short.fall_distance = 2.0;
        assert_eq!(mlg.tick(&world(), &short, true), None);
        let mut slow = falling(70.0);
        slow.velocity = Vec3::new(0.0, -0.1, 0.0);
        assert_eq!(mlg.tick(&world(), &slow, true), None);
    }

    #[test]
    fn nothing_without_a_bucket_or_when_switched_off() {
        let mut mlg = Mlg::default();
        let mut bare = falling(66.0);
        bare.water_bucket = false;
        assert_eq!(mlg.tick(&world(), &bare, true), None);
        assert_eq!(mlg.tick(&world(), &falling(66.0), false), None);
    }

    #[test]
    fn it_does_not_act_standing_or_swimming() {
        let mut mlg = Mlg::default();
        let mut standing = falling(64.0);
        standing.on_ground = true;
        assert_eq!(mlg.tick(&world(), &standing, true), None);
        let mut swimming = falling(66.0);
        swimming.in_water = true;
        assert_eq!(mlg.tick(&world(), &swimming, true), None);
    }

    #[test]
    fn it_lands_in_water_or_lava_on_its_own() {
        let mut w = world();
        w.set(BlockPos::new(0, 63, 0), Cell::WATER, "water");
        let mut mlg = Mlg::default();
        assert_eq!(mlg.tick(&w, &falling(66.0), true), None);
        let mut w = world();
        w.set(BlockPos::new(0, 63, 0), Cell::LAVA, "lava");
        assert_eq!(mlg.tick(&w, &falling(66.0), true), None);
    }

    #[test]
    fn far_above_the_ground_it_only_looks_down() {
        let mut mlg = Mlg::default();
        let c = mlg.tick(&TestWorld::flat(5), &falling(200.0), true).unwrap();
        assert!(!c.use_item);
        // over a void: no ground to put water on, never a click
        let mut w = TestWorld::flat(5);
        w.air(BlockPos::new(0, 63, 0));
        let mut mlg = Mlg::default();
        assert!(!mlg.tick(&w, &falling(66.0), true).unwrap().use_item);
    }

    fn with_water_at(w: &mut TestWorld) {
        w.set(BlockPos::new(0, 64, 0), Cell::WATER, "water");
    }

    #[test]
    fn it_picks_the_water_up_after_landing() {
        let mut w = world();
        let mut mlg = Mlg::default();
        mlg.tick(&w, &falling(66.0), true).unwrap();
        with_water_at(&mut w);
        let mut landed = PlayerState {
            pos: Vec3::new(0.5, 64.0, 0.5),
            in_water: true,
            on_ground: true,
            pitch: 90.0,
            empty_bucket: true,
            ..PlayerState::default()
        };
        let mut picked = false;
        for _ in 0..6 {
            let c = mlg.tick(&w, &landed, false).expect("recovering");
            assert_eq!(c.look.unwrap().1, 90.0);
            picked |= c.bucket == Some(Bucket::Empty) && c.use_item;
        }
        assert!(picked);
        // the water is gone: done
        let mut dry = world();
        dry.air(BlockPos::new(0, 64, 0));
        landed.in_water = false;
        let mut ended = false;
        for _ in 0..8 {
            ended |= mlg.tick(&dry, &landed, false).is_none();
        }
        assert!(ended);
    }

    #[test]
    fn it_clicks_again_when_the_first_click_missed() {
        let w = world();
        let mut mlg = Mlg::default();
        mlg.tick(&w, &falling(66.0), true).unwrap();
        let mut clicks = 0;
        for _ in 0..8 {
            let c = mlg.tick(&w, &falling(65.0), true).unwrap();
            clicks += usize::from(c.use_item);
        }
        assert!(clicks >= 1);
    }

    #[test]
    fn clicks_are_held_two_ticks_then_let_go() {
        let w = world();
        let mut mlg = Mlg::default();
        // the first click is the one that made it start
        let first = mlg.tick(&w, &falling(66.0), true).unwrap();
        assert!(first.use_item);
        let mut run = vec![first.use_item];
        for _ in 0..14 {
            run.push(mlg.tick(&w, &falling(65.0), true).unwrap().use_item);
        }
        let mut held = 0;
        let mut seen = 0;
        for click in run {
            held = if click { held + 1 } else { 0 };
            assert!(held <= 2, "held too long");
            seen += usize::from(held == 2);
        }
        assert!(seen >= 2, "the click is not repeated");
    }

    #[test]
    fn it_gives_up_in_the_end() {
        let w = world();
        let mut mlg = Mlg::default();
        mlg.tick(&w, &falling(66.0), true).unwrap();
        let mut hanging = falling(66.0);
        hanging.water_bucket = false;
        let mut ended = false;
        for _ in 0..(GIVE_UP_TICKS + 2) {
            ended |= mlg.tick(&w, &hanging, true).is_none();
        }
        assert!(ended);
    }
}
