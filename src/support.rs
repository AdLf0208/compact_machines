//! Game-side glue for Compact Machines slice 1 (a machine that owns a room).
//!
//! [`crate::runtime`] is the pure truth: the jar's eight room templates, room codes, the
//! 1024-block region spiral, a room's shell geometry and the store of rooms/machines/history with
//! its world file. This module is the only place that reads the game and writes it back: the
//! machine block entity, the two machine items' components, the Personal Shrinking Device's
//! clicks, the deferred teleports, the shell build, the world file and the commands.
//!
//! The room rules are unchanged from the retired `.hnmod` port; only the transport is new. The
//! pin has no dimension API, so a room is still a sealed `solid_wall` shell in the reserved block
//! range of the machine's own level (`ROOM_BASE` + 1024 per region). What changed with the SDK:
//! the deferred teleport is the typed `entity_teleport_to_level` instead of a console `/tp` (so it
//! needs no operator and no command parse), and the room registry loads lazily on the first
//! running server tick because the SDK's `WorldPhase` has no open-ready arm.
//!
//! Diagnostics are the recorded loss: the SDK has no logger a package may reach, so the 0.1.0
//! `log::info!`/`log::warn!` lines are dropped (`notes.md`, migration trap 4).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use mod_api::{
    BlockClick, BlockEntityPart, BlockEntityTickEvent, BlockEntityTickReply, BlockEntityTicksEvent,
    BlockKey, BlockPart, BlockState, BlockUpdateFlags, BreakBlockEvent, Command, CommandArgument,
    CommandArgumentValue, CommandSource, Component, Flow, Host, HostSide, Identifier,
    InteractionResult, ItemKey, ItemPart, ItemStack, LevelRef, LootDropsEvent,
    OnBlockPlacedByEvent, PlayerRef, Registrar, RightClickBlockEvent, RunModCommandEvent,
    ServerHost, ServerTickEvent, ServerTickPhase, Tag, Text, UseItemEvent, UseItemOnEvent,
    WorldLifecycleEvent, WorldPhase,
};

use crate::runtime::{
    Binding, Command as RoomCommand, Enter, MACHINE_COLOR_COMPONENT, MAX_ENTRY_DEPTH,
    ROOM_CODE_COMPONENT, ROOM_TEMPLATE_COMPONENT, Rng, RoomBox, Store, Template,
};

/// The machine block (a machine bound to a room).
const MACHINE: &str = "compactmachines:machine";
/// The machine block a fresh item places (unbound until the device is used).
const NEW_MACHINE: &str = "compactmachines:new_machine";
/// The Personal Shrinking Device.
const PSD: &str = "compactmachines:personal_shrinking_device";
/// The unbreakable shell block a room is built from.
const SOLID_WALL: &str = "compactmachines:solid_wall";
/// The block entity type this mod declares; both machine blocks hold it.
const MACHINE_TYPE: &str = "machine";
/// The room registry, beside the world so a backup carries it.
const STORE_DIR: &str = "compactmachines";
const STORE_FILE: &str = "rooms.dat";
/// Write at most once a second; a crash loses at most that much.
const SAVE_INTERVAL_TICKS: u64 = 20;
/// A shell that cannot be placed (chunks never loaded) is retried this long.
const BUILD_RETRY_TICKS: u32 = 600;
/// How many sites a room tries before it gives up on finding an empty one.
const SITE_TRIES: u32 = 64;
/// The PSD's cooldown after a shift, in ticks (`RoomHelper` puts a 25-tick cooldown on the device
/// at every entry and every exit).
const PSD_COOLDOWN_TICKS: u64 = 25;
/// The 0.1.0 command usages, byte for byte: the e2e's scenarios and the published README name
/// them, so a rename here is a user-visible break.
const USAGE_ROOMS: &str = "compactmachines rooms";
const USAGE_INFO: &str = "compactmachines info";
const USAGE_GIVE_NEW: &str = "compactmachines give new <template:word>";
const USAGE_GIVE_EXISTING: &str = "compactmachines give existing <code:word>";
const USAGE_TELEPORT: &str = "compactmachines teleport <code:word>";

/// Registration keys, so a hot path compares block identity instead of allocating a name.
static MACHINE_KEY: OnceLock<BlockKey> = OnceLock::new();
static NEW_MACHINE_KEY: OnceLock<BlockKey> = OnceLock::new();
static SOLID_WALL_KEY: OnceLock<BlockKey> = OnceLock::new();
static PSD_KEY: OnceLock<ItemKey> = OnceLock::new();

/// `Block.UPDATE_ALL` (`UPDATE_NEIGHBORS | UPDATE_CLIENTS`), what a placed block uses.
fn shell_flags() -> BlockUpdateFlags {
    BlockUpdateFlags::NEIGHBORS | BlockUpdateFlags::CLIENTS
}

// ---------------------------------------------------------------------------
// The three item components a machine carries (`CMDataComponents`)
// ---------------------------------------------------------------------------

/// The item components a machine item carries: the template a `new_machine` opens, the room a
/// bound `machine` opens, and the colour the jar's recipes write.
const ROOM_TEMPLATE: ValueTag = ValueTag(ROOM_TEMPLATE_COMPONENT);
const ROOM_CODE: ValueTag = ValueTag(ROOM_CODE_COMPONENT);
const MACHINE_COLOR: ValueTag = ValueTag(MACHINE_COLOR_COMPONENT);

/// One string-valued component: the SDK reads and writes components through a [`Component`]
/// descriptor, and all three of the jar's carry a plain string.
struct ValueTag(&'static str);

impl Component for ValueTag {
    type Value = Tag;

    fn id(&self) -> &str {
        self.0
    }

    /// A value that is not a string is not this component: the stack reads as an unbound machine
    /// rather than being guessed at.
    fn decode(&self, value: &Tag) -> Option<Tag> {
        matches!(value, Tag::String(_)).then(|| value.clone())
    }

    fn encode(&self, value: Tag) -> Tag {
        value
    }
}

/// The device's declaration, before content loads: the three component ids the machine items use.
pub fn declare_components(types: &mut Vec<String>) {
    types.push(ROOM_TEMPLATE_COMPONENT.to_owned());
    types.push(ROOM_CODE_COMPONENT.to_owned());
    types.push(MACHINE_COLOR_COMPONENT.to_owned());
}

/// A stack's string component, if it carries one.
fn stack_component(stack: &ItemStack, component: &ValueTag) -> Option<String> {
    match stack.get(component)? {
        Tag::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// Write a string component into a stack's component patch; whether the stack took it.
fn write_component(stack: &mut ItemStack, component: &ValueTag, value: &str) -> bool {
    stack.set(component, Tag::String(value.into())).is_ok()
}

/// A machine item's room code, when it carries one.
fn stack_room_code(stack: &ItemStack) -> Option<String> {
    stack_component(stack, &ROOM_CODE)
}

/// A machine item's template, when it carries one (`normal` when missing).
fn stack_template(stack: &ItemStack) -> Template {
    stack_component(stack, &ROOM_TEMPLATE)
        .and_then(|text| Template::from_id(&text))
        .unwrap_or(Template::Normal)
}

// ---------------------------------------------------------------------------
// The port's state
// ---------------------------------------------------------------------------

/// A teleport queued for the next server tick's end.
struct Teleport {
    player: u128,
    dim: String,
    pos: [i32; 3],
    /// A room whose shell to place once the player has arrived.
    build: Option<String>,
}

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
    /// The binding of a machine that just broke, for its item's drop.
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

/// `Hooks: Send + Sync`, so the state is a `Mutex`, never a `RefCell`.
static CORE: Mutex<Option<Core>> = Mutex::new(None);

fn core() -> MutexGuard<'static, Option<Core>> {
    CORE.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn store_path(world: &Path) -> PathBuf {
    world.join(STORE_DIR).join(STORE_FILE)
}

/// Load the room registry from `<world>/compactmachines/rooms.dat`.
fn load_store(world: &Path) -> Store {
    let path = store_path(world);
    let Ok(text) = fs::read_to_string(&path) else {
        return Store::new();
    };
    // The decode's per-line warnings are dropped with the 0.1.0 logger: a malformed line is
    // skipped exactly as before, and the rest of the store still loads (`Store::decode`).
    let (store, _warnings) = Store::decode(&text);
    store
}

/// Write the store when it changed, atomically (`.tmp` then rename).
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
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = path.with_extension("dat.tmp");
    if fs::write(&tmp, text).is_err() || fs::rename(&tmp, &path).is_err() {
        return;
    }
    let mut guard = core();
    if let Some(state) = guard.as_mut() {
        state.dirty = false;
    }
}

/// Load the world's registry on the first running tick that needs it.
///
/// The 0.1.0 glue loaded at `WorldPhase::WorldOpenReady`; the SDK's `WorldPhase` carries only the
/// failure and close phases, so the load is lazy (the enderstorage pattern).
fn ensure_world(host: &mut dyn ServerHost) {
    let loaded = {
        let guard = core();
        guard.as_ref().is_some_and(|state| state.world.is_some())
    };
    if loaded {
        return;
    }
    let Some(dir) = host.world_dir() else {
        return;
    };
    let mut guard = core();
    let state = guard.get_or_insert_with(Core::default);
    if state.world.as_deref() == Some(dir.as_path()) {
        return;
    }
    state.store = load_store(&dir);
    state.teleports.clear();
    state.builds.clear();
    state.broken.clear();
    state.cooldown.clear();
    state.dirty = false;
    state.ticks = 0;
    state.last_save = 0;
    state.world = Some(dir);
}

/// Forget the open world, saving it first when asked.
fn close_world(save: bool) {
    let world = {
        let guard = core();
        guard.as_ref().and_then(|state| state.world.clone())
    };
    if save && let Some(world) = world {
        save_store(&world);
    }
    let mut guard = core();
    if let Some(state) = guard.as_mut() {
        state.world = None;
        state.store = Store::new();
        state.teleports.clear();
        state.builds.clear();
        state.broken.clear();
        state.cooldown.clear();
        state.dirty = false;
    }
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

/// Where a player stands, floored to the block they occupy.
fn player_pos(player: PlayerRef, host: &dyn Host) -> Option<[i32; 3]> {
    let pos = player.entity().position(host)?;
    Some([
        pos[0].floor() as i32,
        pos[1].floor() as i32,
        pos[2].floor() as i32,
    ])
}

// ---------------------------------------------------------------------------
// Blocks
// ---------------------------------------------------------------------------

/// Whether `block` is one of the two machine blocks.
fn is_machine_block(block: BlockKey) -> bool {
    MACHINE_KEY.get() == Some(&block) || NEW_MACHINE_KEY.get() == Some(&block)
}

/// Whether `block` is the unbound machine block.
fn is_new_machine(block: BlockKey) -> bool {
    NEW_MACHINE_KEY.get() == Some(&block)
}

/// Whether a state is one of the two machine blocks.
fn is_machine_state(state: BlockState) -> bool {
    is_machine_block(state.block())
}

/// The block at `pos`, if the level is loaded there.
fn block_at(host: &dyn Host, level: LevelRef, pos: [i32; 3]) -> Option<BlockKey> {
    level.block(host, pos)
}

/// The three air blocks' keys, resolved once per site check.
fn air_keys(host: &dyn Host) -> Vec<BlockKey> {
    ["minecraft:air", "minecraft:cave_air", "minecraft:void_air"]
        .into_iter()
        .filter_map(|id| host.block_key(id))
        .collect()
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Declare what the game answers for this mod's blocks and items: the machine block entity type
/// (both machine blocks), the Part that keeps it ticking like the jar's own, and the three item
/// Parts (the two machine items' placement and the device's use).
pub fn declare(r: &mut dyn Registrar) {
    let machine = r.block_key(MACHINE);
    let new_machine = r.block_key(NEW_MACHINE);
    let wall = r.block_key(SOLID_WALL);
    let _ = MACHINE_KEY.set(machine);
    let _ = NEW_MACHINE_KEY.set(new_machine);
    let _ = SOLID_WALL_KEY.set(wall);

    // `block_entity` + `block_entity_ticks` (API 15) are one declaration now: the type, the
    // blocks that hold it, and the Part attached to it.
    let kind = r
        .block_entity_type(MACHINE_TYPE)
        .blocks(&[MACHINE, NEW_MACHINE])
        .build();
    let _ = r.block_entity_part(&kind, MachinePart);

    // The two machine items take their template/room from the item they were placed from
    // (`place_block`, API 14), and the device is clicked on a machine and used in the air
    // (`use_item_on`/`use_item`, API 13). Bind each key first: `item_part(item_key(id), part)`
    // is a double `&mut`.
    let machine_item = r.item_key(MACHINE);
    let new_machine_item = r.item_key(NEW_MACHINE);
    let psd_item = r.item_key(PSD);
    let _ = PSD_KEY.set(psd_item);
    r.item_part(machine_item, Placed);
    r.item_part(new_machine_item, Placed);
    r.item_part(psd_item, ShrinkingDevice);
    r.block_part(machine, MachineClick);
    r.block_part(new_machine, MachineClick);
}

// ---------------------------------------------------------------------------
// The machine block entity (API 15)
// ---------------------------------------------------------------------------

/// The machine's block entity. The room itself is not stored here (the world file is the truth),
/// so the Part's only jobs are the ones the jar's `EntityBlock` does: it exists, and it ticks.
struct MachinePart;

#[mod_api::horizon_part]
impl BlockPart for MachinePart {}

#[mod_api::horizon_part]
impl BlockEntityPart for MachinePart {
    /// `EntityBlock.getTicker`: the machine ticks. A ticked block entity is out of the world
    /// during its own tick, and this slice's machine has no per-tick work (the deferred teleports
    /// and the shell build run from `server_tick`), so the tick answers `Unchanged` instead of
    /// dirtying its chunk every tick.
    fn block_entity_ticks(&self, _host: &dyn Host, event: &mut BlockEntityTicksEvent<'_>) {
        event.ticks = true;
    }

    fn block_entity_tick(&self, _host: &mut dyn ServerHost, event: &mut BlockEntityTickEvent<'_>) {
        *event.reply = BlockEntityTickReply::Unchanged;
    }
}

// ---------------------------------------------------------------------------
// Placing a machine (API 14)
// ---------------------------------------------------------------------------

/// A machine placed from an item takes the item's template (an unbound `new_machine`) or its
/// room code (a bound `machine`).
struct Placed;

#[mod_api::horizon_part]
impl ItemPart for Placed {
    fn on_block_placed_by(&self, host: &mut dyn ServerHost, event: &mut OnBlockPlacedByEvent<'_>) {
        place_machine(host, event);
    }
}

fn place_machine(host: &mut dyn ServerHost, event: &mut OnBlockPlacedByEvent<'_>) {
    if !is_machine_state(event.state) {
        return;
    }
    ensure_world(host);
    let dimension = event.placer.map_or_else(
        || host.dimension().to_owned(),
        |placer| {
            placer
                .dimension(&*host)
                .map_or_else(|| host.dimension().to_owned(), |id| id.to_string())
        },
    );
    let key = (dimension, event.pos);
    let owner = event.placer.map_or(0, |placer| placer.uuid());
    let binding = if is_new_machine(event.state.block()) {
        Binding::Pending {
            template: stack_template(event.stack),
            owner,
        }
    } else {
        // A bound machine item whose component is missing would place a machine with no room;
        // the 0.1.0 glue left it unbound and the device then gave it a `normal` room.
        let Some(code) = stack_room_code(event.stack) else {
            return;
        };
        Binding::Bound { code }
    };
    let mut guard = core();
    let state = guard.get_or_insert_with(Core::default);
    state.store.bind_machine(key, binding);
    state.dirty = true;
}

// ---------------------------------------------------------------------------
// The Personal Shrinking Device (API 13, 21)
// ---------------------------------------------------------------------------

/// The device's item Part: used in the air inside a room, it leaves for the last place the
/// player came from.
struct ShrinkingDevice;

#[mod_api::horizon_part]
impl ItemPart for ShrinkingDevice {
    fn use_item(
        &self,
        host: HostSide<'_>,
        event: &mut UseItemEvent<'_>,
    ) -> Option<InteractionResult> {
        let HostSide::Server(host) = host else {
            // The client predicts the use; only the server moves anyone.
            return None;
        };
        leave_room(host, event.player)
    }
}

/// A PSD used in the air inside a room: leave, back to where the player came from.
fn leave_room(host: &mut dyn ServerHost, uuid: u128) -> Option<InteractionResult> {
    ensure_world(host);
    let player = host.player(uuid)?;
    let level = player.level(&*host)?;
    let dimension = level.dimension_id().to_owned();
    let pos = player_pos(player, &*host)?;
    if on_cooldown(uuid) {
        return None;
    }
    let inside = {
        let guard = core();
        guard
            .as_ref()
            .is_some_and(|state| state.store.room_at(pos, &dimension).is_some())
    };
    if !inside {
        return None;
    }
    let left = {
        let mut guard = core();
        let state = guard.as_mut()?;
        state.store.leave(uuid)
    }?;
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
    Some(InteractionResult::Success)
}

/// The machine's block Part: a device click on a machine creates its room (an unbound machine) or
/// enters it.
struct MachineClick;

#[mod_api::horizon_part]
impl BlockPart for MachineClick {
    fn use_item_on(
        &self,
        host: &mut dyn ServerHost,
        event: &mut UseItemOnEvent<'_>,
    ) -> Option<InteractionResult> {
        machine_click(host, event.click)
    }
}

/// One device click on a machine: create its room, or enter it. `Some(Success)` claims the click
/// so the block does nothing else, as the 0.1.0 hook's `*claimed = true` did.
fn machine_click(host: &mut dyn ServerHost, click: &BlockClick<'_>) -> Option<InteractionResult> {
    let stack = click.stack?;
    let psd = PSD_KEY.get()?;
    if stack.item() != *psd {
        return None;
    }
    ensure_world(host);
    let dimension = click.dimension.key().id().to_owned();
    let player = host.player(click.player)?;
    let player_pos = player_pos(player, &*host)?;
    if on_cooldown(click.player) {
        // Still shrinking: swallow the click rather than doing anything.
        return Some(InteractionResult::Success);
    }
    let level = host.level(&dimension)?;
    let block = block_at(&*host, level, click.pos)?;
    if !is_machine_block(block) {
        return None;
    }
    let key = (dimension.clone(), click.pos);
    let binding = {
        let guard = core();
        guard
            .as_ref()
            .and_then(|state| state.store.binding(&key).cloned())
    };
    let code = match (is_new_machine(block), binding) {
        (_, Some(Binding::Bound { code })) => code,
        // A bound machine block whose store entry says "pending" cannot happen (the block is
        // turned into `machine` in the same call that binds it); refuse rather than guess.
        (false, Some(Binding::Pending { .. })) => return None,
        (true, Some(Binding::Pending { template, owner })) => {
            let code = {
                let mut guard = core();
                let state = guard.get_or_insert_with(Core::default);
                let room = state.store.create_room(
                    template,
                    owner,
                    player_pos,
                    &dimension,
                    &mut state.rng,
                );
                let code = room.code.clone();
                state
                    .store
                    .bind_machine(key.clone(), Binding::Bound { code: code.clone() });
                state.dirty = true;
                code
            };
            // An unbound machine becomes the bound one the jar turns it into.
            if let Some(machine) = MACHINE_KEY.get().copied()
                && let Some(machine_state) = host.block_state_default(machine)
            {
                let _ = level.set_block(host, click.pos, machine_state, shell_flags());
            }
            code
        }
        // A machine with no binding (a machine dropped before the registry knew it, or a legacy
        // item): give it a `normal` room rather than failing the click.
        (_, None) => {
            let mut guard = core();
            let state = guard.get_or_insert_with(Core::default);
            let room = state.store.create_room(
                stack_template(stack),
                0,
                player_pos,
                &dimension,
                &mut state.rng,
            );
            let code = room.code.clone();
            state
                .store
                .bind_machine(key.clone(), Binding::Bound { code: code.clone() });
            state.dirty = true;
            code
        }
    };
    let room = {
        let guard = core();
        guard
            .as_ref()
            .and_then(|state| state.store.room(&code).cloned())
    }?;
    let entered = {
        let mut guard = core();
        let state = guard.as_mut()?;
        state
            .store
            .enter(click.player, &code, player_pos, &room.dim, MAX_ENTRY_DEPTH)
    };
    match entered {
        Enter::Entered { spawn } => {
            let build = (!room.built).then(|| code.clone());
            {
                let mut guard = core();
                if let Some(state) = guard.as_mut() {
                    state.teleports.push(Teleport {
                        player: click.player,
                        dim: room.dim.clone(),
                        pos: spawn,
                        build,
                    });
                }
            }
            let _ = player.action_bar(
                host,
                tr("machine.compactmachines.machine.bound_to", &[&code]),
            );
            start_cooldown(click.player);
        }
        Enter::TooDeep => {
            let _ = player.action_bar(host, tr("compactmachines.errors.too_far_down", &[]));
        }
        Enter::NoSuchRoom => {}
    }
    Some(InteractionResult::Success)
}

/// A click on a machine without the device: say what the room is (`right_click_block`, API 21).
/// Not cancelled — the block has no use of its own, so the click is only heard.
pub fn right_click(host: HostSide<'_>, event: &mut RightClickBlockEvent<'_>) -> Flow {
    let HostSide::Server(host) = host else {
        return Flow::Continue;
    };
    let click = event.click;
    let Some(player) = host.player(click.player) else {
        return Flow::Continue;
    };
    ensure_world(host);
    let dimension = click.dimension.key().id();
    let Some(level) = host.level(dimension) else {
        return Flow::Continue;
    };
    if !block_at(&*host, level, click.pos).is_some_and(is_machine_block) {
        return Flow::Continue;
    }
    if click
        .stack
        .is_some_and(|stack| PSD_KEY.get() == Some(&stack.item()))
    {
        // The device's own click is handled by `use_item_on`.
        return Flow::Continue;
    }
    let key = (dimension.to_owned(), click.pos);
    let binding = {
        let guard = core();
        guard
            .as_ref()
            .and_then(|state| state.store.binding(&key).cloned())
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
                Some(template) => size_line("machine.compactmachines.machine.size", template)
                    .append(Text::literal(format!("  {code}"))),
                None => tr("rooms.errors.compactmachines.room_not_found", &[&code]),
            }
        }
        Some(Binding::Pending { template, .. }) => {
            size_line("compactmachines.rooms.templates.room_dimensions", template)
        }
        None => return Flow::Continue,
    };
    let _ = player.tell(host, line);
    Flow::Continue
}

/// `Internal Size: <w> x <h> x <d>`, as the 0.1.0 glue built it.
fn size_line(key: &str, template: Template) -> Text {
    let dims = template.dims();
    tr(key, &[&format!("{} x {} x {}", dims[0], dims[1], dims[2])])
}

// ---------------------------------------------------------------------------
// Breaking a machine (API 14), and its drop (API 75)
// ---------------------------------------------------------------------------

/// A player broke a machine: forget the binding and remember it for the drop.
pub fn break_block(host: &mut dyn ServerHost, event: &mut BreakBlockEvent<'_>) -> Flow {
    if event.cancelled || !is_machine_state(event.state) {
        return Flow::Continue;
    }
    ensure_world(host);
    let key = (host.dimension().to_owned(), event.pos);
    let mut guard = core();
    if let Some(state) = guard.as_mut() {
        if let Some(binding) = state.store.binding(&key).cloned() {
            state.broken.insert(key.clone(), binding);
            if state.broken.len() > 64 {
                state.broken.clear();
            }
        }
        state.store.unbind_machine(&key);
        state.dirty = true;
    }
    Flow::Continue
}

/// The machine's drop is its item with the room (or the template) on it, as the jar's
/// `copy_components` loot function does.
pub fn loot_drops(host: &mut dyn ServerHost, event: &mut LootDropsEvent<'_>) {
    let Some((pos, state)) = event.context.block else {
        return;
    };
    if !is_machine_state(state) {
        return;
    }
    ensure_world(host);
    let key = (event.context.dimension.to_owned(), pos);
    let binding = {
        let mut guard = core();
        match guard.as_mut() {
            Some(core) => core.broken.remove(&key).or_else(|| {
                core.store
                    .binding(&key)
                    .cloned()
                    .inspect(|_| core.store.unbind_machine(&key))
            }),
            None => None,
        }
    };
    let Some(binding) = binding else {
        return;
    };
    let (item, component, value) = match &binding {
        Binding::Bound { code } => (MACHINE, &ROOM_CODE, code.clone()),
        Binding::Pending { template, .. } => (
            NEW_MACHINE,
            &ROOM_TEMPLATE,
            format!("compactmachines:{}", template.id()),
        ),
    };
    let Ok(id) = Identifier::parse(item) else {
        return;
    };
    let Some(item_key) = host.item_key(&id) else {
        return;
    };
    let name = item_key.id(&*host);
    let mut stack = ItemStack::of(item_key);
    if !write_component(&mut stack, component, &value) {
        // Without the component the drop would be a machine with no room; leave the content's
        // own drop in place rather than removing it for nothing.
        return;
    }
    event.drops.retain(|drop| drop.item().id(&*host) != name);
    event.drops.push(stack);
}

// ---------------------------------------------------------------------------
// The world's lifecycle and the server tick
// ---------------------------------------------------------------------------

/// The world closed: flush the registry, then forget it. (An open needs no arm: the registry
/// loads on the first running tick.)
pub fn world_lifecycle(_host: &mut dyn ServerHost, event: &mut WorldLifecycleEvent<'_>) {
    match event.phase {
        WorldPhase::WorldCloseFinal | WorldPhase::WorldCloseFailed => close_world(true),
        WorldPhase::WorldOpenFailed => close_world(false),
        // `WorldPhase` is non-exhaustive: a phase this pin adds later needs no handling here.
        _ => {}
    }
}

/// The server is stopping: flush the registry.
pub fn server_stopping(_host: &mut dyn ServerHost) {
    close_world(true);
}

/// End of a running tick: run the queued teleports, then place the shells their arrival loaded,
/// and write the store at most once a second.
pub fn server_tick(host: &mut dyn ServerHost, event: &mut ServerTickEvent<'_>) {
    let ServerTickPhase::ServerTickEnd { world_running } = event.phase else {
        return;
    };
    if !world_running {
        return;
    }
    ensure_world(host);
    let (teleports, builds, save) = {
        let mut guard = core();
        let Some(state) = guard.as_mut() else {
            return;
        };
        state.ticks += 1;
        let save =
            state.dirty && state.ticks.saturating_sub(state.last_save) >= SAVE_INTERVAL_TICKS;
        (
            std::mem::take(&mut state.teleports),
            std::mem::take(&mut state.builds),
            save,
        )
    };
    let mut later = Vec::new();
    for teleport in teleports {
        if !teleport_player(host, &teleport) {
            // The player left between the click and the tick: drop the entry from their history
            // so a later exit is not one room behind.
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
    for mut build in builds {
        build.waited += 1;
        let Some((room_box, room_dim)) = room_site(&build.code) else {
            continue;
        };
        let mut done = true;
        if !room_built(&build.code) {
            let Some(level) = host.level(&room_dim) else {
                // The level is not open yet: try again next tick (a room's level is the
                // machine's, so it exists as soon as the world is up).
                if build.waited < BUILD_RETRY_TICKS {
                    later.push(build);
                }
                continue;
            };
            // A room is built by writing this shell into the level's reserved range, so the site
            // must be empty first: writing over a player's build would destroy it, and a
            // non-empty interior would enclose the player the machine teleports in. A site that
            // is not free is abandoned and the room moves to the next one; the shell left by an
            // earlier room of this port is fine to write over, but anything inside the interior
            // is not.
            if blocked_site(&*host, level, &room_box).is_some() {
                let moved = {
                    let mut guard = core();
                    guard
                        .as_mut()
                        .is_some_and(|state| state.store.relocate(&build.code).is_some())
                };
                if moved && build.waited < SITE_TRIES {
                    later.push(build);
                }
                continue;
            }
            let Some(wall) = SOLID_WALL_KEY.get().copied() else {
                continue;
            };
            let Some(wall_state) = host.block_state_default(wall) else {
                continue;
            };
            for pos in room_box.shell_blocks() {
                if block_at(&*host, level, pos) == Some(wall) {
                    continue;
                }
                if !matches!(
                    level.set_block(host, pos, wall_state, shell_flags()),
                    Ok(true)
                ) {
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
        } else if build.waited < BUILD_RETRY_TICKS {
            later.push(build);
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

/// Move a player to a queued destination: `Entity.teleport` with a portal's own semantics, at
/// the block centre (x + 0.5, feet at y, z + 0.5) the 0.1.0 `/tp` line used.
fn teleport_player(host: &mut dyn ServerHost, teleport: &Teleport) -> bool {
    let Some(player) = host.player(teleport.player) else {
        return false;
    };
    let pos = [
        f64::from(teleport.pos[0]) + 0.5,
        f64::from(teleport.pos[1]),
        f64::from(teleport.pos[2]) + 0.5,
    ];
    player
        .entity()
        .teleport_to_level(host, &teleport.dim, pos, None)
}

/// A room's geometry and the level it stands in.
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

/// The first block of a room's inflated box that is not free for it: anything that is not air
/// (the interior) or air/`solid_wall` (the shell). The 0.1.0 glue logged the block; the logger is
/// gone, so the caller only learns *that* the site was taken.
fn blocked_site(host: &dyn Host, level: LevelRef, room_box: &RoomBox) -> Option<[i32; 3]> {
    let wall = SOLID_WALL_KEY.get().copied();
    let air = air_keys(host);
    room_box.volume_blocks().into_iter().find(|pos| {
        let Some(block) = block_at(host, level, *pos) else {
            // An unloaded position reads as no block: it is not judged here, and the shell
            // write below is what fails and retries the build next tick.
            return false;
        };
        if room_box.interior_contains(*pos) {
            // The interior must be air; a previous room's shell there would enclose the player.
            !air.contains(&block)
        } else {
            // The shell may be air or the shell block an earlier room of this port left.
            !air.contains(&block) && Some(block) != wall
        }
    })
}

// ---------------------------------------------------------------------------
// `/compactmachines` (API 8)
// ---------------------------------------------------------------------------

/// `/compactmachines rooms|info|give new|give existing|teleport`.
pub fn declare_commands(commands: &mut Vec<Command>) {
    commands.push(Command::new(USAGE_ROOMS));
    commands.push(Command::new(USAGE_INFO));
    commands.push(Command::op(USAGE_GIVE_NEW, 2));
    commands.push(Command::op(USAGE_GIVE_EXISTING, 2));
    commands.push(Command::op(USAGE_TELEPORT, 2));
}

/// The words the 0.1.0 grammar parsed, rebuilt from the typed invocation so
/// [`crate::runtime::parse`] stays the one grammar (and keeps its tests).
fn words_of(literals: &[String], arguments: &[CommandArgument]) -> Vec<String> {
    let mut words = literals.to_vec();
    for argument in arguments {
        match &argument.value {
            CommandArgumentValue::String(text) => words.push(text.clone()),
            CommandArgumentValue::Integer(value) => words.push(value.to_string()),
            CommandArgumentValue::Number(value) => words.push(value.to_string()),
            CommandArgumentValue::Boolean(value) => words.push(value.to_string()),
            _ => {}
        }
    }
    words
}

/// Run one of the commands above and take it.
pub fn run_command(host: &mut dyn ServerHost, event: &mut RunModCommandEvent<'_>) -> Flow {
    let invocation = event.command;
    let line = words_of(&invocation.literals, &invocation.arguments);
    if line.first().map(String::as_str) != Some("compactmachines") {
        return Flow::Continue;
    }
    let args: Vec<&str> = line.iter().skip(1).map(String::as_str).collect();
    let parsed = crate::runtime::parse(&args);
    if parsed == RoomCommand::Unknown {
        return Flow::Continue;
    }
    ensure_world(host);
    let player = match event.source {
        CommandSource::Player { player, .. } => Some(*player),
        _ => None,
    };
    match parsed {
        RoomCommand::Help => reply(
            host,
            player,
            tr(
                "commands.rooms.compactmachines.room_reg_count",
                &[&room_count().to_string()],
            ),
        ),
        RoomCommand::Rooms => {
            let (count, codes) = room_list();
            reply(
                host,
                player,
                tr(
                    "commands.rooms.compactmachines.room_reg_count",
                    &[&count.to_string()],
                )
                .append(Text::literal(format!(" {codes}"))),
            );
        }
        RoomCommand::Info => {
            let pos = player
                .and_then(|player| player_pos(player, &*host))
                .unwrap_or_else(|| {
                    [
                        invocation.position[0].floor() as i32,
                        invocation.position[1].floor() as i32,
                        invocation.position[2].floor() as i32,
                    ]
                });
            let dimension = invocation.dimension.id(&*host).unwrap_or_default();
            let room = {
                let guard = core();
                guard
                    .as_ref()
                    .and_then(|state| state.store.room_at(pos, &dimension))
                    .cloned()
            };
            let text = match room {
                Some(room) => {
                    let dims = room.template.dims();
                    tr(
                        "machine.compactmachines.machine_room_info",
                        &[
                            &format!("{} {} {}", pos[0], pos[1], pos[2]),
                            &format!("{}x{}x{}", dims[0], dims[1], dims[2]),
                            &room.code,
                        ],
                    )
                }
                None => tr("compactmachines.commands.not_in_room", &[]),
            };
            reply(host, player, text);
        }
        RoomCommand::GiveNew { template } => {
            let Some(key) = item_key(host, NEW_MACHINE) else {
                reply(host, player, cannot_give());
                return Flow::Cancel;
            };
            let mut stack = ItemStack::of(key);
            let named = format!("compactmachines:{}", template.id());
            if !write_component(&mut stack, &ROOM_TEMPLATE, &named) {
                reply(host, player, cannot_give());
                return Flow::Cancel;
            }
            let _ = write_component(&mut stack, &MACHINE_COLOR, &template.color_hex());
            give(host, player, stack);
        }
        RoomCommand::GiveExisting { code } => {
            let exists = {
                let guard = core();
                guard
                    .as_ref()
                    .is_some_and(|state| state.store.room(&code).is_some())
            };
            if !exists {
                reply(
                    host,
                    player,
                    tr("rooms.errors.compactmachines.room_not_found", &[&code]),
                );
                return Flow::Cancel;
            }
            let Some(key) = item_key(host, MACHINE) else {
                reply(host, player, cannot_give());
                return Flow::Cancel;
            };
            let mut stack = ItemStack::of(key);
            if !write_component(&mut stack, &ROOM_CODE, &code) {
                reply(host, player, cannot_give());
                return Flow::Cancel;
            }
            give(host, player, stack);
        }
        RoomCommand::Teleport { code } => {
            let room = {
                let guard = core();
                guard
                    .as_ref()
                    .and_then(|state| state.store.room(&code))
                    .cloned()
            };
            let Some(room) = room else {
                reply(
                    host,
                    player,
                    tr("rooms.errors.compactmachines.room_not_found", &[&code]),
                );
                return Flow::Cancel;
            };
            let Some(player) = player else {
                // A console cannot be put in a room.
                reply(
                    host,
                    None,
                    tr("rooms.errors.compactmachines.room_not_found", &[&code]),
                );
                return Flow::Cancel;
            };
            let Some(pos) = player_pos(player, &*host) else {
                return Flow::Cancel;
            };
            let dimension = player
                .dimension(&*host)
                .map_or_else(String::new, |id| id.to_string());
            let entered = {
                let mut guard = core();
                match guard.as_mut() {
                    Some(state) => {
                        state
                            .store
                            .enter(player.uuid(), &code, pos, &dimension, MAX_ENTRY_DEPTH)
                    }
                    None => return Flow::Cancel,
                }
            };
            match entered {
                Enter::Entered { spawn } => {
                    let build = (!room.built).then(|| code.clone());
                    let mut guard = core();
                    if let Some(state) = guard.as_mut() {
                        state.teleports.push(Teleport {
                            player: player.uuid(),
                            dim: room.dim.clone(),
                            pos: spawn,
                            build,
                        });
                    }
                }
                Enter::TooDeep => {
                    reply(
                        host,
                        Some(player),
                        tr("compactmachines.errors.too_far_down", &[]),
                    );
                }
                Enter::NoSuchRoom => {
                    reply(
                        host,
                        Some(player),
                        tr("rooms.errors.compactmachines.room_not_found", &[&code]),
                    );
                }
            }
        }
        RoomCommand::Unknown => {}
    }
    Flow::Cancel
}

fn cannot_give() -> Text {
    tr(
        "commands.machines.compactmachines.cannot_give_machine_item",
        &[],
    )
}

/// The item key of a namespaced id, if the world has it.
fn item_key(host: &dyn Host, id: &str) -> Option<ItemKey> {
    let id = Identifier::parse(id).ok()?;
    host.item_key(&id)
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

/// Give a built stack to the player, or say why not (`/give`'s own path).
fn give(host: &mut dyn ServerHost, player: Option<PlayerRef>, stack: ItemStack) {
    let given = player.is_some_and(|player| player.give_stack(host, stack).is_ok());
    if !given {
        reply(host, player, cannot_give());
    }
}

/// A command's answer: the player's chat when a player ran it, else command feedback.
fn reply(host: &mut dyn ServerHost, player: Option<PlayerRef>, line: Text) {
    match player {
        Some(player) => {
            let _ = player.tell(host, line);
        }
        None => {
            let _ = host.commands().feedback(line);
        }
    }
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

/// A translatable line with literal arguments, as the 0.1.0 glue's `Text::tr` built it.
fn tr(key: &str, args: &[&str]) -> Text {
    Text::translate(key, args.iter().map(|arg| Text::literal(*arg)).collect())
}

// ---------------------------------------------------------------------------
// Tests of the glue's pure parts
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The three descriptors name the jar's components, and each reads only a string: a stack
    /// whose component is an int must not be mistaken for a bound machine.
    #[test]
    fn the_component_descriptors_accept_strings_only() {
        for (component, id) in [
            (&ROOM_TEMPLATE, "compactmachines:room_template"),
            (&ROOM_CODE, "compactmachines:room_code"),
            (&MACHINE_COLOR, "compactmachines:machine_color"),
        ] {
            assert_eq!(component.id(), id);
            let text = Tag::String("compactmachines:tiny".into());
            assert_eq!(component.decode(&text), Some(text));
            assert_eq!(component.decode(&Tag::Int(7)), None);
        }
    }

    /// Trap 17 of the migration: the five usages are user-visible (the README and the 0.1.0 e2e
    /// name them), so a rename is a break, not a tidy-up.
    #[test]
    fn the_command_usages_are_the_0_1_0_ones() {
        let mut commands = Vec::new();
        declare_commands(&mut commands);
        let usages: Vec<(&str, u8)> = commands
            .iter()
            .map(|command| (command.usage.as_str(), command.level))
            .collect();
        assert_eq!(
            usages,
            vec![
                ("compactmachines rooms", 0),
                ("compactmachines info", 0),
                ("compactmachines give new <template:word>", 2),
                ("compactmachines give existing <code:word>", 2),
                ("compactmachines teleport <code:word>", 2),
            ]
        );
    }

    /// The registry travels beside the world, under the name the 0.1.0 e2e greps
    /// (`saves/<world>/compactmachines/rooms.dat`).
    #[test]
    fn the_store_sits_beside_the_world() {
        let path = store_path(Path::new("/saves/TheWorld"));
        assert_eq!(path, Path::new("/saves/TheWorld/compactmachines/rooms.dat"));
    }

    /// The typed command invocation rebuilds the words the runtime's grammar parses, with the
    /// literal path first and the arguments in parse order.
    #[test]
    fn the_words_of_an_invocation_keep_the_line_shape() {
        let literals = vec![
            "compactmachines".to_owned(),
            "give".to_owned(),
            "new".to_owned(),
        ];
        let arguments = vec![CommandArgument::new(
            "template".to_owned(),
            CommandArgumentValue::String("compactmachines:tiny".to_owned()),
        )];
        let words = words_of(&literals, &arguments);
        let args: Vec<&str> = words.iter().skip(1).map(String::as_str).collect();
        assert_eq!(
            crate::runtime::parse(&args),
            RoomCommand::GiveNew {
                template: Template::Tiny
            }
        );
    }
}
