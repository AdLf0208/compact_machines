# Compact Machines — a Horizon port

**Compact Machines** is a Minecraft mod. Minecraft Mod. Adds one simple game mechanic: Small rooms inside of blocks. This repository is a **port of it to [Horizon](https://github.com/horizon-rust/horizon)**, the Rust engine for Minecraft Java 26.3. It carries the complete source of the port plus the content files it ships, and installs as a single `.hmod` package.

Upstream: [CompactMods/CompactMachines](https://github.com/CompactMods/CompactMachines) — licence **MIT**.

| | |
|---|---|
| Upstream mod | Compact Machines ([source](https://github.com/CompactMods/CompactMachines)) |
| Ported to | Horizon 0b14f8a, Minecraft Java 26.3, mod API `0.2` |
| Package | `compactmachines.hmod` (install it with the Horizon launcher) |
| Licence | **MIT** — the upstream mod's licence; see [Licence and credit](#licence-and-credit) |

## What this port implements

### The slice, and what it does not do

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

## Not yet ported

No omissions are recorded for this slice; `notes.md` is the authoritative list.

## Installing

1. Install the [Horizon launcher](https://github.com/horizon-rust/horizon/releases) (0.2.70 or newer).
2. Add `compactmachines.hmod` to the instance's mods.
3. The package is pinned to a specific Horizon build; the launcher checks that pin.

## Licence and credit

Compact Machines was created by its upstream authors; the code, models and assets in the original mod
remain theirs and are used here under **MIT**. The Rust in this repository is a clean-room
rewrite for Horizon, not a translation of the original source.

- `LICENSE` — the upstream licence text.
- `LICENSE-NOTICE.md` — what this port carries and where it came from.
- The upstream project: https://github.com/CompactMods/CompactMachines

## Technical details

```
horizon-mod.toml   package manifest (id, version, api, [native] pin)
Cargo.toml         the crate; depends on `horizon-mod-api` from the fetched SDK
src/               lib.rs (the one `impl Hooks`), runtime.rs (pure logic + tests), glue
assets/ data/      the content the package ships
tools/             content generators and the e2e scenario
```

- Build: `cargo horizon-mod build` (packages; it does not compile against the game).
- Test: `cargo test`, `cargo clippy --all-targets -- -D warnings`.
- In-game: `ports/tools/launcher-game.sh run compactmachines` in the campaign tree, or install the
  `.hmod` and play; the harness scenario is `tools/e2e.sh`.

This repository is generated from the campaign's port at `ports/compactmachines/`; the port's own
`notes.md` is the full record (decisions, traps, evidence).
