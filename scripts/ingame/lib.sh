#!/usr/bin/env bash
# Helpers for the in-game basso tests: they drive a running Horizon (lod-dev
# build with basso) through horizon-ctl. Every test starts on a flat stone
# platform high in the sky, so it doesn't depend on the world.
set -u
CTL=${HORIZON_CTL:-$(ls -d ~/.local/share/horizon-launcher/versions/*/horizon-ctl | tail -1)}
SOCK=${HORIZON_SOCK:-$HOME/.local/share/horizon-launcher/game-control.sock}
Y=${PLATFORM_Y:-200}
PASS=0
FAIL=0

ctl() { "$CTL" --socket "$SOCK" "$@"; }
chat() { ctl chat "$*" >/dev/null; }

# The player's position as "x y z".
pos() { ctl state | jq -r '.player.position | map(floor) | join(" ")'; }
posf() { ctl state | jq -r '.player.position | map(. * 100 | floor / 100) | join(" ")'; }
air() { ctl state | jq -r '.player.air'; }

# A flat stone platform at y=$Y, air above, ores/walls added by each test.
# Respawn when dead, so a failed test doesn't spoil the next.
revive() { [ "$(ctl state | jq -r '.screen')" = dead ] && ctl respawn >/dev/null && ctl wait 20 >/dev/null; true; }

platform() {
    revive
    ctl command "difficulty peaceful" >/dev/null
    local r=${1:-40}
    # fill allows 32768 blocks at a time
    for x in $(seq -$r 20 $r); do
        ctl fill $x $((Y - 1)) -$r $((x + 19)) $((Y - 1)) $r stone >/dev/null
        ctl fill $x $Y -$r $((x + 19)) $((Y + 12)) $r air >/dev/null
    done
}

# `ctl tp` sometimes only acknowledges, so teleport with the command.
tp() { ctl command "tp @s $1 $2 $3" >/dev/null; }

# The newest $1 chat lines, flattened to plain text, one per line.
basso_text() { ctl messages "$1" | jq -r '.styled[] | [.extra[]?.text, .text] | map(select(. != null)) | join("")'; }
block_at() { ctl block "$1" "$2" "$3" | jq -r '.block'; }

# Number of [Basso] chat lines so far, to wait for new ones.
basso_lines() { ctl messages 200 | jq '[.chat[] | select(startswith("[Basso]"))] | length'; }
last_basso() { ctl messages 5 | jq -r '[.chat[] | select(startswith("[Basso]"))][0] // ""'; }

# Run a basso command and wait up to $2 ticks for a chat line containing $3.
# Prints the matching line, returns 1 on timeout.
run_until() {
    local line=$1 ticks=$2 want=$3 waited=0 before
    before=$(basso_text 60 | grep -cF -- "$want")
    chat "/basso $line"
    while [ $waited -lt "$ticks" ]; do
        ctl wait 20 >/dev/null
        waited=$((waited + 20))
        if [ "$(basso_text 60 | grep -cF -- "$want")" -gt "$before" ]; then
            basso_text 60 | grep -F -- "$want" | head -1
            return 0
        fi
    done
    return 1
}

ok() { PASS=$((PASS + 1)); echo "  ok   $1"; }
bad() { FAIL=$((FAIL + 1)); echo "  FAIL $1"; }
check() { # check "name" command (a function of the shell)
    local name=$1
    shift
    if "$@" >/dev/null 2>&1; then ok "$name"; else bad "$name"; fi
}
# near X Z TOL: the player is within TOL blocks (horizontally) of X Z
near() {
    local p
    p=$(posf)
    awk -v p="$p" -v x="$1" -v z="$2" -v t="$3" 'BEGIN{split(p,a," "); dx=a[1]-x; dz=a[3]-z; exit !(dx*dx+dz*dz<=t*t)}'
}
summary() { echo "$PASS passed, $FAIL failed"; [ "$FAIL" -eq 0 ]; }
