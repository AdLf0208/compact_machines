// Game-side glue for Compact Machines slice 1 (a machine that owns a room).
//
// Injected into `crate::game` as `compactmachines_state`; it needs the game,
// so it is not part of the standalone library (only `runtime.rs` is).
//
// The pin has no dimension API and no player-position write, so a room is a
// sealed `solid_wall` shell in the reserved Overworld volume and the player is
// moved with the deferred console `/tp` from `server_tick` (the pattern
// `waystones` proved). The room registry is a world file, because a block
// entity's own compound cannot be read back from a hook at the pin.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use horizon_client::items::ItemStack;
use horizon_client::text::Text;
use horizon_world::nbt::{Compound, JavaString, Tag};
use horizon_world::registry::BlockStateId;

use crate::game::compactmachines::{
    Binding, Command, Enter, MACHINE_COLOR_COMPONENT, MAX_ENTRY_DEPTH, ROOM_CODE_COMPONENT,
    ROOM_TEMPLATE_COMPONENT, RoomBox, Rng, Store, Template,
};
use crate::game::native_hooks::NativeLootDrops;
use crate::game::native_lifecycle::{ServerTickPhase, WorldPhase};
use crate::game::native_server::{NativeCommand, NativeServerPlayer};

const MACHINE: &str = "compactmachines:machine";
const NEW_MACHINE: &str = "compactmachines:new_machine";
const PSD: &str = "compactmachines:personal_shrinking_device";
const SOLID_WALL: &str = "compactmachines:solid_wall";
const STORE_DIR: &str = "compactmachines";
const STORE_FILE: &str = "rooms.dat";
/// Write at most once a second; a crash loses at most that much.
const SAVE_INTERVAL_TICKS: u64 = 20;
/// `Block.UPDATE_ALL`, what a placed block uses.
const SHELL_FLAGS: i32 = 3;
/// A shell that cannot be placed (chunks never loaded) is retried this long.
const BUILD_RETRY_TICKS: u32 = 600;

/// A teleport queued for the next server tick's read scope.
struct Teleport {
    player: u128,
    dim: String,
    pos: [i32; 3],
    /// A room whose shell to place once the player has arrived.
    build: Option<String>,
}

/// The PSD's cooldown after a shift, in ticks (`RoomHelper` puts a 25-tick
/// cooldown on the device at every entry and every exit).
const PSD_COOLDOWN_TICKS: u64 = 25;

/// A room whose shell still has to be placed.
struct PendingBuild {
    code: String,
    waited: u32,
}

struct Core {
    world: Option<PathBuf>,
    store: Store,
    rng: Rng,
    /// Teleports to run at the next tick's end.
    teleports: Vec<Teleport>,
    /// Shells to place after their player's teleport has loaded the chunks.
    builds: Vec<PendingBuild>,
    /// The room code of a machine that just broke, for its item's drop.
    broken: HashMap<(String, [i32; 3]), Binding>,
    /// When each player's PSD becomes usable again, in ticks.
    cooldown: HashMap<u128, u64>,
    dirty: bool,
    ticks: u64,
    last_save: u64,
}

impl Default for Core {
    fn default() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0x5eed_1234, |elapsed| elapsed.as_nanos() as u64);
        Core {
            world: None,
            store: Store::new(),
            rng: Rng::new(seed),
            teleports: Vec::new(),
            builds: Vec::new(),
            broken: HashMap::new(),
            cooldown: HashMap::new(),
            dirty: false,
            ticks: 0,
            last_save: 0,
        }
    }
}

static CORE: Mutex<Option<Core>> = Mutex::new(None);

fn core() -> std::sync::MutexGuard<'static, Option<Core>> {
    CORE.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn state_name(game: &crate::game::Game<'_>, state: BlockStateId) -> String {
    game.res.facts().get(state).name.to_string()
}

fn block_at(game: &crate::game::Game<'_>, pos: [i32; 3]) -> String {
    state_name(game, game.world.block(pos[0], pos[1], pos[2]))
}

fn store_path(world: &Path) -> PathBuf {
    world.join(STORE_DIR).join(STORE_FILE)
}

fn load_store(world: &Path) -> Store {
    let path = store_path(world);
    let Ok(text) = fs::read_to_string(&path) else {
        return Store::new();
    };
    let (store, warnings) = Store::decode(&text);
    for warning in warnings {
        log::warn!("[compactmachines] {}: {warning}", path.display());
    }
    store
}

fn save_store(world: &Path) {
    let text = {
        let guard = core();
        match guard.as_ref() {
            Some(state) if state.dirty => state.store.encode(),
            _ => return,
        }
    };
    let path = store_path(world);
    let Some(dir) = path.parent() else {
        return;
    };
    if let Err(error) = fs::create_dir_all(dir) {
        log::warn!("[compactmachines] cannot make {}: {error}", dir.display());
        return;
    }
    let tmp = path.with_extension("dat.tmp");
    if let Err(error) = fs::write(&tmp, text) {
        log::warn!("[compactmachines] cannot write {}: {error}", tmp.display());
        return;
    }
    if let Err(error) = fs::rename(&tmp, &path) {
        log::warn!("[compactmachines] cannot replace {}: {error}", path.display());
        return;
    }
    let mut guard = core();
    if let Some(state) = guard.as_mut() {
        state.dirty = false;
    }
}

/// The action's acting player, with where they stand.
fn acting(game: &crate::game::Game<'_>) -> Option<(u128, [i32; 3], String)> {
    let player: NativeServerPlayer = game.native_acting_player()?;
    Some((
        player.uuid,
        [
            player.position[0].floor() as i32,
            player.position[1].floor() as i32,
            player.position[2].floor() as i32,
        ],
        player.dimension,
    ))
}

/// Whether the player's PSD is still cooling down.
fn on_cooldown(player: u128) -> bool {
    let guard = core();
    match guard.as_ref() {
        Some(state) => state
            .cooldown
            .get(&player)
            .is_some_and(|&until| until > state.ticks),
        None => false,
    }
}

/// Starts the device's cooldown: one click cannot both enter and leave.
fn start_cooldown(player: u128) {
    let mut guard = core();
    if let Some(state) = guard.as_mut() {
        state
            .cooldown
            .insert(player, state.ticks + PSD_COOLDOWN_TICKS);
    }
}

/// A machine item's room code, when it carries one.
fn stack_room_code(game: &crate::game::Game<'_>, stack: &ItemStack) -> Option<String> {
    match game.native_component(stack, ROOM_CODE_COMPONENT).ok().flatten()? {
        Tag::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// A machine item's template, when it carries one (`normal` when missing).
fn stack_template(game: &crate::game::Game<'_>, stack: &ItemStack) -> Template {
    game.native_component(stack, ROOM_TEMPLATE_COMPONENT)
        .ok()
        .flatten()
        .and_then(|tag| match tag {
            Tag::String(text) => Template::from_id(&text.to_string_lossy()),
            _ => None,
        })
        .unwrap_or(Template::Normal)
}

fn send_item(game: &mut crate::game::Game<'_>, player: u128, stack: ItemStack) {
    if game.native_give_stack(player, stack).is_some() {
        game.native_tell(
            player,
            Text::tr("commands.machines.compactmachines.cannot_give_machine_item", &[]),
        );
    }
}

// ---------------------------------------------------------------------------
// Block entity (API 15): both machine blocks are block entities
// ---------------------------------------------------------------------------

pub(crate) fn on_block_entity(block: &str, entity: &mut Option<(String, Compound)>) {
    if block == MACHINE || block == NEW_MACHINE {
        *entity = Some((block.to_owned(), Compound::new()));
    }
}

pub(crate) fn on_block_entity_ticks(block: &str, ticks: &mut bool) {
    if block == MACHINE || block == NEW_MACHINE {
        *ticks = true;
    }
}

// ---------------------------------------------------------------------------
// Items and components (API 65, 98)
// ---------------------------------------------------------------------------

pub(crate) fn register_component_types(types: &mut Vec<String>) {
    types.push(ROOM_TEMPLATE_COMPONENT.to_owned());
    types.push(ROOM_CODE_COMPONENT.to_owned());
    types.push(MACHINE_COLOR_COMPONENT.to_owned());
}

/// A machine placed from an item takes the item's template (a `new_machine`)
/// or its room code (a bound `machine`) — `place_block`, API 14.
#[expect(
    private_interfaces,
    reason = "the glue is a crate-private module injected into crate::game"
)]
pub(crate) fn on_place_block(
    game: &mut crate::game::Game<'_>,
    stack: &ItemStack,
    pos: [i32; 3],
    state: BlockStateId,
) {
    let name = state_name(game, state);
    if name != MACHINE && name != NEW_MACHINE {
        return;
    }
    let dimension = game.native_dimension().to_owned();
    let key = (dimension, pos);
    let owner = acting(game).map_or(0, |(uuid, _, _)| uuid);
    let binding = if name == MACHINE {
        let Some(code) = stack_room_code(game, stack) else {
            return;
        };
        Binding::Bound { code }
    } else {
        Binding::Pending {
            template: stack_template(game, stack),
            owner,
        }
    };
    log::info!(
        "[compactmachines] place_block: {} at {pos:?} in {} -> {binding:?}",
        state_name(game, state),
        key.0
    );
    let mut guard = core();
    let state = guard.get_or_insert_with(Core::default);
    state.store.bind_machine(key, binding);
    state.dirty = true;
}

// ---------------------------------------------------------------------------
// The Personal Shrinking Device (API 13, 21)
// ---------------------------------------------------------------------------

/// A PSD on a machine: create its room (an unbound machine) or enter it
/// (`use_item_on`, API 13). The click is claimed so the block does nothing else.
#[expect(
    private_interfaces,
    reason = "the glue is a crate-private module injected into crate::game"
)]
pub(crate) fn on_use_item_on(
    game: &mut crate::game::Game<'_>,
    stack: &ItemStack,
    pos: [i32; 3],
    _face: [i32; 3],
    claimed: &mut bool,
) {
    if &*stack.item != PSD {
        return;
    }
    let Some((player, _, _)) = acting(game) else {
        return;
    };
    if on_cooldown(player) {
        // Still shrinking: swallow the click rather than doing anything.
        *claimed = true;
        return;
    }
    let Some((uuid, player_pos, dimension)) = acting(game) else {
        return;
    };
    let name = block_at(game, pos);
    if name != MACHINE && name != NEW_MACHINE {
        return;
    }
    let key = (dimension.clone(), pos);
    let binding = {
        let guard = core();
        guard
            .as_ref()
            .and_then(|state| state.store.binding(&key).cloned())
    };
    let code = match (name.as_str(), binding) {
        (_, Some(Binding::Bound { code })) => code,
        (NEW_MACHINE, Some(Binding::Pending { template, owner })) => {
            let code = {
                let mut guard = core();
                let state = guard.get_or_insert_with(Core::default);
                let room = state
                    .store
                    .create_room(template, owner, player_pos, &dimension, &mut state.rng);
                let code = room.code.clone();
                state.store.bind_machine(key.clone(), Binding::Bound { code: code.clone() });
                state.dirty = true;
                code
            };
            // An unbound machine becomes the bound one the jar turns it into.
            if let Some(bound) = game.pack.assets.registry.default_state(MACHINE) {
                let _ = game.native_set_block(&dimension, pos, bound, SHELL_FLAGS);
            }
            code
        }
        // A machine with no binding (a machine dropped before the registry
        // knew it, or a legacy item): give it a `normal` room rather than
        // failing the click.
        (_, None) => {
            let mut guard = core();
            let state = guard.get_or_insert_with(Core::default);
            let room = state.store.create_room(
                stack_template(game, stack),
                0,
                player_pos,
                &dimension,
                &mut state.rng,
            );
            let code = room.code.clone();
            state.store.bind_machine(key.clone(), Binding::Bound { code: code.clone() });
            state.dirty = true;
            code
        }
        _ => {
            log::info!("[compactmachines] use_item_on: nothing bound, not a machine");
            return;
        }
    };
    log::info!("[compactmachines] use_item_on: room {code}");
    let room = {
        let guard = core();
        let Some(state) = guard.as_ref() else {
            return;
        };
        state.store.room(&code).cloned()
    };
    let Some(room) = room else {
        return;
    };
    let entered = {
        let mut guard = core();
        let Some(state) = guard.as_mut() else {
            return;
        };
        state.store.enter(
            uuid,
            &code,
            player_pos,
            &room.dim,
            MAX_ENTRY_DEPTH,
        )
    };
    match entered {
        Enter::Entered { spawn } => {
            let build = (!room.built).then(|| code.clone());
            {
                // The guard must die here: `core()` is not reentrant, and
                // `start_cooldown` takes it again below.
                let mut guard = core();
                if let Some(state) = guard.as_mut() {
                    state.teleports.push(Teleport {
                        player: uuid,
                        dim: room.dim.clone(),
                        pos: spawn,
                        build,
                    });
                }
            }
            game.native_action_bar(
                uuid,
                Text::tr("machine.compactmachines.machine.bound_to", &[&code]),
            );
            start_cooldown(uuid);
        }
        Enter::TooDeep => {
            game.native_action_bar(uuid, Text::tr("compactmachines.errors.too_far_down", &[]));
        }
        Enter::NoSuchRoom => {}
    }
    *claimed = true;
}

/// A PSD used in the air inside a room: leave, back to where the player came
/// from (`use_item`, API 13).
#[expect(
    private_interfaces,
    reason = "the glue is a crate-private module injected into crate::game"
)]
pub(crate) fn on_use_item(
    game: &mut crate::game::Game<'_>,
    stack: &ItemStack,
    claimed: &mut bool,
) {
    if &*stack.item != PSD {
        return;
    }
    let Some((uuid, player_pos, dimension)) = acting(game) else {
        return;
    };
    if on_cooldown(uuid) {
        return;
    }
    let inside = {
        let guard = core();
        guard
            .as_ref()
            .is_some_and(|state| state.store.room_at(player_pos, &dimension).is_some())
    };
    if !inside {
        return;
    }
    let left = {
        let mut guard = core();
        let Some(state) = guard.as_mut() else {
            return;
        };
        state.store.leave(uuid)
    };
    let Some(left) = left else {
        return;
    };
    {
        let mut guard = core();
        if let Some(state) = guard.as_mut() {
            state.teleports.push(Teleport {
                player: uuid,
                dim: left.dim,
                pos: left.pos,
                build: None,
            });
        }
    }
    start_cooldown(uuid);
    *claimed = true;
}

/// A click on a machine without the PSD: say what it opens (`right_click_block`,
/// API 21). Not cancelled — the block has no use of its own.
#[expect(
    private_interfaces,
    reason = "the glue is a crate-private module injected into crate::game"
)]
pub(crate) fn on_right_click_block(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    _face: [i32; 3],
    _canceled: &mut bool,
) {
    let name = block_at(game, pos);
    if name != MACHINE && name != NEW_MACHINE {
        return;
    }
    if game
        .held()
        .is_some_and(|held| &*held.item == PSD)
    {
        return;
    }
    let Some((uuid, _, dimension)) = acting(game) else {
        return;
    };
    let binding = {
        let guard = core();
        guard
            .as_ref()
            .and_then(|state| state.store.binding(&(dimension, pos)).cloned())
    };
    let line = match binding {
        Some(Binding::Bound { code }) => {
            let template = {
                let guard = core();
                guard
                    .as_ref()
                    .and_then(|state| state.store.room(&code))
                    .map(|room| room.template)
            };
            match template {
                Some(template) => {
                    let dims = template.dims();
                    Text::tr(
                        "machine.compactmachines.machine.size",
                        &[&format!("{} x {} x {}", dims[0], dims[1], dims[2])],
                    )
                    .append(Text::literal(format!("  {code}")))
                }
                None => Text::tr("rooms.errors.compactmachines.room_not_found", &[&code]),
            }
        }
        Some(Binding::Pending { template, .. }) => {
            let dims = template.dims();
            Text::tr(
                "compactmachines.rooms.templates.room_dimensions",
                &[&format!("{} x {} x {}", dims[0], dims[1], dims[2])],
            )
        }
        None => return,
    };
    game.native_tell(uuid, line);
}

// ---------------------------------------------------------------------------
// Breaking: the item keeps the room (API 14, 75)
// ---------------------------------------------------------------------------

#[expect(
    private_interfaces,
    reason = "the glue is a crate-private module injected into crate::game"
)]
pub(crate) fn on_destroy_block(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    state: BlockStateId,
    canceled: &mut bool,
) {
    if *canceled {
        return;
    }
    let name = state_name(game, state);
    if name != MACHINE && name != NEW_MACHINE {
        return;
    }
    let key = (game.native_dimension().to_owned(), pos);
    let mut guard = core();
    let Some(state) = guard.as_mut() else {
        return;
    };
    if let Some(binding) = state.store.binding(&key).cloned() {
        state.broken.insert(key.clone(), binding);
        if state.broken.len() > 64 {
            state.broken.clear();
        }
    }
    state.store.unbind_machine(&key);
    state.dirty = true;
}

/// The machine's drop is its item with the room/template on it
/// (`loot_drops`, API 75), as the jar's `copy_components` loot function does.
#[expect(
    private_interfaces,
    reason = "the glue is a crate-private module injected into crate::game"
)]
pub(crate) fn on_loot_drops(
    game: &mut crate::game::Game<'_>,
    context: &NativeLootDrops<'_>,
    drops: &mut Vec<ItemStack>,
) {
    let Some((pos, state)) = context.block else {
        return;
    };
    let name = state_name(game, state);
    if name != MACHINE && name != NEW_MACHINE {
        return;
    }
    let key = (context.dimension.to_owned(), pos);
    let binding = {
        let mut guard = core();
        match guard.as_mut() {
            Some(state) => state.broken.remove(&key).or_else(|| {
                state
                    .store
                    .binding(&key)
                    .cloned()
                    .inspect(|_| state.store.unbind_machine(&key))
            }),
            None => None,
        }
    };
    let Some(binding) = binding else {
        return;
    };
    let (item, component, value) = match binding {
        Binding::Bound { code } => (MACHINE, ROOM_CODE_COMPONENT, code),
        Binding::Pending { template, .. } => (
            NEW_MACHINE,
            ROOM_TEMPLATE_COMPONENT,
            format!("compactmachines:{}", template.id()),
        ),
    };
    drops.retain(|drop| &*drop.item != item);
    let mut stack = ItemStack::new(item, 1);
    if game
        .native_set_component(&mut stack, component, Tag::String(JavaString::from(value.as_str())))
        .is_ok()
    {
        drops.push(stack);
    }
}

// ---------------------------------------------------------------------------
// Entering and leaving: deferred, in the tick's read scope (API 3, 4, 21)
// ---------------------------------------------------------------------------

#[expect(
    private_interfaces,
    reason = "the glue is a crate-private module injected into crate::game"
)]
pub(crate) fn on_world_lifecycle(game: &mut crate::game::Game<'_>, phase: WorldPhase) {
    match phase {
        WorldPhase::WorldOpenReady => {
            let world = game.native_world_dir();
            let store = world.as_deref().map(load_store).unwrap_or_default();
            let mut guard = core();
            let state = guard.get_or_insert_with(Core::default);
            state.world = world;
            state.store = store;
            state.teleports.clear();
            state.builds.clear();
            state.broken.clear();
            state.cooldown.clear();
            state.dirty = false;
            state.ticks = 0;
            state.last_save = 0;
        }
        WorldPhase::WorldCloseFinal | WorldPhase::WorldCloseFailed => {
            let world = {
                let guard = core();
                guard.as_ref().and_then(|state| state.world.clone())
            };
            if let Some(world) = world {
                save_store(&world);
            }
            let mut guard = core();
            if let Some(state) = guard.as_mut() {
                state.world = None;
                state.store = Store::new();
                state.teleports.clear();
                state.builds.clear();
                state.broken.clear();
                state.dirty = false;
            }
        }
        WorldPhase::WorldOpenFailed => {
            let mut guard = core();
            if let Some(state) = guard.as_mut() {
                state.world = None;
                state.store = Store::new();
                state.teleports.clear();
                state.builds.clear();
                state.broken.clear();
                state.dirty = false;
            }
        }
    }
}

/// End of a running tick: run the queued teleports, then place the shells the
/// arrival loaded, and write the store at most once a second (API 4).
#[expect(
    private_interfaces,
    reason = "the glue is a crate-private module injected into crate::game"
)]
pub(crate) fn on_server_tick(game: &mut crate::game::Game<'_>, phase: ServerTickPhase) {
    let ServerTickPhase::ServerTickEnd { world_running } = phase else {
        return;
    };
    if !world_running {
        return;
    }
    let (teleports, builds, save) = {
        let mut guard = core();
        let Some(state) = guard.as_mut() else {
            return;
        };
        state.ticks += 1;
        let save = state.dirty && state.ticks.saturating_sub(state.last_save) >= SAVE_INTERVAL_TICKS;
        (
            std::mem::take(&mut state.teleports),
            std::mem::take(&mut state.builds),
            save,
        )
    };
    let mut later = Vec::new();
    for teleport in teleports {
        log::info!(
            "[compactmachines] teleport {} to {:?} in {}",
            teleport.player,
            teleport.pos,
            teleport.dim
        );
        let line = crate::game::compactmachines::teleport_line(
            &teleport.dim,
            teleport.pos,
            teleport.player,
        );
        let ran = game.native_run_command(&line);
        log::info!("[compactmachines] run {line} -> {ran}");
        if !ran {
            // The player left between the click and the tick: drop the entry
            // from their history so a later exit is not one room behind.
            let mut guard = core();
            if let Some(state) = guard.as_mut() {
                let _ = state.store.leave(teleport.player);
            }
            continue;
        }
        if let Some(code) = teleport.build {
            later.push(PendingBuild { code, waited: 0 });
        }
    }
    let wall = game.pack.assets.registry.default_state(SOLID_WALL);
    for mut build in builds {
        build.waited += 1;
        let Some((room_box, room_dim)) = room_site(&build.code) else {
            continue;
        };
        let mut done = true;
        if let (Some(wall), true) = (wall, !room_built(&build.code)) {
            for pos in room_box.shell_blocks() {
                if block_at(game, pos) == SOLID_WALL {
                    continue;
                }
                if !game.native_set_block(&room_dim, pos, wall, SHELL_FLAGS) {
                    done = false;
                }
            }
        }
        if done {
            let mut guard = core();
            if let Some(state) = guard.as_mut() {
                state.store.set_built(&build.code, true);
                state.dirty = true;
            }
            log::info!(
                "[compactmachines] room {} sealed: {} shell blocks",
                build.code,
                room_box.shell_blocks().len()
            );
        } else if build.waited < BUILD_RETRY_TICKS {
            later.push(build);
        } else {
            log::warn!(
                "[compactmachines] room {} could not be sealed: its chunks never loaded",
                build.code
            );
        }
    }
    {
        let mut guard = core();
        if let Some(state) = guard.as_mut() {
            state.builds = later;
            if save {
                state.last_save = state.ticks;
            }
        }
    }
    if save {
        let world = {
            let guard = core();
            guard.as_ref().and_then(|state| state.world.clone())
        };
        if let Some(world) = world {
            save_store(&world);
        }
    }
}

/// A room's geometry and the dimension it stands in.
fn room_site(code: &str) -> Option<(RoomBox, String)> {
    let guard = core();
    let state = guard.as_ref()?;
    let room = state.store.room(code)?;
    Some((state.store.room_box(code)?, room.dim.clone()))
}

fn room_built(code: &str) -> bool {
    let guard = core();
    guard
        .as_ref()
        .and_then(|state| state.store.room(code))
        .is_none_or(|room| room.built)
}

#[expect(
    private_interfaces,
    reason = "the glue is a crate-private module injected into crate::game"
)]
pub(crate) fn on_server_stopping(_game: &mut crate::game::Game<'_>) {
    let world = {
        let guard = core();
        guard.as_ref().and_then(|state| state.world.clone())
    };
    if let Some(world) = world {
        save_store(&world);
    }
}

// ---------------------------------------------------------------------------
// `/compactmachines` (API 8)
// ---------------------------------------------------------------------------

pub(crate) fn on_register_commands(commands: &mut Vec<NativeCommand>) {
    commands.push(NativeCommand::new("compactmachines rooms"));
    commands.push(NativeCommand::new("compactmachines info"));
    commands.push(NativeCommand::op(
        "compactmachines give new <template:word>",
        2,
    ));
    commands.push(NativeCommand::op(
        "compactmachines give existing <code:word>",
        2,
    ));
    commands.push(NativeCommand::op(
        "compactmachines teleport <code:word>",
        2,
    ));
}

#[expect(
    private_interfaces,
    reason = "the glue is a crate-private module injected into crate::game"
)]
pub(crate) fn on_perform_command(
    game: &mut crate::game::Game<'_>,
    player: &NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    let words: Vec<&str> = command.split_whitespace().collect();
    if words.first().copied() != Some("compactmachines") {
        return;
    }
    let parsed = crate::game::compactmachines::parse(&words[1..]);
    if parsed == Command::Unknown {
        return;
    }
    *ran = true;
    match parsed {
        Command::Help => {
            game.native_tell(
                player.uuid,
                Text::tr("commands.rooms.compactmachines.room_reg_count", &[&room_count()]),
            );
        }
        Command::Rooms => {
            let (count, codes) = room_list();
            game.native_tell(
                player.uuid,
                Text::tr(
                    "commands.rooms.compactmachines.room_reg_count",
                    &[&count.to_string()],
                )
                .append(Text::literal(format!(" {codes}"))),
            );
        }
        Command::Info => {
            let pos = [
                player.position[0].floor() as i32,
                player.position[1].floor() as i32,
                player.position[2].floor() as i32,
            ];
            let room = {
                let guard = core();
                guard
                    .as_ref()
                    .and_then(|state| state.store.room_at(pos, &player.dimension))
                    .cloned()
            };
            match room {
                Some(room) => {
                    let dims = room.template.dims();
                    game.native_tell(
                        player.uuid,
                        Text::tr(
                            "machine.compactmachines.machine_room_info",
                            &[
                                &format!("{} {} {}", pos[0], pos[1], pos[2]),
                                &format!("{}x{}x{}", dims[0], dims[1], dims[2]),
                                &room.code,
                            ],
                        ),
                    );
                }
                None => {
                    game.native_tell(
                        player.uuid,
                        Text::tr("compactmachines.commands.not_in_room", &[]),
                    );
                }
            }
        }
        Command::GiveNew { template } => {
            let mut stack = ItemStack::new(NEW_MACHINE, 1);
            let named = format!("compactmachines:{}", template.id());
            if game
                .native_set_component(
                    &mut stack,
                    ROOM_TEMPLATE_COMPONENT,
                    Tag::String(JavaString::from(named.as_str())),
                )
                .is_err()
            {
                game.native_tell(
                    player.uuid,
                    Text::tr("commands.machines.compactmachines.cannot_give_machine_item", &[]),
                );
                return;
            }
            let _ = game.native_set_component(
                &mut stack,
                MACHINE_COLOR_COMPONENT,
                Tag::String(JavaString::from(template.color_hex().as_str())),
            );
            send_item(game, player.uuid, stack);
        }
        Command::GiveExisting { code } => {
            let exists = {
                let guard = core();
                guard
                    .as_ref()
                    .is_some_and(|state| state.store.room(&code).is_some())
            };
            if !exists {
                game.native_tell(
                    player.uuid,
                    Text::tr("rooms.errors.compactmachines.room_not_found", &[&code]),
                );
                return;
            }
            let mut stack = ItemStack::new(MACHINE, 1);
            if game
                .native_set_component(
                    &mut stack,
                    ROOM_CODE_COMPONENT,
                    Tag::String(JavaString::from(code.as_str())),
                )
                .is_err()
            {
                game.native_tell(
                    player.uuid,
                    Text::tr("commands.machines.compactmachines.cannot_give_machine_item", &[]),
                );
                return;
            }
            send_item(game, player.uuid, stack);
        }
        Command::Teleport { code } => {
            let room = {
                let guard = core();
                guard
                    .as_ref()
                    .and_then(|state| state.store.room(&code))
                    .cloned()
            };
            let Some(room) = room else {
                game.native_tell(
                    player.uuid,
                    Text::tr("rooms.errors.compactmachines.room_not_found", &[&code]),
                );
                return;
            };
            let pos = [
                player.position[0].floor() as i32,
                player.position[1].floor() as i32,
                player.position[2].floor() as i32,
            ];
            let entered = {
                let mut guard = core();
                match guard.as_mut() {
                    Some(state) => state.store.enter(
                        player.uuid,
                        &code,
                        pos,
                        &player.dimension,
                        MAX_ENTRY_DEPTH,
                    ),
                    None => return,
                }
            };
            match entered {
                Enter::Entered { spawn } => {
                    let build = (!room.built).then(|| code.clone());
                    {
                        let mut guard = core();
                        if let Some(state) = guard.as_mut() {
                            state.teleports.push(Teleport {
                                player: player.uuid,
                                dim: room.dim.clone(),
                                pos: spawn,
                                build,
                            });
                        }
                    }
                }
                Enter::TooDeep => {
                    game.native_tell(
                        player.uuid,
                        Text::tr("compactmachines.errors.too_far_down", &[]),
                    );
                }
                Enter::NoSuchRoom => {
                    game.native_tell(
                        player.uuid,
                        Text::tr("rooms.errors.compactmachines.room_not_found", &[&code]),
                    );
                }
            }
        }
        Command::Unknown => {}
    }
}

fn room_count() -> usize {
    let guard = core();
    guard.as_ref().map_or(0, |state| state.store.rooms_len())
}

fn room_list() -> (usize, String) {
    let guard = core();
    let Some(state) = guard.as_ref() else {
        return (0, String::new());
    };
    let codes: Vec<String> = state.store.rooms().map(|room| room.code.clone()).collect();
    (codes.len(), codes.join(" "))
}
