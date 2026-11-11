#!/usr/bin/env bash
# Bounded launch probe: does the game come up with this port's bundle installed?
# Prints the launch result, the control replies and the game's own log, so a
# hang at launch is diagnosed from evidence instead of guessed at.
set -uo pipefail
CLONE=${CAMPAIGN_ROOT:-$(cd "$(dirname "$0")/../../.." && pwd)}
PORT=$CLONE/ports/compactmachines
PIN_BIN=$CLONE/ports/_shared/bin/horizon-1d63e0a5
CTL=${CTL:-$PIN_BIN/horizon-ctl}
GAME=${GAME:-$PORT/target/horizon-cargo/fast/horizon-client}
RES=${RES:-${HORIZON_RES:-${HORIZON_RES:-$HOME/.local/horizon-client/26.3}}}
SOCK=${SOCK:-/tmp/opencode/compactmachines-probe.sock}
DEV=${DEV:-1}

[ -x "$CTL" ] || { echo "missing ctl: $CTL"; exit 1; }
[ -x "$GAME" ] || { echo "missing game: $GAME"; exit 1; }
echo "ctl=$CTL"
echo "game=$GAME"
echo "dev-mod=$DEV"

SCRATCH=$(mktemp -d /tmp/opencode/compactmachines-probe.XXXXXX)
export HORIZON_HOME=$SCRATCH/game
export HORIZON_LAUNCHER_HOME=$SCRATCH/launcher
export XDG_DATA_HOME=$SCRATCH/data
export XDG_CONFIG_HOME=$SCRATCH/config
export HORIZON_NO_AUDIO=1
mkdir -p "$(dirname "$SOCK")" "$XDG_CONFIG_HOME"
rm -f "$SOCK"
cd "$SCRATCH"

DEV_ARGS=()
[ "$DEV" = 1 ] && DEV_ARGS=(--dev-mod "$PORT/content")

timeout 300 "$CTL" --socket "$SOCK" launch \
    --game "$GAME" --world CMProbe --seed 20261105 --creative --offscreen --wait \
    --resources "$RES" "${DEV_ARGS[@]}"
echo "launch rc=$?"

timeout 30 "$CTL" --socket "$SOCK" hello
echo "hello rc=$?"
timeout 60 "$CTL" --socket "$SOCK" wait-ready 120
echo "wait-ready rc=$?"
timeout 60 "$CTL" --socket "$SOCK" command "execute in minecraft:overworld run setblock 0 100 0 minecraft:stone"
echo "stone setblock rc=$?"
timeout 30 "$CTL" --socket "$SOCK" wait 4 >/dev/null
timeout 30 "$CTL" --socket "$SOCK" block 0 100 0
echo "stone block rc=$?"
timeout 60 "$CTL" --socket "$SOCK" command "execute in minecraft:overworld run setblock 0 100 0 compactmachines:new_machine"
echo "setblock rc=$?"
timeout 30 "$CTL" --socket "$SOCK" wait 4 >/dev/null
timeout 30 "$CTL" --socket "$SOCK" block 0 100 0
echo "block rc=$?"
timeout 60 "$CTL" --socket "$SOCK" command "item replace entity @s hotbar.0 with compactmachines:new_machine 1"
echo "item rc=$?"
timeout 30 "$CTL" --socket "$SOCK" wait 4 >/dev/null
timeout 30 "$CTL" --socket "$SOCK" inventory | head -c 600
echo
echo "inventory rc=$?"

echo "== give =="
timeout 30 "$CTL" --socket "$SOCK" give compactmachines:personal_shrinking_device 1
echo "give psd rc=$?"
timeout 30 "$CTL" --socket "$SOCK" give compactmachines:personal_shrinking_device 1
echo "give psd twice rc=$?"
timeout 30 "$CTL" --socket "$SOCK" give compactmachines:new_machine 1
echo "give machine rc=$?"
timeout 30 "$CTL" --socket "$SOCK" select compactmachines:personal_shrinking_device
echo "select psd rc=$?"
timeout 30 "$CTL" --socket "$SOCK" inventory | jq -c '{counts: .counts, held: .held}'
echo "== cache =="
ls -l "$SCRATCH/.horizon-cache" 2>&1 || true
echo "== game log tail =="
tail -40 "$SCRATCH/logs/latest.log" 2>/dev/null || tail -40 "$HORIZON_HOME/logs/latest.log" 2>/dev/null || echo "no latest.log"
timeout 20 "$CTL" --socket "$SOCK" quit >/dev/null 2>&1 || true
sleep 1
rm -rf "$SCRATCH"
