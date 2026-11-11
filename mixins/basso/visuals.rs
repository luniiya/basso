// One-line hook helper: draws basso's path, goal and blocks to break.
fn basso_visuals(game: &crate::game::Game<'_>, rims: &mut Vec<horizon_client::mod_api::NativeBlockRim>) {
    crate::game::basso_adapter::visuals(game, rims);
}
