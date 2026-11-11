// One-line hook helper: basso walks, and the player's keys don't, while it plays.
fn basso_movement_input(
    game: &crate::game::Game<'_>,
    forward: &mut f32,
    strafe: &mut f32,
    jump: &mut bool,
    sneak: &mut bool,
) {
    crate::game::basso_adapter::movement_input(game, forward, strafe, jump, sneak);
}
