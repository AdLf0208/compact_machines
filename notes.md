# Compact Machines — port notes

Ported from **Compact Machines 7.0.81** (`CompactMods/CompactMachines`, **MIT**,
rank 0), Minecraft Java 1.21.1 NeoForge. The jar is
`compactmachines-neoforge-7.0.81.jar` (684,420 B, 457 entries,
`META-INF/neoforge.mods.toml` says `license = "MIT"`), kept as evidence at
`ports/_shared/licence-cache/compactmachines-neoforge-7.0.81.jar`. Upstream
behaviour was read from the public repository, branch `21.1/main`, commit
`83eaf43a641495b913b81f14616a64e91bae971f`; the inventory and the behaviour
extracted are in [FINDINGS.md](FINDINGS.md). This is a clean-room Rust rewrite,
not a translation.

**LOCAL-ONLY** until the maintainer approves publishing (ARCHITECTURE.md §8).
MIT permits redistribution with the copyright and licence notice; the block and
item art, models and blockstates are carried from the jar, so the notice travels
with them ([LICENSE](LICENSE)).

Status: **slice 1 implemented and verified in-game** — the machine block, its
room (with the jar's own sizes), the Personal Shrinking Device's enter/leave,
the world registry and the commands. **0.2.0 is the flat SDK package** (the
`.hnmod` container was retired on 2026-11-18); its rules are unchanged and its
ladder is green, but **no 0.2.0 in-game run exists** — `horizon-launcher` is not
installed here. See "Migration to the SDK format" at the end of this file.

## Class + tier and approach

**Class:** content · **tier:** deep (`ports/ledger.csv`, source-map row
`compactmachines`). Per `horizon/docs/modpacks.md` a `deep` port is about
entities, renderers, dimensions or new vanilla hooks; the machine's deep half is
the **room** — its identity, its geometry and moving a player in and out. The
brief's rule for this tier is "one room size, one entrance, and record what
needs a dimension API as a gap"; the slice does that and then proves the scaling
numbers with a second size.

**Approach chosen:** a room is a **sealed `solid_wall` shell in a reserved
Overworld volume**, identified by a room code and persisted in
`<world>/compactmachines/rooms.dat`; the player is moved by the deferred console
`execute in minecraft:overworld run tp …` from `server_tick` (the pin has no
player-position API and no dimension API, and `native_run_command` is refused
inside a player hook). The room shell is placed with `native_set_block` on the
first entry, once the arrival has loaded the chunks.

## The jar's numbers this slice uses (all from the jar, cited in FINDINGS.md §2)

| Template | Interior | Colour | Shell |
|---|---|---|---|
| tiny | 3 × 3 × 3 | `#C95B13` | 5³ (98 blocks) |
| small | 5 × 5 × 5 | `#D4D2D2` | 7³ |
| normal | 7 × 7 × 7 | `#FBF236` | 9³ (386 blocks) |
| large | 9 × 9 × 9 | `#211B2E` | 11³ |
| giant | 11 × 11 × 11 | `#43D6CD` | 13³ |
| colossal | 13 × 13 × 13 | `#423F42` | 15³ |
| farming | 21 × 11 × 21 | `#00FF00` | 23 × 13 × 23 |
| soaryn | 45 × 45 × 45 | `#7B2FBE` | 47³ |

Also carried: the machine's hardness 8 and blast resistance 20 with the metal
sound set (`minecraft:iron_block`, the content schema's way of naming iron's
sounds) and `requiresCorrectToolForDrops`; the breakable wall's 3 / 128; the
solid wall's hardness −1, blast resistance 3,600,000.8 and light 15; the PSD's
one-item stack and its 25-tick cooldown after each shift (kept server-side — the
pin has no helper to put a cooldown on a mod item) with its default
`max_allowed_depth = 5`; room codes of 12 characters from the 31-character
alphabet `0123456789ABDFGHJKLMNPQRSTVWXYZ` as `XXXX-XXXX-XXXX`; the 1024-block
room grid (a region is 64 chunks, the room centred on block 8 of it) and the
square-spiral allocation order. The machine's note-block instrument (`COW_BELL`)
and its `PushReaction.IGNORE` are not expressible in the content schema; both are
recorded as gaps rather than claimed.

## Hooks

*(The 0.1.0 `.hnmod` layout; the SDK mapping — every injection's new home — is
the "Helper → hook table" under "Migration to the SDK format" below.)*

| Hook (API) | Helper | What it does |
|---|---|---|
| `content_root` (AE2 pattern) | `embedded_content.rs` | writes the bundled `.hmod` to `.horizon-cache/` and adds it to the dev list — one installable file |
| `component_types` (65) | `component_types.rs` | registers `compactmachines:room_template`, `:room_code`, `:machine_color` before content loads |
| `block_entity` (15) | `block_entity.rs` | both machine blocks are block entities |
| `block_entity_ticks` (15) | `ticks.rs` | they tick |
| `place_block` (14) | `place.rs` | a placed machine takes its item's template (unbound) or room code (bound) |
| `use_item_on` (13) | `use_on.rs` | PSD on a machine: create the room (unbound) or enter it; claims the click |
| `use_item` (13) | `use_item.rs` | PSD in the air inside a room: leave to the last entry point |
| `right_click_block` (21) | `right_click.rs` | a click without a PSD says the room and its size |
| `destroy_block` (14) | `destroy.rs` | forgets the binding, remembers it for the drop |
| `loot_drops` (75) | `loot.rs` | the machine drops its item with the room code (or the template) |
| `world_lifecycle` (3) | `world.rs` | loads the registry at open, flushes it at close |
| `server_tick` (4) | `server_tick.rs` | runs the queued `/tp`, places the shell, writes the registry at most once a second |
| `server_stopping` (44) | `stopping.rs` | flush |
| `register_commands` (8) | `register.rs` | `/compactmachines rooms\|info\|give new\|give existing\|teleport` |
| `perform_command` (8) | `perform.rs` | the command bodies |

All 15 injections resolve against the pin's stamped `native-hooks.json`
(432 hooks / 174 helpers); no pending fragment is named.

## Slice boundary

In: the four blocks (`machine`, `new_machine`, `wall`, `solid_wall`), the three
items (`personal_shrinking_device`, `shrinking_module`, `enlarging_module`),
their recipes (wall, both modules, the PSD, the `normal` machine), loot, tags,
lang, models and textures carried from the jar; the room registry, room codes,
the region spiral, the eight templates' sizes as data, the shell build, entering
and leaving with the PSD, the nesting depth limit (5), the machine item carrying
its room through break/place, and the five commands.

Not ported (see Gaps): the compact dimension itself, its biome and dimension
files; the room generator's structures/floors beyond the shell; the preview
screen; room upgrades; the PSD's keybinds; Jade/JEI/Curios integrations; the
villager; the gamerules; OOB damage and teleport veto.

## Gaps (each one is an API or a slice the pin cannot carry yet)

| Gap | Why | What the slice does instead |
|---|---|---|
| **No dimension API** | the pin has three vanilla levels; `execute in` reaches only those, and nothing may add a level | rooms are sealed shells in the Overworld at `(30000 + region·1024, y=200)`; a mod-provided level would need a world/dimension API |
| **No player-position write** | the pin has no hook that moves a player; `native_run_command` is refused inside player hooks | the deferred `execute in <dim> run tp` from `server_tick` (the `waystones` pattern) |
| **No teleport veto** | `entity_teleport` is listen-only, so a non-command teleport out of a room cannot be cancelled | the shell is `solid_wall` (unbreakable), which keeps players in by geometry |
| **No pre-spawn veto outside a room** | upstream cancels entity spawns outside the room bounds | the room is lit (solid wall, light 15) and is a plain Overworld box; mobs can still walk in from outside if the shell is breached |
| **Block entity reads** | a hook cannot read another block entity's saved compound at an arbitrary position | the machine/room state lives in a world file keyed by dimension + position; the BE is created and saved but not the truth |
| **Per-template craftable machines** | upstream writes the template into the recipe result's item components; the `normal` recipe is carried with that component and `horizon-mod check` accepts it, but the other seven recipes differ only in the component and were not carried (and no recipe was exercised in game) | `/compactmachines give new <template>` gives the same item with the same component for any template, and placing it in game was verified for `normal` and `tiny` |
| Preview screen, upgrade screen, keybinds, Jade/JEI/Curios, villager, gamerules, OOB damage | client half and integrations | `/compactmachines info` replaces the preview; the rest is deferred |
| Machine dyeing and the coloured renderer | no block-entity component write is readable back, and the client tint layers are a renderer feature | the `machine_color` component is carried on the machine item from the template; the model's tint is not driven, so machines look uncoloured |
| The machine's note-block instrument (`COW_BELL`) and `PushReaction.IGNORE` | the content block schema carries hardness, blast resistance, light, sounds and tool need, but no note-block instrument and no piston reaction | not expressed: the machine uses the iron sound set and a piston may move it (hardness and resistance are the jar's) |
| PSD "set room spawn" (sneaking) | the slice has one spawn per room (`IRoomBoundaries.defaultSpawn`) | sneak-use is not special-cased; the room spawn is its centre |

## Deliberate deviations (kept, and why)

- **Spawn is on the floor.** Upstream's `defaultSpawn` is the interior centre
  with feet at `inner.minY + 1`, one block above the floor (a one-block drop on
  arrival); this port puts the feet at the interior floor so the player lands on
  it. Same column, one block lower.
- **The shell is `solid_wall`.** Upstream's room generator is part of its
  virtual chunk generator (not in the source branch read) and its walls are the
  breakable `wall`; without a teleport veto a breakable shell would leak into
  the Overworld, so the shell is the unbreakable variant. `wall` is carried and
  craftable with the jar's recipe.
- **25-tick cooldown instead of the item cooldown UI.** The pin has no helper to
  put a cooldown on a mod item, so the port keeps the cooldown server-side
  (it is also what stops one click from both entering and leaving: the click
  fires `use_item_on` *and* the item's own use).
- **Rooms are not rebuilt after a restart** unless they were never finished:
  `built` is persisted, and the shell cannot be broken by a player.

## Upstream defects fixed

None found that this slice implements. Two candidates were checked and are
**not** defects:

- **The one-block-high spawn** (`IRoomBoundaries.defaultSpawn` puts the feet at
  `innerBounds.minY + 1`, one block above the floor): a one-block drop that
  changes nothing a player can use, so it is recorded under deviations, not
  fixed as a defect.
- **Recipe ingredients under `c:` tags** recorded in the jar
  (`c:nuggets/iron`, `c:ingots/gold`, …): not a defect — the tags are NeoForge's
  ecosystem convention, so this port **declares them itself**
  (`content/data/c/tags/item/…`), which is the sanctioned way to keep the
  recipes matching (ruling 2026-11-11, step 3).

The jar's room sizes, colours, block facts and recipe shapes agree with its own
data and with the source read, so nothing else qualified.

## Verification (evidence paths)

| Step | Command | Result |
|---|---|---|
| Unit + embedded tests | `cargo test --locked --offline`; `tools/horizon.py test` | 32 tests, 32 embedded (`game::compactmachines::tests`), 0 failed |
| Lint | `cargo fmt --check`; `cargo clippy --all-targets -- -D warnings` | clean |
| Ladder | `tools/horizon.py stage → check → test → build` | `cargo_exit` 0 at check and build (`../_shared/evidence/compactmachines/ladder/horizon-{check,test,build}.log`) |
| Content | `horizon-mod build` + `horizon-mod check` | `no code: content only`; 27,675 B, SHA-256 `3663ec3b…a78c08`, 53 entries |
| Native bundle | `tools/package.py` + `horizon-mod check … --kind native-mod` | 37,922 B, SHA-256 `74a00b8a…93b663`, 20 entries; byte-identical on a second run |
| e2e (default) | `tools/e2e.sh` (control leg + embedded leg) | `PASS: …` `EXIT:0` — `_shared/evidence/compactmachines/e2e-default.log` |
| e2e (selftest) | `SELFTEST=1 tools/e2e.sh` | `FAIL: no tiny wall …` `EXIT:1` — `e2e-selftest.log` |
| Content alone | `tools/hmod-load.sh` (shared game) | `PASS … EXIT:0`, with a negative control — `hmod-load.log` |

The default e2e logs: the control leg (the same binary with `.horizon-cache`
unusable → both ids unknown, the block stays air), the embedded leg (no
`--dev-mod`; the bundle writes its 27,675 B content file, `/compactmachines give
new` → a 9³ shell with 7³ of air, the PSD in and out, a `tiny` machine → a 5³
shell with 3³ of air, the registry surviving a relaunch with both rooms and the
binding re-entered by `/compactmachines teleport <code>`), and `EXIT:0`.

## Skills

Skills: superpowers/using-superpowers,brainstorming,writing-plans,test-driven-development,systematic-debugging,verification-before-completion; BeautifulRust/rust-best-practices,rust-port

Read at the boot step: `AGENTS.md`, `ARCHITECTURE.md` §3–§10, the pin's
`docs/modpacks.md` / `docs/mods.md` / `docs/native-mods.md` /
`docs/horizon-ctl-commands.md`, `ports/_shared/AGENT_MOD_STRUCTURE.md`,
`ports/_shared/build-slots-usage.md`, the source-map row, the jar, and the
reference ports (`dimstorage`, `enderstorage`, `waystones`, `charginggadgets`).

## Reviewer repairs (2026-11-11) — PR review items and what changed


Each item below is quoted from `ports/_shared/horizon-mods/reviews/pr-*-compactmachines.md` (the review is the specification), with what this repair did about it.

- **Reviewer:** [P1] rooms overwrite the existing world without checking that their site is empty
  **Repair:** Fixed in code: a room only takes a site whose whole volume (shell **and** interior) is air. An occupied site moves the room to the next free region (`Store::relocate`), so no player build is overwritten and no interior can suffocate the arriving player. Tests: `site_tests::a_site_is_free_only_when_its_whole_box_is_air` and `an_occupied_site_is_moved_aside_and_only_the_room_moves`; the build loop logs the move and gives up after 64 sites. The registry page now states the persistent Overworld modification, the reserved volume and the site rule.
- **Reviewer:** [P2] attribution
  **Repair:** Already absolute in the entry (`https://github.com/CompactMods/CompactMachines`, authors Davenonymous, RobotGryphon, AdLf0208); the description prose was rewritten.

Skills: superpowers/using-superpowers,systematic-debugging,verification-before-completion; BeautifulRust/rust-best-practices

### Repair verification (2026-11-11/12)

- Staged `check`: check exit 0 (`ports/_shared/evidence/repair-2026-11-11/compactmachines-check.log`)
- Embedded tests: embedded **34 passed; 0 failed** (`ports/_shared/evidence/repair-2026-11-11/compactmachines-test.log`)
- In-game: e2e **PASS** + `EXIT:0` (`ports/_shared/evidence/repair-2026-11-11/compactmachines-e2e.log`): a 7x7x7 room in a 9x9x9 shell, PSD enter/leave, a 3x3x3 room, bindings surviving a relaunch. The e2e does **not** yet stage an occupied site; the site rule is proved by the two unit tests
- `cargo test` (standalone lib), `cargo clippy --all-targets -- -D warnings` and
  `cargo fmt --check` are clean in the port; the artifacts were built twice and
  hash identically (`registry.toml` carries the sha256 above).
- Skills: superpowers/using-superpowers,systematic-debugging,test-driven-development,verification-before-completion; BeautifulRust/rust-best-practices,rust-port


## Migration to the SDK format (2026-11-18)

Migrated from the retired `.hnmod` container (Horizon's mod-API merge #16941,
launcher ≥ 0.2.70; PR #17992 cleans up the old files) to the flat SDK package at
nightly **`0b14f8acfd97fa2c750e42795249dcc981178ca4`** (2026-11-18) / mixins
**`e59da9094f0c2fdda6c627fe9eff22dec6a906f0`**, package format 6, `api = "0.2"`,
version **0.2.0**. The registry closed this port's former submission **#836** on
2026-11-18 with the format purge, so 0.2.0 supersedes it. The reference recipes
were `ports/imfast/notes.md` → "Migration to the SDK format" (the 12-step form),
`ports/naturesaura/notes.md` (Parts, a block-entity tick, commands) and
`ports/enderstorage/notes.md` (a world file, a debounced save, a `BlockPart`
type and `ItemPart` placement).

**Class + tier, and the approach.** Unchanged: class **`content`**, tier
**`deep`** (`ports/ledger.csv`; source-map row `compactmachines`). Per
`horizon-1d63e0a/docs/modpacks.md` §"Tiers" a `deep` port is about entities,
renderers, dimensions or new vanilla hooks; the machine's deep half is the
**room** (its identity, its geometry, moving a player in and out), so the slice
still carries the machine's data (carry-over) and the room's behaviour (agent
port), and the compact **dimension** stays the recorded gap. The migration
changes no rule: it is the glue layer only.

### Done in this order (the order the traps require)

1. The retired paths were **moved** (never copied) into
   `ports/_shared/evidence/compactmachines/retired-hnmod-0.1.0/` **before** the
   first `cargo horizon-mod build`: `mixins/`, `compatibility.json`,
   `rust-toolchain.toml`, `content/` (after `assets/` and `data/` moved to the
   package root), `tools/audit/`, `tools/{horizon,package,test_stage,
   test_template,stamp_license,carry_content}.py` (the `.sh` ones too),
   `tools/{hmod-load,probe-launch,probe-place}.sh`, `.github/`,
   `compactmachines.hnmod`, `PLAN.md`, and the old port `AGENTS.md`;
   `Cargo.lock` was **deleted** (never packed).
2. `src/runtime.rs` was copied with `cp` and proved with `cmp` + `sha256sum`
   against the retired file (`e6394dbdf7cb203c83442d5db81dd17fa37498e7eb330e24cd522e0b164123a4`).
3. `cargo test` ran on the new crate with **no glue at all** (`src/lib.rs` was
   `pub mod runtime;`): **34 passed, 0 failed** — the runtime tests are green
   before any glue exists, as the campaign requires.
4. Only then were `src/support.rs` (the glue) and the one `impl Hooks` written,
   the manifest, and the package built.

### File map (old `.hnmod` → new SDK package)

| Old | New |
|---|---|
| `mixins/compactmachines/horizon-mixin.toml` (`[mod]`, `[compatibility]`, 2 `[[module]]`, 15 `[[injection]]`) | `horizon-mod.toml` (id/name/version/api/side/capabilities + `[native]` game/horizon/mixins) |
| `Cargo.toml` (`[lib] path = "mixins/compactmachines/runtime.rs"`, `[workspace] members = ["tools/audit"]`) | `Cargo.toml` (`[lib] path = "src/lib.rs"`, one dependency: `horizon-mod-api` from `.horizon/sdk/`) |
| `mixins/compactmachines/runtime.rs` (the crate lib) | `src/runtime.rs` — **byte-identical**, `pub mod runtime;` at the crate root |
| `mixins/compactmachines/support.rs` (1112 lines, injected as `compactmachines_state`) | `src/support.rs` — an ordinary module, the same glue rewritten against the SDK |
| 15 per-hook helper `.rs` files | methods of `impl Hooks`, of four Parts, or a `Registrar` declaration (table below) |
| `content/` (`assets/`, `data/`, its own `horizon-mod.toml`, `compactmachines.hmod`) | package root: `assets/`, `data/` — the content half of the same `.hmod` (52 carried files, unchanged) |
| `mixins/compactmachines/data/compactmachines-content.hmod` + `embedded_content.rs` (the AE2 write-out) | **gone**: code and content are one package, so there is nothing to embed |
| `compatibility.json`, `rust-toolchain.toml`, `horizon-native.json` | **gone**: `[native]` is the pin and the launcher's toolchain builds |
| `tools/horizon.py`, `tools/package.py`/`package.sh`, `tools/audit/`, `tools/test_*.py`, `tools/stamp_license.py`, `tools/carry_content.py`, `tools/hmod-load.sh`, `tools/probe-*.sh` | **gone** (into the evidence folder); the ladder is `cargo test` + `cargo horizon-mod build/check/fits` |
| `compactmachines.hnmod` (37,922 B, 20 entries) | `compactmachines.hmod` (69,948 B, 63 entries) |

**Kept and updated:** `tools/e2e.sh` (now for a launcher-built game: `GAME=`,
optional `CONTROL_GAME=`, the same scenario and assertions, `SELFTEST=1`
still fail-capable), `FINDINGS.md`, this file, `VALIDATION.md`, `README.md`
(rewritten), `LICENSE`, `LICENSE-NOTICE.md` (rewritten for the SDK layout), and
`data/compactmachines/license/{license,notice}.txt` (regenerated from the two
root files, as the 0.1.0 content half carried them).

### Manifest mapping

- **licence, re-read for 0.2.0:** `META-INF/neoforge.mods.toml` in
  `compactmachines-neoforge-7.0.81.jar` (684,420 B, sha256 `3518f732…eade5`)
  says `license = "MIT"` — rank 0, unchanged. `precheck-mod.py` prints
  `licence=MIT rank=0`, `class=content tier=deep`, `status=ported-partial`, and
  exits 3 because the port is *already* ported-partial: this job continues it,
  it is not a new dispatch (`../_shared/evidence/compactmachines/sdk-licence-read.txt`).
  Local-only until the maintainer approves publishing (ARCHITECTURE.md §8).
- identity: `id = "compactmachines"` (also the resource namespace), `name`,
  `version = "0.2.0"` (the port's version, not upstream's 7.0.81),
  `authors = ["Davenonymous", "RobotGryphon", "AdLf0208"]`, `license`/`homepage`/
  `issues` are **not** manifest fields (the builder strips them).
- `api = "0.2"` (exact), `side = "both"` (an integrated server belongs to the
  client installation, so single player and a LAN host need it),
  `capabilities = ["registries"]` (the schema's one value; this package really
  does declare blocks and a block entity type).
- `[native] game = "26.3"`, `horizon = "0b14f8a…"`, `mixins = "e59da90…"`
  replace `[compatibility] horizon_revision` and the bundle's mixins revision
  (`5a64af93` → `e59da909`).
- `[mod] priority = 100` is gone; nothing in this slice needs ordering.

### Helper → hook table (every old injection has a home)

| Old `[[injection]]` / helper | New place | Mapping notes |
|---|---|---|
| `[[module]] runtime` | `src/runtime.rs` + `pub mod runtime;` | byte-identical; `pub` items stay reachable so nothing warns `dead_code` |
| `[[module]] state` (support.rs) | `src/support.rs` | no plan section: an ordinary module |
| `embedded-content` (`content_root`, HEAD, cancellable) | **no hook** | the package carries `assets/` and `data/` itself |
| `component-types` (`component_types`, API 65) | `Hooks::component_types` → `support::declare_components` | same three ids, before content loads |
| `block-entity` (`block_entity`, API 15) | `Registrar::block_entity_type("machine").blocks(&[machine, new_machine]).build()` | a **declaration**; a declared type starts with an empty compound, which is exactly what the old hook returned (`Compound::new()`) |
| `block-entity-ticks` (`block_entity_ticks`, API 15) | `MachinePart::block_entity_ticks` (`event.ticks = true`) | the machine ticks, as the jar's `EntityBlock` does |
| — (no 0.1.0 tick body) | `MachinePart::block_entity_tick` → `Unchanged` | the slice has no per-tick work; `Changed` would dirty the chunk every tick |
| `place-block` (`place_block`, API 14) | `ItemPart::on_block_placed_by` on `Placed` | the **item** side; attached to both machine items; `placer` replaces `native_acting_player` for the owner |
| `use-item-on` (`use_item_on`, API 13) | `BlockPart::use_item_on` on `MachineClick` (attached to both machine blocks) | `*claimed = true` → `Some(InteractionResult::Success)`; the held stack is `event.click.stack` |
| `use-item` (`use_item`, API 13) | `ItemPart::use_item` on `ShrinkingDevice` | both sides: the client half returns `None`, the server half moves the player |
| `right-click-block` (`right_click_block`, API 21) | `Hooks::right_click_block` (`HostSide`) | server arm only; `Flow::Continue` keeps the click going, as the old hook never cancelled |
| `destroy-block` (`destroy_block`, API 14) | `Hooks::break_block` | cancellable; the handler runs only when `!cancelled`, then lets the break proceed |
| `loot-drops` (`loot_drops`, API 75) | `Hooks::loot_drops` | `context.block` gives the `(pos, state)` the old `NativeLootDrops` did |
| `world-lifecycle` (`world_lifecycle`, API 3) | `Hooks::world_lifecycle` | **close phases only**: the SDK's `WorldPhase` has no open-ready arm, so the load is lazy |
| `server-tick` (`server_tick`, API 4) | `Hooks::server_tick` | `ServerTickEnd { world_running }`; the queued `/tp` is now `EntityRef::teleport_to_level` |
| `stopping` (`server_stopping`, API 44) | `Hooks::server_stopping` | flush |
| `register-commands` (`register_commands`, API 8) | `Hooks::register_commands` | the same five usages, byte for byte |
| `perform-command` (`perform_command`, API 8) | `Hooks::run_mod_command` (`CommandInvocation`) | typed literals + arguments are rebuilt into the runtime's word list, so `runtime::parse` stays the one grammar |

No hook is left without a home and no helper was silently deleted.

### Gaps — what the migration changed

| Gap | 0.1.0 | 0.2.0 |
|---|---|---|
| No player-position write | a deferred console `execute in … run tp` from `server_tick` | **closed at the API level**: `EntityRef::teleport_to_level` (typed, no operator, no command parse). Still deferred to `server_tick` so the same tick's shell build follows the arrival |
| No dimension API | rooms are `solid_wall` shells in the machine's own level at `x,z = 30000 + region·1024`, `y = 200` | unchanged |
| No logger in a package | `log::info!`/`warn!` for placement, room codes, teleports, a taken site and an unsealed room | **dropped** (recorded): the SDK builds no `log` into a mod crate; the consequences are the same (a taken site moves the room, a shell retries for 600 ticks and then gives up) |
| No `WorldOpenReady` phase | the registry loaded in the open arm | loads lazily on the first running server tick, and every store entry point calls `ensure_world` first |
| No 0.2.0 in-game run | the 0.1.0 runs stand | **the port's open gap**: `horizon-launcher` is not installed here (only `cargo-horizon-mod`, the packer), so no game carrying this package exists. `tools/e2e.sh` is ready for it (`GAME=`, `CONTROL_GAME=`, `SELFTEST=1`) and the 0.1.0 logs are marked historical (`../_shared/evidence/compactmachines/HISTORICAL-0.1.0.md`) |

Everything else in the 0.1.0 gap list (preview/upgrade screens, keybinds, the
PSD's cooldown UI, Jade/JEI/Curios, the villager, the gamerules, out-of-bounds
damage and the teleport veto, machine dyeing's tint) is unchanged by the
migration.

### Traps this migration hit (the next port must know)

1. **`WorldPhase` has only the failure and close arms.** `WorldOpenReady` does
   not exist in the SDK, so a port that loaded its world file at open must load
   lazily; and because a click can arrive before the first tick, *every* store
   entry point calls `ensure_world`, not only `server_tick`.
2. **A teleport is a handle method, not a host one.** `entity_teleport_to_level`
   appears in `host/entities.rs` but that declaration is the `#[doc(hidden)]`
   backend trait; the mod-facing call is
   `player.entity().teleport_to_level(host, dimension, pos, None)`.
3. **The typed teleport keeps the entity's motion and takes feet coordinates.**
   To keep the 0.1.0 `/tp` arrival exactly, pass `[x + 0.5, y, z + 0.5]` (block
   centre, feet on the floor) and `None` rotation.
4. **No logger.** `log` is not a dependency of a package and adding a registry
   crate is refused by the pin, so the 0.1.0 diagnostics are gone; say so
   instead of inventing a channel.
5. **Item components need a `Component` descriptor.** Read with
   `stack.get(&ValueTag(id))` and write with `stack.set(&ValueTag(id), Tag::String(…))`;
   a descriptor whose `decode` accepts every tag would read an int as a room
   code. All three of the jar's components are strings, and `decode` refuses the
   rest.
6. **One Part attached to two blocks must be attached twice.** `block_part`
   takes a key by value, so bind `let machine = r.block_key(…)` first and attach
   the click Part to *both* machine blocks; attaching to one leaves the other
   silent. Same for the two machine items' `on_block_placed_by`.
7. **`BlockUpdateFlags` cannot be combined in a `const`.** `BitOr` is not a
   `const fn`, so `const SHELL_FLAGS = A | B` does not compile; use a function
   (or the inline expression). The old numeric `3` is gone.
8. **`BlockKey` has no `Default`.** `…unwrap_or_default()` on a key does not
   compile; guard the optional registration key with `let Some(key) = … else`.
9. **An empty Part method is still a subscription.** A `use_without_item` that
   only returns `None` showed up in `uses.json` and would be called on every
   empty-hand machine click; leaving it out is not the same as implementing it.
10. **The player's verbs are on `PlayerRef`,** not on `ServerHost`:
    `player.tell`, `player.action_bar`, `player.give_stack` (`/give`'s path:
    matching stacks, then empty slots, overflow thrown), `player.position` via
    `player.entity().position(host)`.
11. **`run_mod_command` never sees the raw line.** Rebuild the word list from
    `invocation.literals` plus the typed arguments so the runtime's tested
    grammar stays the one parser; a command that parses to `Unknown` returns
    `Flow::Continue` (not `Cancel`), as the old glue did.
12. **`LootDrops.context.block` is a state, not a name.** Match
    `Some((pos, state))`, test the state's block key, and take the dimension
    from `context.dimension` (a `&str`), not from the host.
13. **A `let` and its immediate return trip clippy's `let_and_return`** inside a
    match arm (`-D warnings` is the gate).
14. **Run `cargo fmt` before the `cmp` of `runtime.rs`, and re-check every
    time.** It was already formatted at 0.1.0 and `cargo fmt` left it alone
    (`cmp` and the packaged sha256 both still `e6394dbd…23a4`), but the byte
    identity is a claim that must be re-proved after every gate.

### Upstream defects fixed

None found in this migration, and none of the 0.1.0 findings changed (the
one-block spawn is a deviation, the `c:` tags are declared by the port). One
**port** defect was fixed while rewriting the loot path: 0.1.0 removed the
content's own machine drop *before* checking that the component write succeeded,
so a stack the game refused the component on would drop nothing. 0.2.0 builds
the stack, writes the component, and only then replaces the drop — a refused
write leaves the content's own drop in place.

### Build and verify (measured 2026-11-18)

Every command ran in a `ports/tools/build-slot.sh` lease with `BUILD_LOCAL=1`
(job `compactmachines-sdk`, steps `test`, `lint`, `fmt`, `build`, `check`),
**directly in the port**: the runner's step names speak the retired
`tools/horizon.py` and only set the thread count for this package.
`CARGO_INCREMENTAL=0`, no `--target-dir`, `Cargo.lock` removed before every
build.

```
cargo test --offline                              # 38 passed; 0 failed (34 runtime + 4 glue)
cargo clippy --all-targets --offline -- -D warnings   # exit 0, 0 warnings
cargo fmt --check                                 # clean
rm -f Cargo.lock && cargo horizon-mod build       # -> target/horizon-mod/compactmachines.hmod
cargo horizon-mod build                           # again: same bytes
cargo horizon-mod check compactmachines.hmod      # exit 0
cargo horizon-mod fits compactmachines.hmod --build nightly   # exit 0
```

`check` output (verbatim):

```
compactmachines 0.2.0 (native-mod, format native-1/0b14f8acfd97fa2c750e42795249dcc981178ca4/e59da9094f0c2fdda6c627fe9eff22dec6a906f0, for 26.3)
  69948 bytes, SHA-256 a6530b255f12c58a28a0973425b6ffc6298d6b37c45d136bde2b76d0bf73f4bf, unsigned
  native source: unsandboxed; not executed by the registry check. Compile and test against the exact pinned Horizon and Mixins revisions before review.
```

`fits` output (verbatim):

```
Compact Machines:
0b14f8a  made for
```

The archive is **63 entries**: `horizon-mod.toml`, `Cargo.toml`, `LICENSE`,
`LICENSE-NOTICE.md`, `README.md`, `assets/…` (21), `data/…` (33, the two licence
copies included), `src/lib.rs`, `src/runtime.rs`, `src/support.rs`, `uses.json`.
**No `Cargo.lock`.** `unzip -p compactmachines.hmod src/runtime.rs | sha256sum`
is `e6394dbd…23a4` — the retired file byte for byte.

The carried content is unchanged: the 52 files under `assets/` and `data/` hash
identically to the 0.1.0 content `.hmod`'s entries, except
`data/compactmachines/license/notice.txt`, which is regenerated from the
rewritten `LICENSE-NOTICE.md` (the licence text itself, `865b63ad…`, is
unchanged).

`uses.json` (`../_shared/evidence/compactmachines/sdk-uses.json`, summary in
`sdk-uses-summary.md`) is **363 surface entries**: 14 hooks, 11 `Hooks` methods,
3 Part kinds, the `registries` capability, 69 handle methods, 182 types, 21
traits, 2 macros.

| Check | Command | Result | Evidence |
|---|---|---|---|
| Runtime tests before any glue | `cargo test --offline` on `pub mod runtime;` | **34 passed** | `../_shared/evidence/compactmachines/sdk-test-runtime-before-glue.log` |
| Tests in the finished crate | `cargo test --offline` | **38 passed**, 0 failed | `sdk-test.log` |
| Lints | `cargo clippy --all-targets --offline -- -D warnings` | exit 0, 0 warnings | `sdk-clippy.log` |
| Formatting | `cargo fmt --check` | clean | `sdk-fmt.log` |
| Package | `cargo horizon-mod build` | 69,948 B, exit 0 | `sdk-build.log` |
| Same package twice | `cargo horizon-mod build` again | identical bytes, sha256 `a6530b25…f4bf` both times | `sdk-build-second.log`, `sdk-sha256.txt` |
| Registry check | `cargo horizon-mod check compactmachines.hmod` | exit 0 | `sdk-check.log` |
| Fits the pinned build | `cargo horizon-mod fits … --build nightly` | exit 0 | `sdk-fits.log` |
| Entry list | `unzip -Z1` | 63 entries | `sdk-entries.txt` |
| `runtime.rs` untouched | `unzip -p … src/runtime.rs \| sha256sum` | `e6394dbd…23a4` = the retired file | `sdk-runtime-sha256.txt` |
| No launcher, so no in-game run | `which horizon-launcher` | absent; `tools/e2e.sh` guards `GAME=` | `sdk-e2e-guard.txt` |

### Changelog for the resubmission (supersedes #836)

> Supersedes #836, closed on 2026-11-18 because Horizon retired the `.hnmod`
> container (#17941; PR #17992 cleans up the old files). 0.2.0 is the flat SDK
> package: `api = "0.2"`, `[native]` pinned to nightly
> `0b14f8acfd97fa2c750e42795249dcc981178ca4` and mixins
> `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`, package format 6, for 26.3. The
> fifteen old injections became fourteen typed hooks (`component_types`,
> `block_entity_ticks`, `block_entity_tick`, `on_block_placed_by`, `use_item_on`,
> `use_item`, `right_click_block`, `break_block`, `loot_drops`,
> `world_lifecycle`, `server_tick`, `server_stopping`, `register_commands`,
> `run_mod_command`) plus one `Registrar` block-entity declaration (the old
> `block_entity` hook) and three Parts; the `content_root` embedded-content
> injection is gone because the content half now travels in the same package.
> The deferred console `/tp` became the typed `EntityRef::teleport_to_level`, and
> the room registry loads lazily because the SDK's `WorldPhase` has no
> open-ready arm. `src/runtime.rs` and its 34 tests are unchanged, byte for byte;
> the five command usages and every readout line are unchanged. Package: 63
> entries, **69,948 B**, sha256
> `a6530b255f12c58a28a0973425b6ffc6298d6b37c45d136bde2b76d0bf73f4bf`; `check`
> and `fits` exit 0. No 0.2.0 in-game run: `horizon-launcher` is not installed
> here, so the 0.1.0 in-game evidence stands for the unchanged rules.

### Skills

Skills: superpowers/using-superpowers,writing-plans,test-driven-development,verification-before-completion,rust-port; BeautifulRust/rust-best-practices,rust-port

Read at the boot step: `AGENTS.md`, `ARCHITECTURE.md` §3–§10,
`ports/_shared/pins/horizon-1d63e0a/docs/{modpacks,mods,native-mods}.md`,
`ports/_shared/AGENT_MOD_STRUCTURE.md` §0,
`ports/_shared/build-slots-usage.md`, this port's `notes.md`/`PLAN.md`/`FINDINGS.md`,
the reference migrations (`imfast`, `naturesaura`, `enderstorage`,
`repeatable_trial_vaults`, `framedblocks`, `laserio`, `charginggadgets`), and the
SDK's own `api-source/` (the generated `Hooks`/`Part` traits and the host area
traits) rather than guessing a signature.
