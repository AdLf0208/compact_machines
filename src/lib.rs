//! Compact Machines for Horizon — slice 1: the machine, its room and the Personal Shrinking Device.
//!
//! Ported from Compact Machines 7.0.81 (`CompactMods/CompactMachines`, MIT); see `FINDINGS.md`
//! for the jar inventory and the behaviour read from the upstream repository, and `notes.md`
//! for the hook map, the gaps and the migration to the SDK format.
//!
//! [`runtime`] is the pure truth — the jar's eight room templates, room codes, the 1024-block
//! region spiral, a room's shell geometry and the store of rooms/machines/history with its world
//! file — and it has no `mod_api` type, so `cargo test` alone proves it. [`support`] is the glue:
//! the machine's block entity, the machine items' components, the device's clicks, the deferred
//! teleports, the shell build, the world file and the commands.
//!
//! # Where the retired container's hooks went
//!
//! The 2026-11-17 mod-API merge (#16941, launcher ≥ 0.2.70) retired `.hnmod`: the plan, the
//! injection points and the per-hook helper files are gone. A hook is a typed method here, and a
//! moment that belongs to one block, item or block entity is a **Part** attached to it.
//!
//! | `.hnmod` hook (API) | here |
//! |---|---|
//! | the `content_root` injection (AE2 pattern) | gone: one flat `.hmod` carries `assets/` and `data/` itself |
//! | `component_types` (65) | [`Mod::component_types`] → `support::declare_components` |
//! | `block_entity` (15) | the block entity type declared in [`Mod::register`] |
//! | `block_entity_ticks` (15) | `support::MachinePart::block_entity_ticks` |
//! | `place_block` (14) | `ItemPart::on_block_placed_by` on `support::Placed` |
//! | `use_item_on` (13) | `BlockPart::use_item_on` on `support::MachineClick` |
//! | `use_item` (13) | `ItemPart::use_item` on `support::ShrinkingDevice` |
//! | `right_click_block` (21) | [`Mod::right_click_block`] |
//! | `destroy_block` (14) | [`Mod::break_block`] |
//! | `loot_drops` (75) | [`Mod::loot_drops`] |
//! | `world_lifecycle` (v1) | [`Mod::world_lifecycle`] (close only: the registry loads lazily) |
//! | `server_tick` (4) | [`Mod::server_tick`] |
//! | `validate_tickets` (new) | [`Mod::validate_tickets`]: drop chunk tickets a crashed entry left |
//! | `server_stopping` (44) | [`Mod::server_stopping`] |
//! | `register_commands` (8) | [`Mod::register_commands`] |
//! | `perform_command` (8) | [`Mod::run_mod_command`] (typed arguments) |

pub mod runtime;
pub mod support;

use mod_api::{
    self, ComponentTypesEvent, Flow, Hooks, RegisterCommandsEvent, Registrar, RegistrationContext,
    RunModCommandEvent, ServerHost, ServerStoppingEvent, ServerTickEvent, ValidateTicketsEvent,
    WorldLifecycleEvent,
};

/// The mod keeps its state in [`support`]'s `Mutex<Option<Core>>`, not in this struct: one store
/// serves every machine in the world.
#[derive(Default)]
struct Mod;

#[mod_api::horizon_mod]
impl Hooks for Mod {
    /// Declare what the game answers for the two machine blocks (one block entity type) and the
    /// three items (the two machine items' placement, the device's use).
    fn register(&self, r: &mut dyn Registrar, _cx: &RegistrationContext) {
        support::declare(r);
    }

    /// The three item components a machine carries, registered before content (and so the item
    /// stack codec) loads.
    fn component_types(&self, _context: &RegistrationContext, event: &mut ComponentTypesEvent<'_>) {
        support::declare_components(event.types);
    }

    /// `/compactmachines rooms|info|give new|give existing|teleport`.
    fn register_commands(&self, _host: &mut dyn ServerHost, event: &mut RegisterCommandsEvent<'_>) {
        support::declare_commands(event.commands);
    }

    /// Run one of the commands above and take it.
    fn run_mod_command(
        &self,
        host: &mut dyn ServerHost,
        event: &mut RunModCommandEvent<'_>,
    ) -> Flow {
        support::run_command(host, event)
    }

    /// A click on a machine without the device says what the room is.
    fn right_click_block(
        &self,
        host: mod_api::HostSide<'_>,
        event: &mut mod_api::RightClickBlockEvent<'_>,
    ) -> Flow {
        support::right_click(host, event)
    }

    /// A player broke a machine: forget the binding and remember it for the drop.
    fn break_block(
        &self,
        host: &mut dyn ServerHost,
        event: &mut mod_api::BreakBlockEvent<'_>,
    ) -> Flow {
        support::break_block(host, event)
    }

    /// The broken machine's item drop carries its room (or its template).
    fn loot_drops(&self, host: &mut dyn ServerHost, event: &mut mod_api::LootDropsEvent<'_>) {
        support::loot_drops(host, event);
    }

    /// The world opened or closed: load lazily, flush and forget on a close.
    fn world_lifecycle(&self, host: &mut dyn ServerHost, event: &mut WorldLifecycleEvent<'_>) {
        support::world_lifecycle(host, event);
    }

    /// End of a running tick: ready the rooms whose players wait, the deferred teleports, and
    /// the debounced registry write.
    fn server_tick(&self, host: &mut dyn ServerHost, event: &mut ServerTickEvent<'_>) {
        support::server_tick(host, event);
    }

    /// A level loaded this package's saved chunk tickets: an entry never survives a restart, so
    /// every one of them is stale and is dropped.
    fn validate_tickets(&self, host: &mut dyn ServerHost, event: &mut ValidateTicketsEvent<'_>) {
        support::validate_tickets(host, event);
    }

    /// The server is stopping: flush the registry.
    fn server_stopping(&self, host: &mut dyn ServerHost, _event: &mut ServerStoppingEvent) {
        support::server_stopping(host);
    }
}
