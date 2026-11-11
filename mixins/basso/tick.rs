// One-line hook helper: the adapter module owns the logic.
fn basso_tick(game: &mut crate::game::Game<'_>) {
    crate::game::basso_adapter::tick(game);
}
