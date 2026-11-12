#!/usr/bin/env bash
# place: bridging over a void, pillaring up to a goal in the air, and the
# switches that turn it off.
. "$(dirname "$0")/lib.sh"
echo "place"
platform 40
ctl command "gamemode survival" >/dev/null
chat "/basso cancel"
health() { ctl state | jq -r '.player.health'; }
alive() { awk -v h="$(health)" 'BEGIN{exit !(h>0)}'; }
void() { ctl fill "$1" $((Y - 1)) "$2" "$3" $((Y - 1)) "$4" air >/dev/null; }
give_blocks() { ctl command "clear @s" >/dev/null; ctl command "give @s minecraft:cobblestone 64" >/dev/null; }
count_stone() { ctl inventory | jq -r '.counts["minecraft:cobblestone"] // 0'; }

# a void strip of 6 blocks
give_blocks
void 1 -40 8 40
tp 0.5 $Y 0.5
sleep 1
run_until "goto 10 $Y 0" 1200 "Arrived" >/dev/null
check "bridges a 8 block void" near 10.5 0.5 2
bridge_blocks() { for x in 1 2 3 4 5 6 7 8; do block_at $x $((Y - 1)) 0; done | grep -c cobblestone; }
check "the bridge is made of the carried blocks" test "$(bridge_blocks)" -ge 3
alive && ok "alive after bridging" || bad "died bridging"
check "the bridge used blocks" test "$(count_stone)" -lt 64
ctl fill 1 $((Y - 1)) -40 8 $((Y - 1)) 40 stone >/dev/null

# a goal in the air, 6 up
give_blocks
tp 20.5 $Y 20.5
sleep 1
run_until "goto 20 $((Y + 6)) 20" 1200 "Arrived" >/dev/null
check "pillars up to a goal in the air" near 20.5 20.5 1
check "stands at the goal height" test "$(ctl state | jq -r '.player.position[1] | floor')" -ge $((Y + 6))
alive && ok "alive after pillaring" || bad "died pillaring"

# goal in the air and over the void: parkour/bridge/pillar mixed
give_blocks
void 30 -40 45 40
tp 25.5 $Y 0.5
sleep 1
run_until "goto 40 $((Y + 3)) 0" 1500 "Arrived" >/dev/null
f0=$FAIL
check "builds across and up" near 40.5 0.5 2
[ "$FAIL" -gt "$f0" ] && echo "    ended at $(posf): $(last_basso)"

# no blocks: no building
ctl command "clear @s" >/dev/null
tp 0.5 $Y 30.5
void 3 -40 6 40
sleep 1
chat "/basso goto 8 $Y 30"
ctl wait 200 >/dev/null
check "without blocks it does not walk off the edge" near 2.5 30.5 3
alive && ok "alive without blocks" || bad "died without blocks"
chat "/basso cancel"

# allowPlace off with blocks
give_blocks
chat "/basso set allowPlace false"
chat "/basso goto 8 $Y 30"
ctl wait 200 >/dev/null
check "allowPlace false does not build" near 2.5 30.5 3
chat "/basso set allowPlace true"
chat "/basso cancel"
# out of blocks half way over a void: it must stand on what it built
platform 40; ctl command "gamemode survival" >/dev/null
ctl command "clear @s" >/dev/null
ctl command "give @s minecraft:cobblestone 3" >/dev/null
void 1 -40 12 40
tp 0.5 $Y 0.5; sleep 1
chat "/basso goto 14 $Y 0"
ctl wait 400 >/dev/null
alive && ok "alive when the blocks run out" || bad "died when the blocks run out"
check "it stayed on what it built" test "$(ctl state | jq -r '.player.position[1] | floor')" -ge "$Y"
chat "/basso cancel"

# a gap too wide to jump and no blocks: it stays
platform 40; ctl command "gamemode survival" >/dev/null
ctl command "clear @s" >/dev/null
void 1 -40 4 40
tp 0.5 $Y 0.5; sleep 1
chat "/basso goto 8 $Y 0"
ctl wait 300 >/dev/null
alive && ok "alive at a wide gap" || bad "died at a wide gap"
check "does not jump a 4 block gap" near 0.5 0.5 1.5
chat "/basso cancel"

# a ceiling over the pillar: it must not try to go through it
platform 40; ctl command "gamemode survival" >/dev/null
ctl command "clear @s" >/dev/null
ctl command "give @s minecraft:cobblestone 64" >/dev/null
ctl fill 18 $((Y + 3)) 18 22 $((Y + 3)) 22 stone >/dev/null
tp 20.5 $Y 20.5; sleep 1
chat "/basso goto 20 $((Y + 5)) 20"
ctl wait 300 >/dev/null
alive && ok "alive under a ceiling" || bad "died under a ceiling"
chat "/basso cancel"

ctl command "gamemode creative" >/dev/null
summary
