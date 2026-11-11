#!/usr/bin/env bash
# Proves the built content package (content/compactmachines.hmod) loads on a
# real game: the shared, unmodded game with `--dev-mod <the .hmod>` must
# register the machine block and the PSD item. A second launch WITHOUT
# `--dev-mod` is the negative control: the same ids must be unknown there, so
# the check cannot pass vacuously.
set -euo pipefail
export LC_ALL=C

CLONE=${CAMPAIGN_ROOT:-$(cd "$(dirname "$0")/../../.." && pwd)}
PIN_BIN=$CLONE/ports/_shared/bin/horizon-1d63e0a5
CTL=${CTL:-$PIN_BIN/horizon-ctl}
GAME=${GAME:-$PIN_BIN/horizon-client}
HMOD=${HMOD:-$CLONE/ports/compactmachines/content/compactmachines.hmod}
RES=${RES:-${HORIZON_RES:-${HORIZON_RES:-$HOME/.local/horizon-client/26.3}}}
SOCK=${SOCK:-/tmp/opencode/compactmachines-hmod.sock}

[ -x "$CTL" ] || { echo "missing horizon-ctl: $CTL" >&2; exit 1; }
[ -x "$GAME" ] || { echo "missing horizon-client: $GAME" >&2; exit 1; }
[ -f "$HMOD" ] || { echo "missing package: $HMOD" >&2; exit 1; }
[ -d "$RES/assets" ] || { echo "missing prepared resources: $RES" >&2; exit 1; }

SCRATCH=$(mktemp -d /tmp/opencode/compactmachines-hmod-home.XXXXXX)
export HORIZON_HOME=$SCRATCH/game
export HORIZON_LAUNCHER_HOME=$SCRATCH/launcher
export XDG_DATA_HOME=$SCRATCH/data
export XDG_CONFIG_HOME=$SCRATCH/config
export HORIZON_NO_AUDIO=1
mkdir -p "$(dirname "$SOCK")" "$XDG_CONFIG_HOME"
rm -f "$SOCK"

ctl() { "$CTL" --socket "$SOCK" "$@"; }
fail() { echo "FAIL: $*" >&2; ctl logs --lines 60 >&2 || true; exit 1; }

cleanup() {
    local status=$?
    ctl quit >/dev/null 2>&1 || true
    sleep 1
    rm -rf "$SCRATCH" 2>/dev/null || true
    echo "EXIT:$status"
}
trap cleanup EXIT

launch() { # launch [extra args…]
    rm -f "$SOCK"
    cd "$SCRATCH"
    "$CTL" --socket "$SOCK" launch --game "$GAME" --world CMHmod --seed 20261105 \
        --creative --headless --wait --resources "$RES" "$@" >/dev/null
    ctl wait-ready 120 >/dev/null
    ctl wait 20 >/dev/null
}

echo "== the content package on the shared game =="
launch --dev-mod "$HMOD"
ctl command "execute in minecraft:overworld run setblock 0 100 0 compactmachines:new_machine" >/dev/null
ctl wait 4 >/dev/null
block=$(ctl block 0 100 0 | jq -r '.block // .name // "?"')
[ "$block" = "compactmachines:new_machine" ] || fail "the package did not register the machine block: got $block"
ctl give compactmachines:personal_shrinking_device 1 >/dev/null
ctl wait 4 >/dev/null
psd=$(ctl inventory | jq -r '.counts["compactmachines:personal_shrinking_device"] // 0')
[ "$psd" -ge 1 ] || fail "the package did not register the PSD item"
echo "  machine block: $block; PSD in the inventory: $psd"
ctl quit >/dev/null 2>&1 || true
sleep 2

echo "== the control: the same game without the package =="
rm -f "$SOCK"
cd "$SCRATCH"
"$CTL" --socket "$SOCK" launch --game "$GAME" --world CMHmodControl --seed 20261105 \
    --creative --headless --wait --resources "$RES" >/dev/null
ctl wait-ready 120 >/dev/null
ctl wait 20 >/dev/null
ctl command "execute in minecraft:overworld run setblock 0 100 0 compactmachines:new_machine" >/dev/null || true
ctl wait 4 >/dev/null
block=$(ctl block 0 100 0 | jq -r '.block // .name // "?"')
[ "$block" = "minecraft:air" ] || fail "the unmodded game accepted compactmachines:new_machine (the check is vacuous): $block"
ctl give compactmachines:personal_shrinking_device 1 >/dev/null || true
ctl wait 4 >/dev/null
psd=$(ctl inventory | jq -r '.counts["compactmachines:personal_shrinking_device"] // 0')
[ "$psd" = 0 ] || fail "the unmodded game gave a PSD (the check is vacuous)"
echo "  without the package: the block is $block and no PSD exists"

echo "PASS: compactmachines.hmod registered the machine block and the PSD on a real game, and neither exists without the package"
