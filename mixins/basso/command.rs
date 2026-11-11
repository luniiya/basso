// One-line hook helper: `/basso ...` goes to the adapter, which runs it on
// basso's engine.
fn run_basso_command(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {
    if crate::game::basso_adapter::command(game, command) {
        *ran = true;
    }
}
