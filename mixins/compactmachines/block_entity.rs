// The machine blocks are block entities (`block_entity`, API 15), so the
// machine is a real machine block and not a plain content block.
fn machine_block_entity(block: &str, entity: &mut Option<(String, horizon_world::nbt::Compound)>) {
    crate::game::compactmachines_state::on_block_entity(block, entity);
}
