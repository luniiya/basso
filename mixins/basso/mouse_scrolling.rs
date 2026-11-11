// One-line hook helper: the wheel zooms basso's watch camera.
fn basso_mouse_scrolling(accumulated: [i32; 2], canceled: &mut bool) {
    crate::game::basso_adapter::mouse_scrolling(accumulated, canceled);
}
