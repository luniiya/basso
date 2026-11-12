#!/usr/bin/env bash
# misc: pause and resume, swimming, following, walls to dig, the switches.
. "$(dirname "$0")/lib.sh"
echo "misc"
platform 40
ctl command "gamemode survival" >/dev/null
chat "/basso cancel"
health() { ctl state | jq -r '.player.health'; }
alive() { awk -v h="$(health)" 'BEGIN{exit !(h>0)}'; }
said() { basso_text 60 | grep -qF -- "$1"; }
px() { ctl state | jq -r '.player.position[0]'; }
moved() { awk -v a="$1" -v b="$2" -v d="$3" 'BEGIN{x=a-b; if (x<0) x=-x; exit !(x>=d)}'; }

# pause stops the walk, resume goes on
tp 0.5 $Y 0.5; sleep 1
chat "/basso goto 35 $Y 0"
ctl wait 40 >/dev/null
chat "/basso pause"
ctl wait 10 >/dev/null
a=$(px); ctl wait 40 >/dev/null; b=$(px)
check "pause stands still" awk -v a="$a" -v b="$b" 'BEGIN{x=a-b; if (x<0) x=-x; exit !(x<0.3)}'
check "it had set off" moved "$a" 0.5 3
run_until "resume" 800 "Arrived" >/dev/null
check "resume arrives" near 35.5 0.5 2

# cancel
tp 0.5 $Y 0.5; sleep 1
chat "/basso goto 35 $Y 0"
ctl wait 30 >/dev/null
chat "/basso cancel"
ctl wait 10 >/dev/null
a=$(px); ctl wait 40 >/dev/null; b=$(px)
check "cancel stops" awk -v a="$a" -v b="$b" 'BEGIN{x=a-b; if (x<0) x=-x; exit !(x<0.3)}'

# swimming across a pool, with air to spare
ctl fill 5 $((Y - 4)) -3 25 $((Y - 1)) 3 water >/dev/null
tp 3.5 $Y 0.5; sleep 1
run_until "goto 27 $Y 0" 1500 "Arrived" >/dev/null
check "swims across a pool" near 27.5 0.5 2.5
check "alive after the swim" alive
check "breathes again" test "$(air)" -ge 250

# a stone wall in the way is dug when breaking is allowed
platform 40; ctl command "gamemode survival" >/dev/null
ctl command "clear @s" >/dev/null
ctl command "give @s minecraft:diamond_pickaxe" >/dev/null
ctl fill 10 $Y -40 12 $((Y + 6)) 40 stone >/dev/null
tp 0.5 $Y 0.5; sleep 1
run_until "goto 16 $Y 0" 1500 "Arrived" >/dev/null
check "digs through a thick wall with a pickaxe" near 16.5 0.5 2

# bare handed, a thin wall is still worth it
platform 40; ctl command "gamemode survival" >/dev/null
ctl command "clear @s" >/dev/null
ctl fill 10 $Y -40 10 $((Y + 6)) 40 stone >/dev/null
tp 0.5 $Y 0.5; sleep 1
run_until "goto 14 $Y 0" 2400 "Arrived" >/dev/null
check "digs a thin wall by hand" near 14.5 0.5 2

# and goes around, or stays out, when it may not break
platform 40; ctl command "gamemode survival" >/dev/null
ctl fill 10 $Y -40 12 $((Y + 6)) 40 stone >/dev/null
chat "/basso set allowBreak false"
tp 0.5 $Y 0.5; sleep 1
chat "/basso goto 16 $Y 0"
ctl wait 300 >/dev/null
check "does not dig when it may not" test "$(px | awk '{print ($1 < 10)}')" = 1
chat "/basso cancel"
chat "/basso set allowBreak true"

# following something that stands still, then something that moves away
platform 40; ctl command "gamemode survival" >/dev/null
ctl command "kill @e[type=minecraft:pig]" >/dev/null
tp 0.5 $Y 0.5; sleep 1
ctl command "summon minecraft:pig 20.5 $Y 0.5 {NoAI:1b,Silent:1b}" >/dev/null
chat "/basso follow entity minecraft:pig"
ctl wait 300 >/dev/null
check "follows an animal up to it" near 20.5 0.5 5
ctl command "tp @e[type=minecraft:pig,limit=1] -20.5 $Y 15.5" >/dev/null
ctl wait 400 >/dev/null
check "follows it when it is moved" near -20.5 15.5 6
chat "/basso cancel"
ctl command "kill @e[type=minecraft:pig]" >/dev/null

# nothing to find on an island in the sky: it explores, gives up, and lives
platform 40; ctl command "gamemode survival" >/dev/null
ctl command "clear @s" >/dev/null
tp 0.5 $Y 0.5; sleep 1
chat "/basso mine 1 diamond_ore"
ctl wait 900 >/dev/null
check "said it was exploring" said "Exploring"
check "it kept walking about" test "$(posf | awk '{d=sqrt($1*$1+$3*$3); print (d > 15)}')" = 1
alive && ok "alive after exploring" || bad "died exploring"
chat "/basso cancel"

# settings
chat "/basso set autoTool false"
check "settings answer" said "autoTool is now off"
chat "/basso toggle autoTool"
check "toggle flips it" said "autoTool is now on"
chat "/basso set nonsense true"
check "unknown settings are named" said "nonsense"
chat "/basso set autoTool maybe"
check "bad values are refused" said "isn't true or false"

ctl command "gamemode creative" >/dev/null
# the watch camera must not get in the way of walking, and goes away after
platform 40; ctl command "gamemode survival" >/dev/null
chat "/basso set freecam true"
tp 0.5 $Y 0.5; sleep 1
run_until "goto 25 $Y 0" 600 "Arrived" >/dev/null
check "walks to a goal with the watch camera on" near 25.5 0.5 2
chat "/basso set freecam false"
run_until "goto 0 $Y 10" 600 "Arrived" >/dev/null
check "walks on with the watch camera off" near 0.5 10.5 2
alive && ok "alive after the camera switches" || bad "died around the camera switches"
chat "/basso cancel"
summary
