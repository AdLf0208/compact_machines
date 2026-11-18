# Compact Machines — jar inventory and upstream findings

Ported from **Compact Machines 7.0.81** (`compactmachines-neoforge-7.0.81.jar`,
`META-INF/neoforge.mods.toml`: `modId = "compactmachines"`, `version = "7.0.81"`,
authors Davenonymous/RobotGryphon, **`license = "MIT"`**, issue tracker
<https://github.com/CompactMods/CompactMachines/issues>). Jar kept as evidence at
`ports/_shared/licence-cache/compactmachines-neoforge-7.0.81.jar` (684,420 B,
457 entries).

Upstream behaviour was read from the public repository
`CompactMods/CompactMachines`, branch **`21.1/main`**, commit
`83eaf43a641495b913b81f14616a64e91bae971f` (2026-11-04), i.e. the 1.21.1 line
the jar comes from. Behaviour only; the Rust is a clean-room rewrite. The clone
is analysis scratch (`ports/_shared/upstream/compactmachines/`, gitignored) and
is deleted after these findings.

## 1. Jar inventory (what the mod actually contains)

The name says "compact machines"; the jar is a **room/dimension mod**: a
machine block that owns a room in a custom dimension, a Personal Shrinking Device
(PSD) that moves the player in and out, room templates with sizes, walls, room
upgrades, commands, a villager, a preview screen and JEI/Jade/Curios
integrations. Families found:

| Family | Jar evidence |
|---|---|
| **Blocks (4 + 1 internal)** | `machine` (bound), `new_machine` (unbound), `wall` (breakable), `solid_wall` (unbreakable, light 15); `machine_void_air` is referenced by the c: tag but has no blockstate/model (dimension filler only) |
| **Block entities (2)** | `BoundCompactMachineBlockEntity` (holds `room_code`, owner, colour), `UnboundCompactMachineEntity` (holds the `room_template` id and colour) |
| **Items (7 + 4 textures)** | block items `machine`, `new_machine`, `wall`, `solid_wall`; `personal_shrinking_device`; `shrinking_module`; `enlarging_module` (`atom_resizer`/`core-chip` textures are unused legacy art) |
| **Dimension** | `data/compactmachines/dimension/compact_world.json` (`type: compactmachines:compact_world`, flat generator, 48-tall layer of `compactmachines:machine_void_air`) and `dimension_type/compact_world.json` (height 48, min_y 0, no ceiling, fixed time 18000, overworld effects) |
| **Biome** | `data/compactmachines/worldgen/biome/machine.json` (used by the flat generator) |
| **Room templates (8, in a built-in datapack)** | `datapacks/basic_templates/data/compactmachines/compactmachines/room_templates/{tiny,small,normal,large,giant,colossal,farming,soaryn}.json` — the sizes are below |
| **Recipes (12)** | `wall`, `personal_shrinking_device`, `shrinking_module`, `enlarging_module`, and `new_machine_<template>` × 8 (they differ only in the `room_template` / `machine_color` **item components**) |
| **Loot tables** | `blocks/machine`, `blocks/new_machine` (both `copy_components` of `machine_color` + `room_code`/`room_template` from the block entity), `blocks/wall` |
| **Tags** | `compactmachines:machine` (block+item, both machine ids), `new_machine`, `bound_machine` (item: `machine`), `shrinking_device` (item: PSD + optional `shrink:shrinking_device`), `c:relocation_not_supported` (machine, solid_wall, void air), `minecraft:mineable/pickaxe` + `minecraft:needs_iron_tool` (wall, machine, new_machine) |
| **Data components** | `compactmachines:room_code`, `compactmachines:room_template`, `compactmachines:machine_color`, `compactmachines:shrinking_device` |
| **Commands** | root `/compactmachines`: `teleport`, `eject`, `rebind`, `unbind`, `rooms`, `give new <template>` / `give existing <room-code>` (with an optional player), `upgrades`, `enable-basic-templates` |
| **Client** | preview screen of a room's blocks, room-upgrade screen, keybinds (quick-exit, upgrade screen), Jade plugin, JEI plugin, Curios PSD slot, custom machine model with two tint layers and a shader pair |
| **Other** | villager profession `tinkerer` + trade, 5 gamerules (`allow_*_oob`, `damage_oob`, `damage_psd_on_exit`), room-upgrade items in a separate datapack (empty in the jar) |

## 2. Numbers taken from the jar (not invented)

Room template internal dimensions (width × height × depth) and colour:

| Template | Interior | Colour | Source |
|---|---|---|---|
| `tiny` | 3 × 3 × 3 | `#C95B13` | `room_templates/tiny.json` |
| `small` | 5 × 5 × 5 | `#D4D2D2` | `small.json` |
| `normal` | 7 × 7 × 7 | `#FBF236` | `normal.json` |
| `large` | 9 × 9 × 9 | `#211B2E` | `large.json` |
| `giant` | 11 × 11 × 11 | `#43D6CD` | `giant.json` |
| `colossal` | 13 × 13 × 13 | `#423F42` | `colossal.json` |
| `farming` | 21 × 11 × 21 | `#00FF00` | `farming.json` (floor `minecraft:grass_block`) |
| `soaryn` | 45 × 45 × 45 | `#7B2FBE` | `soaryn.json` |

Block facts (`Machines.java`, `Rooms.java`):

- `machine` / `new_machine`: hardness **8.0**, blast resistance **20.0**, metal
  sound, note-block instrument `cow_bell`, push reaction `IGNORE`,
  `requiresCorrectToolForDrops`.
- `wall`: hardness **3.0**, blast resistance **128.0**,
  `requiresCorrectToolForDrops`.
- `solid_wall`: hardness **-1** (unbreakable), blast resistance
  **3600000.8**, metal sound, light level **15**.
- `personal_shrinking_device`: `stacksTo(1)`, default
  `ShrinkingDeviceConfiguration(max_allowed_depth = 5, allow_loops = true,
  after_use = do_nothing)`; entry and exit put a **25-tick** cooldown on it.
- Default machine colour `#FFFFFF`; a new room takes its template's colour.

Geometry (`RoomTemplate.java`, `IRoomBoundaries.java`, `RoomRegistrarData.java`,
`MathUtil.java`):

- The room's **outer** bounds are the interior size inflated by **1 block on all
  six sides** (`AABB.ofSize(0,0,0, w,h,d).inflate(1)`, moved up by `h/2`), so a
  7³ room is a 9³ shell: 386 shell blocks and 343 interior blocks.
- `innerBounds = outerBounds.deflate(1)`; the default spawn is the interior
  centre with feet at `innerBounds.minY + 1`.
- Room **region grid**: `getRegionPositionByIndex(i)` is a square spiral; a
  region becomes `ChunkPos(regionX * 64, regionZ * 64)`, i.e. rooms sit
  **1024 blocks apart**, centred 8 blocks into that chunk
  (`Vec3.atBottomCenterOf(chunk.getWorldPosition().offset(8, 0, 8))`).

Room code (`RoomCodeGenerator.java`): 12 characters from the 32-character
alphabet `0123456789ABDFGHJKLMNPQRSTVWXYZ` (no `C`, `E`, `I`, `O`, `U`),
formatted `XXXX-XXXX-XXXX` (14 characters with the dashes).

## 3. Behaviour extracted (not code)

- **Placing** a `new_machine` item places `compactmachines:new_machine`, whose
  block entity remembers the item's `room_template` (and colour).
- **Using a PSD on an unbound machine** creates a room from that template
  (registered with a fresh code, the region grid's next site, the owner), turns
  the block into `compactmachines:machine`, binds the code to the block, and
  teleports the player to the room's spawn.
- **Using a PSD on a bound machine** teleports the player into that room; using
  it on the block **without** a PSD opens a client preview screen.
- **Using a PSD inside a room** (not sneaking) pops one entry from that player's
  history and teleports them back to the machine they came from (or to their
  respawn/overworld when the history is empty). Sneaking sets the room's spawn
  point instead.
- **Break** drops the bound machine item with its `room_code` (loot
  `copy_components`), so it can be placed again and re-bound.
- **Nesting** is allowed up to the PSD's `max_allowed_depth` (5); a deeper
  attempt fails with "An otherworldly force prevents you from shrinking more."
- **Room boundary protection** is code, in the compact dimension only: a
  non-command teleport out of the room is cancelled, entities spawning outside
  the room are cancelled, and OOB players are handled by gamerules.
- **Colours** are client-side: the machine model has three layers (border,
  tint, overlay) and two tint indices fed from `machine_color`.

## 4. Differences this slice makes, and why

| Upstream | This port | Reason |
|---|---|---|
| Rooms live in `compactmachines:compact_world`, a code-generated dimension | Rooms are **sealed shells in the reserved Overworld volume** at `(30000 + region·1024, y=200)` | The pin has **no dimension API** (three vanilla levels only); `execute in <dim>` reaches just those. Recorded as the slice's main gap |
| The room shell is produced by the mod's virtual chunk generator | The port **places** a `solid_wall` shell with `native_set_block` on first entry | Same gap: no chunk generator; the unbreakable shell also stands in for the missing teleport veto |
| Break-point protection (cancel a non-command teleport out of the room, cancel spawns outside it) | Not implemented; the shell is unbreakable | `entity_teleport` is **listen-only** at the pin; no pre-spawn veto. Gaps list |
| Room walls are the breakable `compactmachines:wall` (players can enlarge their room) | The shell is `solid_wall`; `wall` is carried and craftable | Without a teleport veto a breakable shell would leak into the Overworld |
| Every template is a craftable `new_machine` variant (item component on the recipe result) | One `new_machine` recipe (the `normal` template); the other seven templates are given by `/compactmachines give new <template>` | Recipe-result components of a **native** component type need the type registered before content load; the item component path works (`native_set_component`), the recipe path is not proven, so the normal recipe is carried and the rest are command-reachable |
| Preview screen, room-upgrade screen, Jade/JEI/Curios, villager, gamerules, keybinds | Not ported | `client` half and integrations; recorded as gaps |
| Room spawn = interior centre, feet at `inner.minY + 1` | Feet on the interior floor (`inner.minY`) | Upstream's `+1` leaves the player one block above the floor (a 1-block drop on arrival); the port lands them on it. One-block deviation, recorded |
| Machine model as shipped (NeoForge `neoforge_data.block_light` on the tint layer) | Same model with that NeoForge-only key stripped | Horizon does not read `neoforge_data`; the key carries no behaviour |

## 5. Files read upstream (behaviour)

`machine/Machines.java`, `machine/block/{BoundCompactMachineBlock,
UnboundCompactMachineBlock,BoundCompactMachineBlockEntity,UnboundCompactMachineEntity}.java`,
`room/{Rooms,RoomCodeGenerator,NewRoomBuilder,RoomRegistrarData,CMRoomRegistrar,
RoomHelper,RoomBlocks,RoomEventHandler}.java`, `room/spatial/GraphChunkManager.java`,
`player/PlayerEventHandler.java`, `shrinking/{PersonalShrinkingDevice,Shrinking}.java`,
`command/{Commands,CMGiveMachineSubcommand,CMTeleportSubcommand,CMEjectSubcommand}.java`,
`util/MathUtil.java`, `core-api/.../room/template/RoomTemplate.java`,
`core-api/.../room/spatial/IRoomBoundaries.java`,
`core-api/.../shrinking/component/ShrinkingDeviceConfiguration.java`.
