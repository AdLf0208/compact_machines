// The item components a machine carries (`component_types`, API 65): the
// template a `new_machine` opens and the room a bound `machine` opens, plus
// the colour the jar's recipes write.
fn register_component_types(types: &mut Vec<String>) {
    crate::game::compactmachines_state::register_component_types(types);
}
