#!/usr/bin/env bash
# edge: tunnels, shafts, a ledge over a void, sealed rooms, a ladder, deep water.
. "$(dirname "$0")/lib.sh"
echo "edge"
ctl command "gamemode survival" >/dev/null
chat "/basso cancel"
health() { ctl state | jq -r '.player.health'; }
alive() { awk -v h="$(health)" 'BEGIN{exit !(h>0)}'; }
give_blocks() { ctl command "clear @s" >/dev/null; ctl command "give @s minecraft:cobblestone 64" >/dev/null; ctl command "give @s minecraft:diamond_pickaxe" >/dev/null; }
reset() { platform 40; ctl command "gamemode survival" >/dev/null; ctl fill -12 $((Y - 12)) -12 30 $((Y - 2)) 30 air >/dev/null; give_blocks; tp 0.5 $Y 0.5; sleep 1; }
height() { ctl state | jq -r '.player.position[1] | floor'; }

# a long 1x2 tunnel through solid stone
reset
ctl fill 1 $Y -1 25 $((Y + 4)) 1 stone >/dev/null
ctl fill 1 $Y 0 25 $((Y + 1)) 0 air >/dev/null
run_until "goto 24 $Y 0" 900 "Arrived" >/dev/null
check "walks a tunnel" near 24.5 0.5 2

# a 1x1 shaft 8 high with solid walls: it has to pillar straight up inside
reset
ctl fill 9 $Y 9 11 $((Y + 9)) 11 stone >/dev/null
ctl fill 10 $Y 10 10 $((Y + 9)) 10 air >/dev/null
tp 10.5 $Y 10.5; sleep 1
run_until "goto 10 $((Y + 8)) 10" 900 "Arrived" >/dev/null
check "pillars up a narrow shaft" test "$(height)" -ge $((Y + 8))
alive && ok "alive in the shaft" || bad "died in the shaft"

# a one-block ledge over the void, 12 long
reset
ctl fill 1 $((Y - 1)) -40 14 $((Y - 1)) 40 air >/dev/null
ctl fill 1 $((Y - 1)) 0 12 $((Y - 1)) 0 stone >/dev/null
run_until "goto 12 $Y 0" 600 "Arrived" >/dev/null
check "walks a narrow ledge" near 12.5 0.5 1.5
alive && ok "alive on the ledge" || bad "fell off the ledge"

# a goal inside a sealed stone room: it digs in
reset
ctl fill 16 $Y 16 20 $((Y + 4)) 20 stone >/dev/null
ctl fill 18 $Y 18 18 $((Y + 1)) 18 air >/dev/null
run_until "goto 18 $Y 18" 1200 "Arrived" >/dev/null
check "digs into a sealed room" near 18.5 18.5 1.5

# the same with breaking off: it must not dig, and says it cannot
ctl fill 16 $Y 16 20 $((Y + 4)) 20 stone >/dev/null
ctl fill 18 $Y 18 18 $((Y + 1)) 18 air >/dev/null
chat "/basso set allowBreak false"
tp 0.5 $Y 0.5; sleep 1
chat "/basso goto 18 $Y 18"
ctl wait 300 >/dev/null
check "does not dig with allowBreak off" test "$(block_at 16 $Y 18)" = minecraft:stone -a "$(block_at 17 $Y 18)" = minecraft:stone
chat "/basso cancel"
chat "/basso set allowBreak true"

# a goal at the bottom of 4 deep water
reset
ctl fill 5 $((Y - 5)) 5 9 $((Y - 1)) 9 stone >/dev/null
ctl fill 6 $((Y - 4)) 6 8 $((Y - 1)) 8 water >/dev/null
run_until "goto 7 $((Y - 4)) 7" 900 "Arrived" >/dev/null
check "dives to the bottom of a pool" near 7.5 7.5 2
alive && ok "alive after the dive" || bad "drowned"

# a ladder up the side of a 7 high stone column; the goal is on top
reset
ctl fill 15 $Y 15 15 $((Y + 6)) 15 stone >/dev/null
for y in $(seq $Y $((Y + 6))); do ctl setblock 14 $y 15 "ladder[facing=west]" >/dev/null; done
ctl command "clear @s" >/dev/null
tp 13.5 $Y 15.5; sleep 1
run_until "goto 15 $((Y + 7)) 15" 900 "Arrived" >/dev/null
check "climbs a ladder to the top of a column" test "$(height)" -ge $((Y + 7))
alive && ok "alive near the ladder" || bad "died near the ladder"

# and back down the same ladder, from the top of the column
tp 15.5 $((Y + 7)) 15.5; sleep 1
run_until "goto 12 $Y 15" 900 "Arrived" >/dev/null
check "climbs down a ladder" near 12.5 15.5 2
check "back on the ground" test "$(height)" -le $((Y + 1))
alive && ok "alive after climbing down" || bad "died climbing down"

# vines on a wall, 5 high, with a ledge on top
reset
ctl fill 15 $Y 15 15 $((Y + 4)) 15 stone >/dev/null
for y in $(seq $Y $((Y + 4))); do ctl setblock 14 $y 15 "vine[east=true]" >/dev/null; done
ctl command "clear @s" >/dev/null
tp 13.5 $Y 15.5; sleep 1
run_until "goto 15 $((Y + 5)) 15" 900 "Arrived" >/dev/null
check "climbs vines to the top" test "$(height)" -ge $((Y + 5))

# diagonal across scattered stones
reset
for p in "5 3" "8 6" "11 9" "14 12" "17 15" "6 12" "12 6" "9 12"; do set -- $p; ctl setblock $1 $Y $2 stone >/dev/null; ctl setblock $1 $((Y + 1)) $2 stone >/dev/null; done
run_until "goto 20 $Y 20" 900 "Arrived" >/dev/null
check "crosses a scattering of pillars" near 20.5 20.5 1.5

chat "/basso cancel"
summary
