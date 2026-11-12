// basso's row in Options > Mods.
fn list_basso_mod(mods: &mut Vec<crate::game::native_hooks::NativeMod>) {
    crate::game::basso_adapter::list(mods);
}
