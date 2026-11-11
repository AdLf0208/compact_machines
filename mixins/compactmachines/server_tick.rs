// End of a running tick: run the queued teleports, place the shells they
// loaded, write the store (`server_tick`, API 4).
fn machine_server_tick(game: &mut crate::game::Game<'_>, phase: ServerTickPhase) {
    crate::game::compactmachines_state::on_server_tick(game, phase);
}
