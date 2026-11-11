// The server is stopping: flush the room registry (`server_stopping`, API 44).
fn machine_server_stopping(game: &mut crate::game::Game<'_>) {
    crate::game::compactmachines_state::on_server_stopping(game);
}
