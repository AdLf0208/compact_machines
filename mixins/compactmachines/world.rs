// The world opened or closed: load or flush the room registry
// (`world_lifecycle`, API 3).
fn machine_world_lifecycle(game: &mut crate::game::Game<'_>, phase: WorldPhase) {
    crate::game::compactmachines_state::on_world_lifecycle(game, phase);
}
