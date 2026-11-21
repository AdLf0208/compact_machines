# Working on Compact Machines (native Horizon mod, SDK format)

> **Build slots (v2):** every build/test/lint step goes through the campaign runner
> (`ports/tools/build-slot.sh` from the campaign root). It **blocks in the kernel** until a
> slot is free — a freed slot is taken in ~0.07 s — so **never poll**: no `while ! slot; sleep`
> loop, no background build, never `--target-dir`. Read `../_shared/build-slots-usage.md` before
> running anything; observe with `status` / `watch` / `events` / `stats`. The runner does **not**
> cd into `--port`, so pass `-- bash -c 'cd ports/compactmachines && …'`, and use `BUILD_LOCAL=1`
> (the far box has no copy of this port). The runner's step names speak the retired `.hnmod`
> tool; for this package they only set the thread count.

This repository is the port, not Horizon and not horizon-mixins. Never modify the original
checkout or player resources; do not commit `target/`, `.horizon/`, exported sources or
Minecraft assets (the carried ones in `assets/` and `data/` are the mod's own, MIT, and ship
deliberately).

**There is no staging step and no mixin plan.** One flat `.hmod` holds `horizon-mod.toml` (the
only identity, `api = "0.2"`, `[native]` pins), `Cargo.toml` (its only dependency is
`horizon-mod-api` from the fetched `.horizon/sdk/`), `src/lib.rs` (the one
`#[mod_api::horizon_mod] impl Hooks`), `src/support.rs` (the game glue: the machine block entity,
the item components, the device's clicks, the deferred teleports, the shell build, the world file
and the commands) and `src/runtime.rs` (the pure room rules, unit-tested). `assets/` and `data/`
are the carried content and travel in the same file. The retired `.hnmod` layout is history:
`../_shared/evidence/compactmachines/retired-hnmod-0.1.0/`.

Keep every room rule and number in `src/runtime.rs` with its test; the glue only reads the game,
calls `runtime`, and writes the result back. **`src/runtime.rs` is byte-identical to the retired
port's file — do not "tidy" it.** A test of a glue-only helper belongs next to its code in
`src/support.rs`.

Validate, in this order, each inside a lease:

```sh
CARGO_INCREMENTAL=0 cargo test --offline            # 38 tests: 34 runtime, 4 glue
cargo fmt --check                                   # NOT --all: that checks the fetched SDK
CARGO_INCREMENTAL=0 cargo clippy --all-targets --offline -- -D warnings
rm -f Cargo.lock && cargo horizon-mod build         # target/horizon-mod/compactmachines.hmod
cp target/horizon-mod/compactmachines.hmod ./compactmachines.hmod
cargo horizon-mod check compactmachines.hmod        # the registry's own check
cargo horizon-mod fits compactmachines.hmod --build nightly
sha256sum compactmachines.hmod                      # quote it in notes.md
```

A `Cargo.lock` is packed and changes the artifact: remove it immediately before the build and
keep it gitignored.

The in-game leg (available since 2026-11-18): `ports/tools/launcher-game.sh run compactmachines`
from the campaign root builds a game at the pin carrying this package and runs `tools/e2e.sh` in a
build slot; record the `PASS`/`EXIT:0` line in `../_shared/evidence/compactmachines-e2e/` together
with a control run (`GAME=$CONTROL_GAME`, must end `EXIT:1`) and a `SELFTEST=1` run (`EXIT:1`).
A source-only change is **not** seen by the launcher's cache: delete the mod's installed native
version under `~/.cache/horizon-e2e/root/data/horizon-launcher/versions/` (only the one carrying
this mod) before rebuilding, or the e2e runs the previous package.
