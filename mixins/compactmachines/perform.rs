// A typed `/compactmachines` line (`perform_command`, API 8).
fn machine_perform_command(
    game: &mut crate::game::Game<'_>,
    player: &NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    crate::game::compactmachines_state::on_perform_command(game, player, command, ran);
}
