# compactmachines (Horizon port)

A Horizon port of [compactmachines](CompactMods/CompactMachines) — MIT.

**Status: first slices.** What is below is what the port actually does today.

## Implemented so far

- | Template | Interior | Colour | Shell |
- |---|---|---|---|
- | tiny | 3 × 3 × 3 | `#C95B13` | 5³ (98 blocks) |
- | small | 5 × 5 × 5 | `#D4D2D2` | 7³ |
- | normal | 7 × 7 × 7 | `#FBF236` | 9³ (386 blocks) |
- | large | 9 × 9 × 9 | `#211B2E` | 11³ |
- | giant | 11 × 11 × 11 | `#43D6CD` | 13³ |
- | colossal | 13 × 13 × 13 | `#423F42` | 15³ |

## Not yet ported

- | Gap | Why | What the slice does instead |
- |---|---|---|
- | **No player-position write** | the pin has no hook that moves a player; `native_run_command` is refused inside player hooks | the deferred `execute in <dim> run tp` from `server_tick` (the `waystones` pattern) |
- | **No teleport veto** | `entity_teleport` is listen-only, so a non-command teleport out of a room cannot be cancelled | the shell is `solid_wall` (unbreakable), which keeps players in by geometry |
- | Preview screen, upgrade screen, keybinds, Jade/JEI/Curios, villager, gamerules, OOB damage | client half and integrations | `/compactmachines info` replaces the preview; the rest is deferred |
- | PSD "set room spawn" (sneaking) | the slice has one spawn per room (`IRoomBoundaries.defaultSpawn`) | sneak-use is not special-cased; the room spawn is its centre |

## Install

Build the bundle with a native-mod capable launcher, or add the file from the
registry once it is listed. Pinned to Horizon `1d63e0a` and horizon-mixins
`5a64af93` for game 26.3.

## Build from source

```sh
cargo test
python3 tools/horizon.py check --horizon <horizon checkout>
python3 tools/horizon.py build --horizon <horizon checkout>
python3 tools/package.py
```

## Credits and licence

Original mod: CompactMods/CompactMachines. Port: AdLf0208. Licence: MIT — see [LICENSE](LICENSE);
the original notices are preserved.
