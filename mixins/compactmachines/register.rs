// World open: declare the `/compactmachines` usages (`register_commands`, API 8).
fn machine_register_commands(commands: &mut Vec<NativeCommand>) {
    crate::game::compactmachines_state::on_register_commands(commands);
}
