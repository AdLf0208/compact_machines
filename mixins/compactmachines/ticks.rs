// Both machine blocks tick (`block_entity_ticks`, API 15).
fn machine_block_entity_ticks(block: &str, ticks: &mut bool) {
    crate::game::compactmachines_state::on_block_entity_ticks(block, ticks);
}
