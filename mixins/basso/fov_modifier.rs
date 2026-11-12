// One-line hook helper: the FOV stays put while basso's watch camera is on.
fn basso_fov_modifier(modifier: &mut f32) {
    crate::game::basso_adapter::fov_modifier(modifier);
}
