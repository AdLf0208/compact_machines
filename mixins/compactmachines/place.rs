// A machine placed from an item takes the item's template (an unbound
// `new_machine`) or its room (a bound `machine`) (`place_block`, API 14).
fn machine_place_block(
    game: &mut crate::game::Game<'_>,
    stack: &horizon_client::items::ItemStack,
    pos: [i32; 3],
    state: BlockStateId,
) {
    crate::game::compactmachines_state::on_place_block(game, stack, pos, state);
}
