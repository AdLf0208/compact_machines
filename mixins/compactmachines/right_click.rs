// A click on a machine without the PSD says what it opens
// (`right_click_block`, API 21).
fn machine_right_click_block(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    face: [i32; 3],
    canceled: &mut bool,
) {
    crate::game::compactmachines_state::on_right_click_block(game, pos, face, canceled);
}
