#!/usr/bin/env bash
# features: verbose logging, fences (with and without blocks), working with
# the inventory open (bridging and mining), glowstone-style building blocks.
. "$(dirname "$0")/lib.sh"
echo "features"
platform 40
log=~/.local/share/horizon/instances/lod-dev/logs/latest.log
reset() { platform 40; ctl command "gamemode survival" >/dev/null; ctl command "clear @s" >/dev/null; tp 0.5 $Y 0.5; sleep 1; chat "/basso cancel"; }
height() { ctl state | jq -r '.player.position[1]'; }
alive() { awk -v h="$(ctl state | jq -r '.player.health')" 'BEGIN{exit !(h>0)}'; }
go() { run_until "goto $1 $2 $3" ${4:-900} "Arrived" >/dev/null; }
screen() { ctl state | jq -r '.screen'; }

# verbose: levels 0 and 1 differ
reset
chat "/basso set verbose 0"
n=$(wc -l < "$log")
go 8 $Y 0 600
check "verbose 0: no [v1] lines" test "$(tail -n +$((n + 1)) "$log" | grep -c '\[v1\]')" -eq 0
reset
chat "/basso set verbose 1"
n=$(wc -l < "$log")
go 8 $Y 0 600
check "verbose 1: prints the search" test "$(tail -n +$((n + 1)) "$log" | grep -c 'Search over')" -ge 1
chat "/basso set verbose 0"
reset
chat "/basso set verbose 2"
check "verbose is reported" test "$(basso_text 20 | grep -c 'verbose is now 2')" -ge 1
chat "/basso set verbose 0"

# a fence wall, no blocks: around it, never over it
reset
ctl fill 4 $Y -6 4 $Y 6 oak_fence >/dev/null
go 8 $Y 0 1200
check "walks around a fence wall" near 8.5 0.5 2
check "stayed on the ground (not on the fence)" awk -v h="$(height)" -v y=$Y 'BEGIN{exit !(h<y+0.9)}'
alive && ok "alive after the fence" || bad "died at the fence"

# the same wall with blocks in hand and a very long wall: still arrives
reset
ctl fill 4 $Y -30 4 $Y 30 oak_fence >/dev/null
ctl command "give @s minecraft:cobblestone 64" >/dev/null
go 8 $Y 0 2400
check "crosses a long fence wall with blocks" near 8.5 0.5 2
alive && ok "alive after the long fence" || bad "died at the long fence"

# bridging with the inventory open
reset
ctl fill 3 $((Y - 1)) -40 6 $((Y - 1)) 40 air >/dev/null
ctl command "give @s minecraft:cobblestone 64" >/dev/null
sleep 1
ctl open-inventory >/dev/null
sleep 1
go 10 $Y 0 1500
check "bridged a gap with the inventory open" near 10.5 0.5 2
check "the inventory stayed open" test "$(screen)" != playing
ctl close >/dev/null
alive && ok "alive after the bridge" || bad "died at the bridge"

# glowstone as the only block
reset
ctl fill 3 $((Y - 1)) -40 6 $((Y - 1)) 40 air >/dev/null
ctl command "give @s minecraft:glowstone 64" >/dev/null
sleep 1
go 10 $Y 0 1500
check "bridges with glowstone" near 10.5 0.5 2
reset
ctl fill 3 $((Y - 1)) -40 6 $((Y - 1)) 40 air >/dev/null
ctl command "give @s minecraft:diamond_block 64" >/dev/null
sleep 1
chat "/basso set allowPlace true"
run_until "goto 10 $Y 0" 400 "Arrived" >/dev/null
check "does not build with diamond blocks" test "$(block_at 4 $((Y - 1)) 0)" != minecraft:diamond_block
chat "/basso cancel"

# mining with the inventory open
reset
ctl command "give @s minecraft:diamond_pickaxe" >/dev/null
ctl setblock 6 $Y 0 coal_ore >/dev/null
sleep 1
ctl open-inventory >/dev/null
sleep 1
run_until "mine 1 coal_ore" 900 "Mined" >/dev/null
check "mined coal with the inventory open" test "$(block_at 6 $Y 0)" != minecraft:coal_ore
ctl close >/dev/null
summary
