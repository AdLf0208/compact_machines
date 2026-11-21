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
    ServerHost, ServerTickEvent, ServerTickPhase, Tag, Text, TicketController, TicketOwner,
    UseItemEvent, UseItemOnEvent, ValidateTicketsEvent, WorldLifecycleEvent, WorldPhase,
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
/// A room that is not ready this long (its chunks never load, or every site it tries is taken)
/// stops holding its player: the player is told and stays where they are, never moved into an
/// unbuilt room.
const ENTRY_WAIT_TICKS: u32 = 200;
/// How many players may wait for a room at once. A click from a player already waiting
/// replaces their own entry, so one player cannot grow the queue.
const MAX_PENDING_ENTRIES: usize = 8;
/// The PSD's cooldown after a shift, in ticks (`RoomHelper` puts a 25-tick cooldown on the device
/// at every entry and every exit).
const PSD_COOLDOWN_TICKS: u64 = 25;
/// The 0.1.0 command wordings, unchanged: the e2e's scenarios and the published README name
/// them, so a rename here is a user-visible break. The *argument kinds* had to change: a
/// `word` is `[0-9A-Za-z_]`, so it can carry neither the `:` of `compactmachines:normal` nor the
/// `-` of a room code (`XXXX-XXXX-XXXX`), and the dispatcher refused every documented line with
/// "Expected whitespace to end one argument" (found by this port's first in-game run). The last
/// argument is a greedy text, which carries both.
const USAGE_ROOMS: &str = "compactmachines rooms";
const USAGE_INFO: &str = "compactmachines info";
const USAGE_GIVE_NEW: &str = "compactmachines give new <template:text>";
const USAGE_GIVE_EXISTING: &str = "compactmachines give existing <code:text>";
const USAGE_TELEPORT: &str = "compactmachines teleport <code:text>";

/// Registration keys, so a hot path compares block identity instead of allocating a name.
static MACHINE_KEY: OnceLock<BlockKey> = OnceLock::new();
static NEW_MACHINE_KEY: OnceLock<BlockKey> = OnceLock::new();
static SOLID_WALL_KEY: OnceLock<BlockKey> = OnceLock::new();
static PSD_KEY: OnceLock<ItemKey> = OnceLock::new();
/// This package's own chunk-ticket controller (registered once, in `register`).
static TICKETS: OnceLock<TicketController> = OnceLock::new();

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
}

/// A room entry waiting for its destination to be ready. The player is **not** moved until the
/// room is validated and built: a later pass can then never move the room out from under them.
struct PendingEntry {
    player: u128,
    code: String,
    /// Where the player came from: their own position and **their own** dimension.
    origin: [i32; 3],
    origin_dim: String,
    waited: u32,
}

/// What one preparation step of an unbuilt room must do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// The room is built and its site is final: the player may be moved.
    Ready,
    /// Its shell's chunks are not loaded (or a write failed): try again next tick.
    Wait,
    /// Its site is taken: move the room to the next free one, then try again.
    Relocate,
    /// Its site is loaded and free: write the shell.
    Build,
    /// The room no longer exists (or no site is left): drop the entry.
    Gone,
}

/// What preparation must do, from the room's state and its site. The one rule the entry path
/// must keep: an unbuilt room is never `Ready`, so nothing is ever moved into a site the next
/// pass may abandon.
fn step(built: bool, loaded: bool, blocked: bool) -> Step {
    if built {
        return Step::Ready;
    }
    if !loaded {
        return Step::Wait;
    }
    if blocked { Step::Relocate } else { Step::Build }
}

/// Whether a waiting entry has waited too long.
fn entry_expired(waited: u32) -> bool {
    waited >= ENTRY_WAIT_TICKS
}

/// Queue an entry, or replace the same player's own. A full queue refuses the newest player
/// rather than growing without a bound.
fn admit(pending: &mut Vec<PendingEntry>, entry: PendingEntry) -> bool {
    if let Some(own) = pending
        .iter_mut()
        .find(|waiting| waiting.player == entry.player)
    {
        *own = entry;
        return true;
    }
    if pending.len() >= MAX_PENDING_ENTRIES {
        return false;
    }
    pending.push(entry);
    true
}

struct Core {
    world: Option<PathBuf>,
    store: Store,
    rng: Rng,
    /// Teleports to run at the next tick's end.
    teleports: Vec<Teleport>,
    /// Entries waiting for their room to be validated and built.
    entries: Vec<PendingEntry>,
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
            entries: Vec::new(),
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
    state.entries.clear();
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
        state.entries.clear();
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
    // A room's chunks are held by this package's own ticket while the room is being built (the
    // player is not the one loading them any more). `validates` asks for `validate_tickets`,
    // which drops tickets a previous run left behind: an entry never survives a restart.
    let _ = TICKETS.set(r.register_ticket_controller("rooms", true));

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
    // The click is in the block's level; the player's own level is what their return point
    // means (reach keeps them in the same level here, but the player is the authority).
    let origin_dim = player
        .dimension(&*host)
        .map_or_else(|| dimension.clone(), |id| id.to_string());
    begin_entry(host, click.player, &code, player_pos, &origin_dim);
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

/// A level loaded the saved chunk tickets of this package's controller. An entry is never
/// persisted, so every ticket found here is from a run that ended before it finished: remove
/// them, so a restart never keeps a room's chunks (or their entities) loaded.
pub fn validate_tickets(_host: &mut dyn ServerHost, event: &mut ValidateTicketsEvent<'_>) {
    let blocks: Vec<[i32; 3]> = event
        .validation
        .block_tickets()
        .into_iter()
        .map(|(pos, _set)| pos)
        .collect();
    for pos in blocks {
        event.validation.remove_block_tickets(pos);
    }
}

/// End of a running tick: ready the rooms whose players are waiting, run the queued teleports
/// (only ever into a built room), and write the store at most once a second.
pub fn server_tick(host: &mut dyn ServerHost, event: &mut ServerTickEvent<'_>) {
    let ServerTickPhase::ServerTickEnd { world_running } = event.phase else {
        return;
    };
    if !world_running {
        return;
    }
    ensure_world(host);
    let (entries, save) = {
        let mut guard = core();
        let Some(state) = guard.as_mut() else {
            return;
        };
        state.ticks += 1;
        let save =
            state.dirty && state.ticks.saturating_sub(state.last_save) >= SAVE_INTERVAL_TICKS;
        (std::mem::take(&mut state.entries), save)
    };
    // Ready each waiting room before its player moves: the site is validated and the shell is
    // written while the player is still at their origin, so the destination cannot change under
    // them afterwards.
    let mut waiting = Vec::new();
    for mut entry in entries {
        match prepare_room(host, &entry.code) {
            Step::Ready => finish_entry(
                host,
                entry.player,
                &entry.code,
                entry.origin,
                &entry.origin_dim,
            ),
            Step::Gone => notify(
                host,
                entry.player,
                tr(
                    "rooms.errors.compactmachines.room_not_found",
                    &[&entry.code],
                ),
            ),
            _ if entry_expired(entry.waited) => {
                release_room(host, &entry.code);
                notify(
                    host,
                    entry.player,
                    Text::literal("The room could not be prepared; nothing was changed"),
                );
            }
            _ => {
                entry.waited += 1;
                waiting.push(entry);
            }
        }
    }
    let teleports = {
        let mut guard = core();
        match guard.as_mut() {
            Some(state) => {
                state.entries = waiting;
                if save {
                    state.last_save = state.ticks;
                }
                std::mem::take(&mut state.teleports)
            }
            None => return,
        }
    };
    for teleport in teleports {
        if !teleport_player(host, &teleport) {
            // The player left between the click and the tick: drop the entry from their history
            // so a later exit is not one room behind.
            let mut guard = core();
            if let Some(state) = guard.as_mut() {
                let _ = state.store.leave(teleport.player);
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

/// One line to a player's action bar, silently dropped when they are gone.
fn notify(host: &mut dyn ServerHost, player: u128, line: Text) {
    if let Some(target) = host.player(player) {
        let _ = target.action_bar(host, line);
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

/// A room's geometry, the level it stands in, and whether its shell exists.
fn room_state(code: &str) -> Option<(RoomBox, String, bool)> {
    let guard = core();
    let state = guard.as_ref()?;
    let room = state.store.room(code)?;
    Some((state.store.room_box(code)?, room.dim.clone(), room.built))
}

// ---------------------------------------------------------------------------
// Entry: ready the room, then move the player
// ---------------------------------------------------------------------------

/// Record the entry and queue the teleport. The history holds the player's **own** dimension
/// and position: the room's level is where they arrive, never where they come back to (a bound
/// machine carried to another dimension must not send them to its origin's coordinates in the
/// room's world).
fn record_entry(
    store: &mut Store,
    player: u128,
    code: &str,
    origin: [i32; 3],
    origin_dim: &str,
    max_depth: usize,
) -> Enter {
    store.enter(player, code, origin, origin_dim, max_depth)
}

/// Ask for an entry into `code`. The player is moved only once the room's site is validated and
/// its shell is built; until then the entry waits in the tick's queue.
fn begin_entry(
    host: &mut dyn ServerHost,
    player: u128,
    code: &str,
    origin: [i32; 3],
    origin_dim: &str,
) {
    start_cooldown(player);
    if prepare_room(host, code) == Step::Ready {
        finish_entry(host, player, code, origin, origin_dim);
        return;
    }
    let mut guard = core();
    let Some(state) = guard.as_mut() else {
        return;
    };
    admit(
        &mut state.entries,
        PendingEntry {
            player,
            code: code.to_owned(),
            origin,
            origin_dim: origin_dim.to_owned(),
            waited: 0,
        },
    );
}

/// One preparation step for a room: hold its shell's chunks, and once they are loaded validate
/// its site (relocating while it is taken) and write the shell.
///
/// The old order teleported first and built from the arrival: a site the build pass then found
/// occupied was abandoned, which left the player at a destination that no longer belonged to a
/// room (and the PSD then refused to take them back). Now the site is decided before the move.
fn prepare_room(host: &mut dyn ServerHost, code: &str) -> Step {
    let Some((room_box, room_dim, built)) = room_state(code) else {
        return Step::Gone;
    };
    if built {
        return Step::Ready;
    }
    let Some(level) = host.level(&room_dim) else {
        return Step::Wait;
    };
    // Holding the chunks is what makes the site readable: a chunk nobody visited reads as no
    // block, which would hide the terrain (or a player's build) that makes the site unusable.
    hold_room(host, level, &room_box, true);
    let loaded = room_loaded(&*host, level, &room_box);
    let blocked = loaded && blocked_site(&*host, level, &room_box).is_some();
    match step(built, loaded, blocked) {
        Step::Relocate => {
            // Nothing is written and nobody is moved: the room takes the next free site and the
            // next tick judges that one.
            hold_room(host, level, &room_box, false);
            let moved = {
                let mut guard = core();
                guard
                    .as_mut()
                    .is_some_and(|state| state.store.relocate(code).is_some())
            };
            if moved { Step::Wait } else { Step::Gone }
        }
        Step::Build => {
            if build_shell(host, level, &room_box) {
                {
                    let mut guard = core();
                    if let Some(state) = guard.as_mut() {
                        state.store.set_built(code, true);
                        state.dirty = true;
                    }
                }
                hold_room(host, level, &room_box, false);
                Step::Ready
            } else {
                Step::Wait
            }
        }
        ready => ready,
    }
}

/// Finish an entry: record the player's own return point, then queue the teleport into the
/// room's level.
fn finish_entry(
    host: &mut dyn ServerHost,
    player: u128,
    code: &str,
    origin: [i32; 3],
    origin_dim: &str,
) {
    let (entered, room_dim) = {
        let mut guard = core();
        let Some(state) = guard.as_mut() else {
            return;
        };
        let entered = record_entry(
            &mut state.store,
            player,
            code,
            origin,
            origin_dim,
            MAX_ENTRY_DEPTH,
        );
        let room_dim = state.store.room(code).map(|room| room.dim.clone());
        (entered, room_dim)
    };
    match entered {
        Enter::Entered { spawn } => {
            let Some(room_dim) = room_dim else {
                return;
            };
            {
                let mut guard = core();
                if let Some(state) = guard.as_mut() {
                    state.teleports.push(Teleport {
                        player,
                        dim: room_dim,
                        pos: spawn,
                    });
                }
            }
            if let Some(target) = host.player(player) {
                let _ = target.action_bar(
                    host,
                    tr("machine.compactmachines.machine.bound_to", &[code]),
                );
            }
            start_cooldown(player);
        }
        Enter::TooDeep => {
            if let Some(target) = host.player(player) {
                let _ = target.action_bar(host, tr("compactmachines.errors.too_far_down", &[]));
            }
        }
        Enter::NoSuchRoom => {}
    }
}

/// The chunks a room's shell touches (its interior inflated by one block).
fn room_chunks(room_box: &RoomBox) -> Vec<[i32; 2]> {
    let min_x = room_box.min[0] - 1;
    let max_x = room_box.min[0] + room_box.dims[0] as i32;
    let min_z = room_box.min[2] - 1;
    let max_z = room_box.min[2] + room_box.dims[2] as i32;
    let mut chunks = Vec::new();
    for x in (min_x >> 4)..=(max_x >> 4) {
        for z in (min_z >> 4)..=(max_z >> 4) {
            chunks.push([x, z]);
        }
    }
    chunks
}

/// Hold (or release) this package's own chunk ticket on every chunk of a room's shell, owned by
/// the room's spawn block. A hold does not wait for generation: the room is built once the
/// chunks report loaded, which is what the next preparation step asks.
fn hold_room(host: &mut dyn ServerHost, level: LevelRef, room_box: &RoomBox, forced: bool) {
    let Some(controller) = TICKETS.get().copied() else {
        return;
    };
    let owner = TicketOwner::Block(room_box.spawn());
    for at in room_chunks(room_box) {
        let Some(chunk) = level.chunk_ref(&*host, at) else {
            continue;
        };
        // `false` from the call means "the tickets did not change" (already held or already
        // released), not a refusal.
        let _ = chunk.set_mod_forced(host, controller, owner, forced, false);
    }
}

/// Whether every chunk of a room's shell is loaded, so its blocks can be read and written.
fn room_loaded(host: &dyn Host, level: LevelRef, room_box: &RoomBox) -> bool {
    room_chunks(room_box).into_iter().all(|at| {
        level
            .chunk_ref(host, at)
            .is_some_and(|chunk| chunk.loaded(host))
    })
}

/// Release the tickets of a room's current site (a give-up or a close).
fn release_room(host: &mut dyn ServerHost, code: &str) {
    let Some((room_box, room_dim, _)) = room_state(code) else {
        return;
    };
    let Some(level) = host.level(&room_dim) else {
        return;
    };
    hold_room(host, level, &room_box, false);
}

/// Write the room's one-block shell. A write that does not land waits for the next tick.
fn build_shell(host: &mut dyn ServerHost, level: LevelRef, room_box: &RoomBox) -> bool {
    let Some(wall) = SOLID_WALL_KEY.get().copied() else {
        return false;
    };
    let Some(wall_state) = host.block_state_default(wall) else {
        return false;
    };
    let mut done = true;
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
    done
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
            begin_entry(host, player.uuid(), &code, pos, &dimension);
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
                ("compactmachines give new <template:text>", 2),
                ("compactmachines give existing <code:text>", 2),
                ("compactmachines teleport <code:text>", 2),
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

    /// The review finding: the entry used to teleport the player first, and a later pass could
    /// move the room away from under them. An unbuilt room is never `Ready`, so no teleport is
    /// ever queued for one; a taken site is relocated, never built over.
    #[test]
    fn an_unbuilt_room_is_never_ready_and_a_taken_site_moves() {
        for loaded in [false, true] {
            for blocked in [false, true] {
                assert_ne!(
                    step(false, loaded, blocked),
                    Step::Ready,
                    "an unbuilt room answered Ready (loaded={loaded}, blocked={blocked})"
                );
            }
        }
        assert_eq!(step(false, true, true), Step::Relocate);
        assert_eq!(step(false, true, false), Step::Build);
        assert_eq!(step(false, false, false), Step::Wait);
        assert_eq!(step(false, false, true), Step::Wait);
        assert_eq!(step(true, false, true), Step::Ready);
        assert_eq!(step(true, true, false), Step::Ready);
    }

    /// A room is only built once every chunk of its shell is loaded, so the site is judged on
    /// real blocks: an unloaded chunk reads as no block and would hide a taken site.
    #[test]
    fn the_shell_is_not_written_before_its_chunks_are_loaded() {
        assert_eq!(step(false, false, false), Step::Wait);
        assert_eq!(step(false, true, false), Step::Build);
    }

    /// The review finding: a machine-triggered entry recorded the room's dimension with the
    /// player's origin coordinates, so an exit after the machine was carried to another
    /// dimension sent the player to those coordinates in the wrong world. The entry records the
    /// player's own dimension, which is the only world their coordinates mean anything in.
    #[test]
    fn an_entry_returns_to_the_players_own_dimension_not_the_rooms() {
        let mut store = Store::new();
        let mut rng = Rng::new(1);
        let code = store
            .create_room(
                Template::Normal,
                7,
                [0, 100, 0],
                "minecraft:overworld",
                &mut rng,
            )
            .code
            .clone();
        // The bound machine was carried through a portal: the room stays in the Overworld,
        // the click (and so the player) is in the Nether.
        let entered = record_entry(
            &mut store,
            42,
            &code,
            [10, 70, -4],
            "minecraft:the_nether",
            MAX_ENTRY_DEPTH,
        );
        assert!(matches!(entered, Enter::Entered { .. }));
        let left = store.leave(42).expect("the entry was recorded");
        assert_eq!(left.dim, "minecraft:the_nether");
        assert_eq!(left.pos, [10, 70, -4]);
    }

    /// A player spamming the device, or a machine clicked twice, gets one pending entry, not a
    /// queue: the wait is per player and bounded.
    #[test]
    fn pending_entries_are_one_per_player_and_bounded() {
        fn of(player: u128, code: &str) -> PendingEntry {
            PendingEntry {
                player,
                code: code.to_owned(),
                origin: [0, 100, 0],
                origin_dim: "minecraft:overworld".to_owned(),
                waited: 0,
            }
        }
        let mut pending = Vec::new();
        assert!(admit(&mut pending, of(1, "AAAA-AAAA-AAAA")));
        assert!(admit(&mut pending, of(2, "BBBB-BBBB-BBBB")));
        // The same player's new click replaces their own entry instead of growing the queue.
        assert!(admit(&mut pending, of(1, "AAAA-AAAA-AAAA")));
        assert_eq!(pending.len(), 2);
        for player in 3..(2 + MAX_PENDING_ENTRIES as u128 + 8) {
            admit(&mut pending, of(player, "CCCC-CCCC-CCCC"));
        }
        assert_eq!(pending.len(), MAX_PENDING_ENTRIES);
        assert!(!entry_expired(0));
        assert!(entry_expired(ENTRY_WAIT_TICKS));
    }
}
