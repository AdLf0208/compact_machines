# Compact Machines slice 1 — validation record

Pin: Horizon `1d63e0a5a37a7a708eb0b32b49bf0dfeb28ed8a9` (432 stamped hooks /
174 helpers), horizon-mixins `5a64af938ec86b00d53d16a408aabc0038fb3e57`
(schema 6). Game target 26.3. Every step ran in a `build-slot.sh` lease
(`CARGO_INCREMENTAL=0`, no `--target-dir` of my own).

## Ladder

| Step | Command | Result |
|---|---|---|
| Runtime tests | `cargo test --locked --offline` | **32 passed**, 0 failed |
| Format | `cargo fmt --all -- --check` | clean |
| Lint | `cargo clippy --all-targets --locked --offline -- -D warnings` | clean |
| Stage | `python3 tools/horizon.py stage` | `stage_changed: true`, 15 injections planned |
| Check | `python3 tools/horizon.py check` | `cargo_exit: 0`, **0 warnings** |
| Embedded tests | `python3 tools/horizon.py test` | `game::compactmachines::tests`: **32 passed**, 0 failed |
| Game build | `python3 tools/horizon.py build` | `cargo_exit: 0`, `target/horizon-cargo/fast/horizon-client` (log: `../_shared/evidence/compactmachines/ladder/horizon-build.log`) |

## Packages

| Artifact | Command | Size | SHA-256 | Entries |
|---|---|---:|---|---:|
| `content/compactmachines.hmod` | `horizon-mod build` + `horizon-mod check` (`no code: content only`) | 27,675 B | `3663ec3b5ff41b06adfddfa36f7d49f6a0887b6586bd25571788ed6b3ba78c08` | 53 |
| `compactmachines.hnmod` | `tools/package.sh` (built content embedded at `mixins/compactmachines/data/`) + `horizon-mod check … --kind native-mod` | 37,922 B | `74a00b8ae852adf3ee0c91dd4f0a387e217d5c759a4bbb7889046124ed93b663` | 20 |

A second `tools/package.sh` run reproduced the `.hnmod` byte for byte.

## In-game (evidence in `../_shared/evidence/compactmachines/`)

| Log | Legs | Verdict |
|---|---|---|
| `e2e-default.log` | control (the same binary, `.horizon-cache` unusable: no machine item, the block stays air) → embedded (no `--dev-mod`; the bundle writes its 27,675 B content, normal room 7³ in a 9³ shell, PSD out, tiny room 3³ in a 5³ shell, registry + relaunch + re-entry) | `PASS: …` `EXIT:0` |
| `e2e-selftest.log` | `SELFTEST=1`: the tiny room is asserted to reach one block further than it does | `FAIL: no tiny wall at 30011,200,28984` `EXIT:1` |
| `hmod-load.log` | the built `.hmod` alone on the shared game, then the same game without it | `PASS: …` `EXIT:0` |

Transcripts of interest (from `e2e-default.log`):

- the embedded content is found, written and loaded:
  `embedded content ready at …/.horizon-cache/compactmachines-embedded-content.hmod`,
  `content_root: dev len 1` — one installable file carries both halves;
- entering and leaving are the deferred console teleport:
  `run execute in minecraft:overworld run tp <uuid> 30008.5 200 30008.5 -> true`
  and, on the way out, `… 0.5 100 2.5 -> true` (the tiny room's site is
  `30008.5 200 28984.5`, the jar's 1024-block grid);
- `/compactmachines info` in the normal room:
  `Machine at 30008 200 30008 is bound to a 7x7x7 size room at <code>`; in the
  tiny room: `… a 3x3x3 size room at <code>`;
- the geometry is asserted from the world, not from the builder: at the arrival,
  `spawn+4`/`spawn+3` are `solid_wall`/air (normal), `spawn+2`/`spawn+1` are
  `solid_wall`/air (tiny), the floor is `solid_wall` at `y-1` and the ceiling at
  `y+7` (normal);
- `<world>/compactmachines/rooms.dat` holds two `room … 1 …` lines after the
  first session and `/compactmachines rooms` reports both after a relaunch.

The shell sizes themselves (386 blocks for 7³, 98 for 3³) are pinned by the
runtime unit tests `a_7_room_has_386_shell_blocks_and_343_interior_blocks` and
`a_3_room_has_5_cubed_minus_27_shell_blocks`, not by the e2e.

## Gates

`python3 ports/tools/check-e2e-logs.py compactmachines --strict`,
`python3 ports/tools/check-skills.py --notes ports/compactmachines`,
`python3 ports/tools/upstream_ready.py compactmachines` — see the run recorded at
the end of this file.

## 0.2.0 — the SDK package ladder (2026-11-18)

The ladder above is the **retired `.hnmod`** one (`tools/horizon.py` and
`tools/package.sh` are gone); it is kept as historical evidence. The 0.2.0
ladder runs directly in the port, inside a `build-slot.sh` lease
(`BUILD_LOCAL=1`, `CARGO_INCREMENTAL=0`, no `--target-dir`, `Cargo.lock`
removed before every build):

| Step | Command | Result |
|---|---|---|
| Runtime tests, no glue yet | `cargo test --offline` on `pub mod runtime;` | **34 passed**, 0 failed |
| Tests | `cargo test --offline` | **38 passed**, 0 failed |
| Lint | `cargo clippy --all-targets --offline -- -D warnings` | exit 0, 0 warnings |
| Format | `cargo fmt --check` | clean (never `--all`: that checks the fetched SDK) |
| Package | `rm -f Cargo.lock && cargo horizon-mod build` | 69,948 B, exit 0 |
| Same bytes twice | the build again | sha256 `a6530b25…f4bf` both times |
| Registry check | `cargo horizon-mod check compactmachines.hmod` | exit 0 |
| Fits | `cargo horizon-mod fits compactmachines.hmod --build nightly` | exit 0 |

Evidence: `../_shared/evidence/compactmachines/sdk-*.log` (+ `sdk-uses.json`,
`sdk-entries.txt`, `sdk-sha256.txt`, `sdk-runtime-sha256.txt`). No 0.2.0 in-game
run exists: `horizon-launcher` is not installed here, so no game carrying the
package can be built — recorded as the port's open gap in `notes.md`.

## 26.3 dialect sweep (2026-11-21) — 0.2.2

| Check | Command | Result |
|---|---|---|
| static dialect | `python3 ports/tools/check_261_dialect.py compactmachines.hmod` | exit 0; pre-fix package exit 1 (`../_shared/evidence/compactmachines/dialect-check-red.log`) |
| tests | `cargo test --offline` | exit 0 (`dialect-test.log`) |
| package | `cargo horizon-mod build` twice after `rm -f Cargo.lock` | `cmp`-identical, built against 0a875da |
| registry check | `cargo horizon-mod check compactmachines.hmod` | **74,833 B**, SHA-256 `0ce22800355b08221d3f8203e737d4c09db0ce2e547fdad59d95f81b7a80c161`, exit 0 |
| fits the pin | `cargo horizon-mod fits compactmachines.hmod --build 0a875da` | `0a875da  made for`, exit 0 |
| the gate | `python3 ports/tools/verify-sdk-package.py compactmachines` | 1/1 |
| in game | `tools/e2e-dialect.sh` | `PASS` + `EXIT:0`; `SELFTEST=1` `FAIL` + `EXIT:1`; `check-e2e-logs.py compactmachines --strict` exit 0 |

Changed: 4 loot tables. Proof: wall crafts 8 and drops 1; pre-fix red. Full record in [`notes.md`](notes.md) §"26.3 dialect sweep".
