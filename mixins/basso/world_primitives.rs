// One-line hook helper: draws basso's path, search, goal and blocks to break.
fn basso_world_primitives(
    game: &crate::game::Game<'_>,
    primitives: &mut horizon_client::world_primitives::WorldPrimitives,
) {
    crate::game::basso_adapter::world_primitives(game, primitives);
}
