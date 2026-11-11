//! Movement costs in ticks, from Baritone's `ActionCosts` by way of Azalea.

use std::sync::LazyLock;

pub const WALK_ONE_BLOCK_COST: f32 = 20. / 4.317; // 4.633
pub const SPRINT_ONE_BLOCK_COST: f32 = 20. / 5.612; // 3.564
pub const WALK_OFF_BLOCK_COST: f32 = WALK_ONE_BLOCK_COST * 0.8; // 3.706
pub const SPRINT_MULTIPLIER: f32 = SPRINT_ONE_BLOCK_COST / WALK_ONE_BLOCK_COST; // 0.769
pub const JUMP_PENALTY: f32 = 2.;
pub const ENTER_WATER_PENALTY: f32 = 3.;
pub const CENTER_AFTER_FALL_COST: f32 = WALK_ONE_BLOCK_COST - WALK_OFF_BLOCK_COST; // 0.927
pub const WALK_ONE_IN_WATER_COST: f32 = 20. / 1.960; // 10.204

/// Baritone's multiplier to turn x/z distance into ticks for the heuristic.
pub const COST_HEURISTIC: f32 = 3.563;

/// A tiebreaker so paths avoid breaking blocks when they can.
pub const BLOCK_BREAK_ADDITIONAL_PENALTY: f32 = 2.;

pub static FALL_1_25_BLOCKS_COST: LazyLock<f32> = LazyLock::new(|| distance_to_ticks(1.25));
pub static FALL_0_25_BLOCKS_COST: LazyLock<f32> = LazyLock::new(|| distance_to_ticks(0.25));
pub static JUMP_ONE_BLOCK_COST: LazyLock<f32> =
    LazyLock::new(|| *FALL_1_25_BLOCKS_COST - *FALL_0_25_BLOCKS_COST); // 3.163

/// The ticks a fall of n blocks takes, for n up to 64.
pub static FALL_N_BLOCKS_COST: LazyLock<[f32; 65]> =
    LazyLock::new(|| std::array::from_fn(|n| distance_to_ticks(n as f32)));

fn velocity(ticks: u32) -> f32 {
    (0.98f32.powi(ticks as i32) - 1.) * -3.92
}

fn distance_to_ticks(distance: f32) -> f32 {
    if distance == 0. {
        return 0.;
    }
    let mut tick_count = 0;
    let mut remaining_distance = distance;
    loop {
        let fall_distance_per_tick = velocity(tick_count);
        if fall_distance_per_tick >= remaining_distance {
            // a bit extra to prefer smaller falls of the same tick count
            return (tick_count as f32) + (remaining_distance / fall_distance_per_tick);
        }
        remaining_distance -= fall_distance_per_tick;
        tick_count += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_take_longer_the_further_they_go() {
        for n in 1..FALL_N_BLOCKS_COST.len() {
            assert!(FALL_N_BLOCKS_COST[n] > FALL_N_BLOCKS_COST[n - 1]);
        }
        assert_eq!(FALL_N_BLOCKS_COST[0], 0.);
    }

    #[test]
    fn matches_azalea_values() {
        assert!((*JUMP_ONE_BLOCK_COST - 3.163).abs() < 0.01);
        assert!((FALL_N_BLOCKS_COST[1] - 5.614).abs() < 0.01);
        assert!((FALL_N_BLOCKS_COST[2] - 7.788).abs() < 0.01);
    }
}
