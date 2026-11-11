// One-line hook helper: the chat box completes basso's arguments.
fn basso_chat_tree(
    value: horizon_client::command::Dispatcher,
) -> horizon_client::command::Dispatcher {
    crate::game::basso_adapter::chat_tree(value)
}
