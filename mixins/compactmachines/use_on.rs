// A Personal Shrinking Device on a machine: create the room (an unbound
// machine) or enter it, and claim the click (`use_item_on`, API 13).
fn machine_use_item_on(
    game: &mut crate::game::Game<'_>,
    stack: &horizon_client::items::ItemStack,
    pos: [i32; 3],
    face: [i32; 3],
    claimed: &mut bool,
) {
    crate::game::compactmachines_state::on_use_item_on(game, stack, pos, face, claimed);
}
