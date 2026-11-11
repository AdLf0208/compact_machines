// The PSD used in the air inside a room leaves it (`use_item`, API 13).
fn machine_use_item(
    game: &mut crate::game::Game<'_>,
    stack: &horizon_client::items::ItemStack,
    claimed: &mut bool,
) {
    crate::game::compactmachines_state::on_use_item(game, stack, claimed);
}
