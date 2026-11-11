// A machine's drop is its item with the room on it (`loot_drops`, API 75).
fn machine_loot_drops(
    game: &mut crate::game::Game<'_>,
    context: &NativeLootDrops<'_>,
    drops: &mut Vec<ItemStack>,
) {
    crate::game::compactmachines_state::on_loot_drops(game, context, drops);
}
