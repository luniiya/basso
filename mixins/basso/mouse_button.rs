// One-line hook helper: no clicks in the world while basso plays.
fn basso_mouse_button(game: &crate::game::Game<'_>, canceled: &mut bool) {
    crate::game::basso_adapter::mouse_button(game, canceled);
}
