#!/usr/bin/env bash
# hazards: lava ahead and below, a long fall, water to swim out of.
. "$(dirname "$0")/lib.sh"
echo "hazards"
platform 40
ctl command "gamemode survival" >/dev/null
health() { ctl state | jq -r '.player.health'; }
alive() { awk -v h="$(health)" 'BEGIN{exit !(h>0)}'; }
full() { awk -v h="$(health)" 'BEGIN{exit !(h>=20)}'; }
chat "/basso cancel"

# lava pool between the player and the goal: must go around
tp 0.5 $Y 0.5
ctl fill 8 $((Y - 1)) -3 12 $((Y - 1)) 3 lava >/dev/null
run_until "goto 20 $Y 0" 600 "Arrived" >/dev/null; check "reaches a goal behind a lava pool" near 20.5 0.5 2
check "did not touch the lava" full
ctl fill 8 $((Y - 1)) -3 12 $((Y - 1)) 3 stone >/dev/null

# mining an ore that sits on lava: never step or dig into the lava
tp 0.5 $Y 0.5
ctl fill 5 $((Y - 1)) -1 7 $((Y - 1)) 1 lava >/dev/null
ctl setblock 6 $Y 0 coal_ore >/dev/null
run_until "mine 1 coal_ore" 600 "Mined" >/dev/null; check "mines an ore over lava alive" alive
chat "/basso cancel"
ctl fill 5 $((Y - 1)) -1 7 $((Y - 1)) 1 stone >/dev/null

# a goal far below: the drop is survivable (water) or dug down in steps
tp 0.5 $Y 0.5
ctl fill 30 $((Y - 40)) -3 36 $((Y - 40)) 3 stone >/dev/null
run_until "goto 33 $((Y - 39)) 0" 900 "Arrived" >/dev/null; check "reaches a goal 40 blocks below" near 33.5 0.5 3
check "survives the way down" alive
chat "/basso cancel"
ctl command "gamemode creative" >/dev/null
summary
