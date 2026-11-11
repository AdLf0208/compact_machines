#!/usr/bin/env bash
# In-game check for Compact Machines slice 1: the machine, its room and the PSD.
#
# The Overworld arena at y=100 (stone floor at y=99). Every mod command is
# typed with `ctl chat "/cmd"` because `ctl command` runs the console path and
# bypasses the native `perform_command` hook.
#
# Default run (EMBEDDED=1, no --dev-mod): the delivered `.hnmod` carries its
# content, the helper writes it to `.horizon-cache/`, and:
#  - `/compactmachines give new compactmachines:normal` gives a new machine;
#  - placing it and using the PSD on it creates a room and moves the player to
#    the room's spawn: a 9x9x9 `solid_wall` shell with 7x7x7 of air;
#  - `ctl use-item` with the PSD inside teleports the player back to the
#    machine;
#  - a second machine from `compactmachines:tiny` makes a 5x5x5 shell (3x3x3
#    of air), so the jar's scaling numbers really scale;
#  - the registry file survives a relaunch and the room can be re-entered.
#
# CONTROL=1 first runs the same binary with `.horizon-cache` unusable: the
# content ids must be unknown there (the check is not vacuous).
# SELFTEST=1 asserts the tiny room is 7 wide instead of 3, and must exit
# non-zero.
set -euo pipefail
export LC_ALL=C

CLONE=${CAMPAIGN_ROOT:-$(cd "$(dirname "$0")/../../.." && pwd)}
PORT=$CLONE/ports/compactmachines
PIN_BIN=$CLONE/ports/_shared/bin/horizon-1d63e0a5
CTL=${CTL:-$PIN_BIN/horizon-ctl}
GAME=${GAME:-$PORT/target/horizon-cargo/fast/horizon-client}
RES=${RES:-${HORIZON_RES:-${HORIZON_RES:-$HOME/.local/horizon-client/26.3}}}
SOCK=${SOCK:-/tmp/opencode/compactmachines-e2e.sock}
SELFTEST=${SELFTEST:-0}
KEEP=${KEEP:-0}
WORLD=CompactMachinesE2E
CONTROL_WORLD=CompactMachinesControl
EMBEDDED=${EMBEDDED:-1}
CONTROL=${CONTROL:-$EMBEDDED}
DEV_MOD=()
[ "$EMBEDDED" = 1 ] || DEV_MOD=(--dev-mod "$PORT/content")

[ -x "$CTL" ] || { echo "missing horizon-ctl: $CTL" >&2; exit 1; }
[ -x "$GAME" ] || { echo "missing horizon-client: $GAME" >&2; exit 1; }
[ -d "$RES/assets" ] || { echo "missing prepared resources: $RES" >&2; exit 1; }

SCRATCH=$(mktemp -d /tmp/opencode/compactmachines-home.XXXXXX)
export HORIZON_HOME=$SCRATCH/game
export HORIZON_LAUNCHER_HOME=$SCRATCH/launcher
export XDG_DATA_HOME=$SCRATCH/data
export XDG_CONFIG_HOME=$SCRATCH/config
export HORIZON_NO_AUDIO=1
mkdir -p "$(dirname "$SOCK")" "$XDG_CONFIG_HOME"
rm -f "$SOCK"

ctl() { "$CTL" --socket "$SOCK" "$@"; }
say() { ctl chat "/$1" >/dev/null; }
fail() { echo "FAIL: $*" >&2; ctl logs --lines 80 >&2 || true; exit 1; }

# The player's feet, as "x y z" rounded down, from `ctl state`.
feet() {
    ctl state | jq -r '.player | "\(.position[0] | floor) \(.position[1] | floor) \(.position[2] | floor)"'
}
dimension() { ctl state | jq -r '.dimension // ""'; }
block_name() { ctl block "$1" "$2" "$3" | jq -r '.name // .block // "?"'; }
expect_block() { # expect_block X Y Z NAME
    local got
    got=$(block_name "$1" "$2" "$3")
    [ "$got" = "$4" ] || fail "expected $4 at $1,$2,$3; got $got"
    echo "  block($1,$2,$3) = $4: ok"
}

# Wait until the block at X Y Z is NAME (the shell is placed after arrival).
wait_block() { # wait_block X Y Z NAME
    local i got
    for i in $(seq 1 60); do
        got=$(block_name "$1" "$2" "$3")
        [ "$got" = "$4" ] && return 0
        ctl wait 5 >/dev/null
    done
    echo "last block($1,$2,$3) = $(block_name "$1" "$2" "$3")" >&2
    return 1
}

# wait_message PATTERN: wait until a chat line matches and print that line
# (a command's reply arrives a few ticks after the chat line is sent).
wait_message() {
    local i lines
    for i in $(seq 1 40); do
        lines=$(ctl messages 12 | jq -r '.chat[]?')
        if grep -q -- "$1" <<<"$lines"; then
            grep -m1 -- "$1" <<<"$lines"
            return 0
        fi
        ctl wait 2 >/dev/null || true
    done
    return 1
}

# The room code inside a line.
code_of() {
    grep -o '[0-9A-Z]\{4\}-[0-9A-Z]\{4\}-[0-9A-Z]\{4\}' <<<"$1" | tail -1
}

cleanup() {
    local status=$?
    ctl quit >/dev/null 2>&1 || true
    sleep 1
    if [ "$KEEP" = 1 ]; then
        echo "kept scratch: $SCRATCH" >&2
    else
        rm -rf "$SCRATCH" 2>/dev/null || true
    fi
    # Last line of every run: `check-e2e-logs.py` reads it as the verdict.
    echo "EXIT:$status"
}
trap cleanup EXIT

launch() { # launch [WORLD]
    local world=${1:-$WORLD}
    rm -f "$SOCK"
    cd "$SCRATCH"
    if [ "$EMBEDDED" = 1 ]; then
        echo "  launch: no --dev-mod; the bundle carries its own content" >&2
    fi
    "$CTL" --socket "$SOCK" launch \
        --game "$GAME" --world "$world" --seed 20261105 --creative --offscreen --wait \
        --resources "$RES" "${DEV_MOD[@]}" >/dev/null
    ctl wait-ready 120
    ctl wait 40
}

# The first launch only: a stone floor and a clear volume in the Overworld.
# The floor goes in before the player arrives, so they stand on it (a player
# teleported into air falls to the terrain far below, out of reach).
arena() {
    ctl command "execute in minecraft:overworld run fill -8 99 -8 24 99 24 minecraft:stone"
    ctl command "execute in minecraft:overworld run fill -8 100 -8 24 110 24 minecraft:air"
    ctl command "execute in minecraft:overworld run tp @s 0.5 100 2.5"
    local loaded=false
    for _ in $(seq 1 120); do
        ctl wait 5
        if [ "$(ctl block 0 100 0 | jq -r '.loaded // false')" = true ]; then
            loaded=true
            break
        fi
    done
    [ "$loaded" = true ] || fail "the Overworld arena chunk never loaded"
    ctl command "execute in minecraft:overworld run kill @e[type=!player]"
    ctl wait 20
}

# The control for EMBEDDED=1: the same binary (every hook is in it) with the
# embedded content made unavailable — `.horizon-cache` is a plain file, so the
# helper can neither compare nor write its content file and falls back to
# `Continue`. Both ids must then be unknown.
control_leg() {
    echo "== control: the same game with no embedded content file =="
    rm -rf "$SCRATCH/.horizon-cache"
    : > "$SCRATCH/.horizon-cache"
    launch "$CONTROL_WORLD"
    arena
    ctl command "item replace entity @s hotbar.0 with compactmachines:new_machine 1" >/dev/null || true
    ctl wait 4
    ctl inventory | jq -e --arg id "compactmachines:new_machine" \
        '[.hotbar[], .main[] | select(.item == $id)] | length == 0' >/dev/null \
        || fail "the control game has the machine item, so the content did not come from the embedded file"
    ctl command "execute in minecraft:overworld run setblock 20 100 0 compactmachines:new_machine" >/dev/null || true
    ctl wait 4
    local name
    name=$(block_name 20 100 0)
    [ "$name" = "minecraft:air" ] || fail "the control game holds $name, so the block id was known"
    [ -f "$SCRATCH/.horizon-cache" ] || fail "the control's .horizon-cache was replaced: the helper wrote its content"
    echo "  without the embedded file: no machine item, the block stayed $name"
    ctl quit >/dev/null 2>&1 || true
    sleep 2
    # The game keeps its own cache inside `.horizon-cache`; remove whatever is
    # left (a file here, possibly a directory again) before the next launch.
    rm -rf "$SCRATCH/.horizon-cache"
    [ ! -e "$SCRATCH/.horizon-cache" ] || fail "the control's .horizon-cache survived"
}

# EMBEDDED=1: the bundle must have written its content out at startup, byte for
# byte the delivered content mod.
embedded_content_check() {
    local cache=$SCRATCH/.horizon-cache/compactmachines-embedded-content.hmod
    [ -f "$cache" ] || fail "the bundle did not write its embedded content to $cache"
    cmp -s "$cache" "$PORT/content/compactmachines.hmod" \
        || fail "the embedded content differs from content/compactmachines.hmod"
    echo "  embedded content: $(wc -c < "$cache" | tr -d ' ') B at $cache, identical to content/compactmachines.hmod"
}

# A machine placed at X Y Z from the hotbar, its id checked.
place_machine() { # place_machine X Y Z TEMPLATE
    # The harness's placement keeps the held stack, so clear any earlier
    # machine first: `select` takes the first matching slot and would place the
    # previous room's template again.
    ctl command "clear @s compactmachines:new_machine" >/dev/null || true
    ctl wait 4
    say "compactmachines give new compactmachines:$4"
    ctl wait 6
    ctl inventory | jq -e --arg id "compactmachines:new_machine" \
        '[.hotbar[], .main[] | select(.item == $id)] | length == 1' >/dev/null \
        || fail "expected exactly one new_machine after /compactmachines give new compactmachines:$4: $(ctl inventory | jq -c '{counts, hotbar, main}')"
    ctl select compactmachines:new_machine >/dev/null \
        || fail "the new_machine is not in the hotbar: $(ctl inventory | jq -c '{counts, hotbar, main}')"
    ctl wait 4
    # Stand next to the spot, as a player placing a block does.
    ctl command "execute in minecraft:overworld run tp @s $1.5 100 $(($3 + 2)).5" >/dev/null
    ctl wait 4
    # `ctl place` needs a visible face: aim at the floor under the target.
    ctl look-at-block "$1" "$(($2 - 1))" "$3" up >/dev/null \
        || fail "cannot aim at the floor under $1,$2,$3"
    ctl wait 2
    ctl place "$1" "$2" "$3" >/dev/null || fail "cannot place the machine at $1,$2,$3"
    ctl wait 6
    expect_block "$1" "$2" "$3" "compactmachines:new_machine"
}

# Use the PSD on the machine at X Y Z from the given standing spot, then wait
# for the arrival.
enter_room() { # enter_room MACHINE_X MACHINE_Y MACHINE_Z STAND_X STAND_Z
    local cx=$1 cy=$2 cz=$3 sx=$4 sz=$5
    ctl command "execute in minecraft:overworld run tp @s $sx.5 100 $sz.5"
    ctl wait 6
    ctl give compactmachines:personal_shrinking_device 1 >/dev/null \
        || fail "the harness could not give the PSD: $(ctl inventory | jq -c '{counts, hotbar}')"
    ctl wait 4
    ctl select compactmachines:personal_shrinking_device >/dev/null \
        || fail "the PSD is not in the hotbar: $(ctl inventory | jq -c '{counts, hotbar, main}')"
    ctl interact "$cx" "$cy" "$cz" >/dev/null || fail "the PSD click on $cx,$cy,$cz failed"
    ctl wait 30
}

echo "== launch (Overworld arena, the bundle's own content) =="
if [ "$CONTROL" = 1 ]; then control_leg; fi
launch
if [ "$EMBEDDED" = 1 ]; then embedded_content_check; fi
arena
[ "$(dimension)" = "minecraft:overworld" ] || fail "the player is not in the Overworld"

echo "== a new machine, a room of the jar's normal size (7x7x7) =="
place_machine 0 100 0 normal
say "compactmachines info"
outside=$(wait_message "not in a room") \
    || fail "/compactmachines info did not say the player is outside a room: $(ctl messages 8 | jq -c '.chat')"
echo "  info: $outside"
enter_room 0 100 0 0 2
read -r x y z <<<"$(feet)"
echo "  arrived at $x $y $z in $(dimension)"
[ "$(dimension)" = "minecraft:overworld" ] || fail "the room is not in the Overworld: $(dimension)"
[ "$y" -ge 199 ] && [ "$y" -le 202 ] || fail "arrived at y=$y, expected the room floor at y=200"
wait_block $((x + 4)) 200 "$z" compactmachines:solid_wall || fail "no wall 4 blocks east of the spawn"
wait_block $((x + 3)) 200 "$z" minecraft:air || fail "the interior 3 east of the spawn is not air"
wait_block "$x" 199 "$z" compactmachines:solid_wall || fail "no floor under the spawn"
wait_block "$x" 207 "$z" compactmachines:solid_wall || fail "no ceiling 7 above the floor"
normal_spawn="$x $z"
say "compactmachines info"
info_line=$(wait_message "7x7x7") \
    || fail "/compactmachines info gave no 7x7x7 room line: $(ctl messages 8 | jq -c '.chat')"
echo "  info: $info_line"
case "$info_line" in
    *"7x7x7"*) ;;
    *) fail "/compactmachines info did not name the 7x7x7 room: '$info_line'" ;;
esac
normal_code=$(code_of "$info_line")
echo "  normal room code: $normal_code"

echo "== the PSD inside the room leaves it, back at the machine =="
ctl use-item >/dev/null || fail "the PSD use in the room failed"
ctl wait 30
read -r x y z <<<"$(feet)"
echo "  left to $x $y $z"
[ "$(dimension)" = "minecraft:overworld" ] || fail "leaving the room left the Overworld"
[ "$y" -ge 99 ] && [ "$y" -le 102 ] || fail "left to y=$y, expected the machine at y=100"
[ "$x" -ge -2 ] && [ "$x" -le 2 ] || fail "left to x=$x, expected the machine at x=0"

echo "== a second machine, a room of the jar's tiny size (3x3x3) =="
place_machine 6 100 0 tiny
enter_room 6 100 0 6 2
read -r x y z <<<"$(feet)"
echo "  tiny arrived at $x $y $z"
# A 3x3x3 interior centred on the spawn is air at +1 and wall at +2.
tiny_air=$((x + 1))
tiny_wall=$((x + 2))
if [ "$SELFTEST" = 1 ]; then
    # Deliberately wrong: the tiny room is 3 wide, so +3 is air outside it.
    echo "(selftest: asserting the tiny room reaches $((x + 3)), which it does not)"
    tiny_wall=$((x + 3))
fi
wait_block "$tiny_wall" 200 "$z" compactmachines:solid_wall \
    || fail "no tiny wall at $tiny_wall,200,$z"
wait_block "$tiny_air" 200 "$z" minecraft:air \
    || fail "the tiny interior at $tiny_air,200,$z is not air"
say "compactmachines info"
tiny_line=$(wait_message "3x3x3") \
    || fail "/compactmachines info gave no 3x3x3 room line: $(ctl messages 8 | jq -c '.chat')"
echo "  info: $tiny_line"
case "$tiny_line" in
    *"3x3x3"*) ;;
    *) fail "/compactmachines info did not name the 3x3x3 room: '$tiny_line'" ;;
esac
tiny_code=$(code_of "$tiny_line")
ctl use-item >/dev/null || fail "the PSD use in the tiny room failed"
ctl wait 30

echo "== the registry survives a relaunch, and a room can be re-entered =="
ctl wait 40
STORE="$SCRATCH/saves/$WORLD/compactmachines/rooms.dat"
[ -s "$STORE" ] || fail "missing room registry $STORE"
rooms=$(grep -c '^room ' "$STORE" || true)
echo "  store: $rooms rooms, codes $(grep '^room ' "$STORE" | awk '{print $2}' | tr '\n' ' ')"
[ "$rooms" = 2 ] || fail "the store holds $rooms rooms, expected 2"
grep -q "room $normal_code normal 1" "$STORE" || fail "the normal room is not in the store as built"
grep -q "room $tiny_code tiny 1" "$STORE" || fail "the tiny room is not in the store as built"
ctl save >/dev/null
ctl quit >/dev/null 2>&1 || true
sleep 3
launch
arena
say "compactmachines rooms"
rooms_line=$(wait_message "Number of registered rooms") \
    || fail "after relaunch /compactmachines rooms gave no count: $(ctl messages 8 | jq -c '.chat')"
echo "  after relaunch: $rooms_line"
case "$rooms_line" in
    *": 2"*) ;;
    *) fail "after relaunch the registry does not hold 2 rooms: '$rooms_line'" ;;
esac
say "compactmachines teleport $tiny_code"
ctl wait 40
read -r x y z <<<"$(feet)"
echo "  re-entered the tiny room at $x $y $z"
[ "$y" -ge 199 ] && [ "$y" -le 202 ] || fail "re-entry arrived at y=$y, expected the room floor"
wait_block $((x + 2)) 200 "$z" compactmachines:solid_wall || fail "the tiny room was not rebuilt"
ctl use-item >/dev/null || fail "the PSD use after the relaunch failed"
ctl wait 30

echo "== the mechanism, from the game log =="
ctl logs --lines 600 | grep "\[compactmachines\]" | tail -20 || true

echo "PASS: a new machine made a 7x7x7 room in a 9x9x9 shell, the PSD entered and left it, a tiny machine made a 3x3x3 room in a 5x5x5 shell, and both rooms and their bindings survived a relaunch"
