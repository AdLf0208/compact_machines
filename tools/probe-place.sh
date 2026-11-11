#!/usr/bin/env bash
# Bounded placement probe: does `ctl place` work at all in this arena, and does
# it place this mod's machine block? Prints every reply and exit code.
set -uo pipefail
CLONE=${CAMPAIGN_ROOT:-$(cd "$(dirname "$0")/../../.." && pwd)}
PORT=$CLONE/ports/compactmachines
PIN_BIN=$CLONE/ports/_shared/bin/horizon-1d63e0a5
CTL=${CTL:-$PIN_BIN/horizon-ctl}
GAME=${GAME:-$PORT/target/horizon-cargo/fast/horizon-client}
RES=${RES:-${HORIZON_RES:-${HORIZON_RES:-$HOME/.local/horizon-client/26.3}}}
SOCK=${SOCK:-/tmp/opencode/compactmachines-place.sock}
SCRATCH=$(mktemp -d /tmp/opencode/compactmachines-place.XXXXXX)
export HORIZON_HOME=$SCRATCH/game HORIZON_LAUNCHER_HOME=$SCRATCH/launcher
export XDG_DATA_HOME=$SCRATCH/data XDG_CONFIG_HOME=$SCRATCH/config HORIZON_NO_AUDIO=1
mkdir -p "$(dirname "$SOCK")" "$XDG_CONFIG_HOME"
rm -f "$SOCK"
cd "$SCRATCH"
ctl() { "$CTL" --socket "$SOCK" "$@"; }

timeout 300 "$CTL" --socket "$SOCK" launch --game "$GAME" --world CMPlace --seed 20261105 \
    --creative --offscreen --wait --resources "$RES" >/dev/null
echo "launch rc=$?"
ctl wait-ready 120 >/dev/null
echo "wait-ready rc=$?"
ctl command "execute in minecraft:overworld run fill -4 99 -4 8 99 8 minecraft:stone" >/dev/null
ctl command "execute in minecraft:overworld run fill -4 100 -4 8 108 8 minecraft:air" >/dev/null
ctl command "execute in minecraft:overworld run tp @s 0.5 100 2.5" >/dev/null
ctl wait 10 >/dev/null

echo "== vanilla stone =="
ctl command "item replace entity @s hotbar.0 with minecraft:stone 4" >/dev/null
ctl wait 4 >/dev/null
ctl select minecraft:stone
echo "select stone rc=$?"
ctl look-at-block 0 99 0 up >/dev/null
ctl wait 2 >/dev/null
ctl place 0 100 0
echo "place stone rc=$?"
ctl wait 4 >/dev/null
ctl block 0 100 0

echo "== the machine =="
ctl chat "/compactmachines give new compactmachines:normal"
ctl wait 6 >/dev/null
ctl messages 6 | jq -c '.chat'
ctl inventory | jq -c '{counts: .counts, held: .held, hotbar: [.hotbar[] | select(.item != null)]}'
ctl select compactmachines:new_machine
echo "select machine rc=$?"
ctl state | jq -c '{screen: .screen, held: .held, player: .player.position}'
ctl look-at-block 0 99 0 up >/dev/null
ctl wait 2 >/dev/null
ctl place 0 100 0
echo "place machine rc=$?"
ctl wait 6 >/dev/null
ctl block 0 100 0
ctl logs --lines 20
timeout 20 "$CTL" --socket "$SOCK" quit >/dev/null 2>&1 || true
sleep 1
rm -rf "$SCRATCH"
