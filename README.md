# Compact Machines for Horizon

A native Horizon port of **Compact Machines 7.0.81**
(`CompactMods/CompactMachines`, MIT) for Minecraft 26.3: a machine block that
owns a compact room you can enter and leave with the Personal Shrinking Device.

Slice 1 is the **machine and one room, at the jar's own sizes**: place a
`new_machine` (a 7³ `normal` room by default, or any template through
`/compactmachines give new <template>`), use the PSD on it, and you arrive
inside a sealed room whose interior is exactly the template's size — 7×7×7
inside a 9×9×9 shell for `normal`, 3×3×3 inside 5³ for `tiny`. Use the PSD inside
and you are back at the machine. Rooms have a code (`XXXX-XXXX-XXXX` from the
jar's own alphabet), an owner and a site on the jar's 1024-block grid, and they
persist in `<world>/compactmachines/rooms.dat`, so a room and its machine
survive a relaunch.

## One package, code and content

The 2026-11-17 mod-API merge retired the `.hnmod` container, so this is **one
flat `.hmod`** (`api = "0.2"`):

```text
compactmachines.hmod
  horizon-mod.toml   the only manifest: id, version, api, side, [native] pins
  Cargo.toml         one crate; its only dependency is the fetched SDK
  src/lib.rs         the one `#[mod_api::horizon_mod] impl Hooks`
  src/runtime.rs     the pure room rules (34 unit tests, no game type)
  src/support.rs     the glue: block entity, items, clicks, teleports, commands
  assets/, data/     the carried content: 4 blocks, 3 items, recipes, loot, tags
```

The room rules (templates, codes, the region spiral, the shell geometry, the
registry file) live in `src/runtime.rs` and are unchanged from the 0.1.0
`.hnmod` port; `src/support.rs` is the same glue, rewritten against the SDK's
typed hooks and Parts. `notes.md` carries the hook table and the migration's
own traps.

## The slice, and what it does not do

In: the four blocks (`machine`, `new_machine`, `wall`, `solid_wall`), the three
items (`personal_shrinking_device`, `shrinking_module`, `enlarging_module`),
their recipes, loot, tags, lang, models and textures carried from the jar; the
room registry, room codes, the region spiral, the eight templates' sizes, the
shell build, entering and leaving with the PSD, the nesting depth limit (5), the
machine item carrying its room through break/place, and the five commands.

Gaps (each named in `notes.md`): the compact **dimension** (the pin has no
dimension API, so rooms are sealed `solid_wall` shells in the machine's own
level at `x,z = 30000 + region·1024`, `y = 200`); the preview and upgrade
screens; the PSD's keybinds and its cooldown UI (the cooldown is server-side);
Jade/JEI/Curios; the villager; the gamerules; out-of-bounds damage and the
teleport veto; machine dyeing's coloured renderer.

## Commands

| Command | What it does |
|---|---|
| `/compactmachines rooms` | the registered rooms and their count |
| `/compactmachines info` | the room you stand in: its size, its code |
| `/compactmachines give new <template>` | a `new_machine` carrying that template (op 2) |
| `/compactmachines give existing <code>` | a bound `machine` for that room (op 2) |
| `/compactmachines teleport <code>` | enter that room (op 2) |

## Building it

The package is built by the launcher against the pinned game build
(`horizon-launcher native-build .`); `cargo horizon-mod build` packs it
(`cargo horizon-mod check compactmachines.hmod` is the registry's own check).
`src/runtime.rs` and its tests build and run with `cargo test` alone, which is
the port's unit gate.

Licence: MIT, read from the jar; see [LICENSE](LICENSE) and
[LICENSE-NOTICE.md](LICENSE-NOTICE.md). Local-only until the maintainer approves
publishing.
