// One-line hook helper: the mouse turns basso's watch camera, not the player.
fn basso_turn_player(xo: f64, yo: f64) -> std::ops::ControlFlow<()> {
    crate::game::basso_adapter::turn_player(xo, yo)
}
