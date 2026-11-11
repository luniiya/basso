// Register the single root command. Horizon gives it the remainder of the
// line unchanged through the `command` hook.
fn register_basso_command(commands: &mut Vec<&'static str>) {
    commands.push("basso");
}
