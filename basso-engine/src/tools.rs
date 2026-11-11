//! Baritone's `autoTool`: which hotbar slot breaks a block fastest. The game
//! says how fast each slot's item mines the block and whether it harvests it;
//! this picks the slot.

/// One hotbar slot facing one block.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SlotSpeed {
    /// The item's mining speed for the block (1 for a hand or a non-tool).
    pub speed: f32,
    /// The block drops with it: it doesn't need a tool, or this is one.
    pub harvests: bool,
}

/// How much of the block one tick breaks, up to the block's hardness, as
/// vanilla's `getDestroyProgress`: speed / 30 when it harvests, / 100 else.
pub fn progress(slot: SlotSpeed) -> f32 {
    slot.speed / if slot.harvests { 30.0 } else { 100.0 }
}

/// The slot that breaks the block fastest, keeping `current` on a tie so
/// basso doesn't flick between equal items.
pub fn best_slot(slots: &[SlotSpeed], current: usize) -> usize {
    let mut best = current.min(slots.len().saturating_sub(1));
    let mut best_progress = slots.get(best).map_or(0.0, |s| progress(*s));
    for (i, slot) in slots.iter().enumerate() {
        let p = progress(*slot);
        if p > best_progress {
            best = i;
            best_progress = p;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    const HAND: SlotSpeed = SlotSpeed {
        speed: 1.0,
        harvests: false,
    };

    fn tool(speed: f32) -> SlotSpeed {
        SlotSpeed {
            speed,
            harvests: true,
        }
    }

    #[test]
    fn picks_the_fastest() {
        let slots = [HAND, tool(4.0), tool(8.0), HAND];
        assert_eq!(best_slot(&slots, 0), 2);
        assert_eq!(best_slot(&slots, 3), 2);
    }

    #[test]
    fn ties_keep_the_current_slot() {
        let slots = [HAND, HAND, HAND];
        assert_eq!(best_slot(&slots, 1), 1);
        let slots = [tool(6.0), HAND, tool(6.0)];
        assert_eq!(best_slot(&slots, 2), 2);
        assert_eq!(best_slot(&slots, 1), 0);
    }

    #[test]
    fn harvesting_beats_raw_speed() {
        // a fast tool that doesn't harvest loses to a slower one that does
        let slots = [
            SlotSpeed {
                speed: 3.0,
                harvests: false,
            },
            tool(2.0),
        ];
        assert_eq!(best_slot(&slots, 0), 1);
    }

    #[test]
    fn progress_matches_vanilla() {
        assert_eq!(progress(HAND), 0.01);
        assert!((progress(tool(6.0)) - 0.2).abs() < 1e-6);
    }

    #[test]
    fn odd_inputs() {
        assert_eq!(best_slot(&[], 4), 0);
        assert_eq!(best_slot(&[HAND, tool(2.0)], 8), 1);
    }
}
