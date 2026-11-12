// One-line hook helper: clicks in the world go to basso while it plays or in click mode.
fn basso_mouse_button(game: &crate::game::Game<'_>, button: i32, pressed: bool, canceled: &mut bool) {
    crate::game::basso_adapter::mouse_button(game, button, pressed, canceled);
}
