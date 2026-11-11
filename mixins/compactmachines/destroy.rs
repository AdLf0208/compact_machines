// A broken machine forgets its binding (`destroy_block`, API 14).
fn machine_destroy_block(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    state: BlockStateId,
    canceled: &mut bool,
) {
    crate::game::compactmachines_state::on_destroy_block(game, pos, state, canceled);
}
