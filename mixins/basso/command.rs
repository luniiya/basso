/// The first in-game Basso command handler. It proves that Horizon loaded the
/// native bundle and routed `/basso` to it; pathfinder actions are added after
/// Basso's ECS is adapted to Horizon's client world.
fn run_basso_command(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {
    let mut words = command.split_whitespace();
    if words.next() != Some("basso") {
        return;
    }

    let line = match words.next() {
        None | Some("help") => "Basso is loaded. Commands: help, goto, mine, follow, pause, resume, cancel.",
        Some(name) => {
            game.native_chat(&format!("Basso `{name}` is not available yet. Try /basso help."));
            *ran = true;
            return;
        }
    };
    game.native_chat(line);
    *ran = true;
}
