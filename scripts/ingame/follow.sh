#!/usr/bin/env bash
# follow: nothing to follow, something up high, across a void, something that
# dies, something that keeps running, and the cancel.
. "$(dirname "$0")/lib.sh"
echo "follow"
platform 40
ctl command "gamemode survival" >/dev/null
chat "/basso cancel"
health() { ctl state | jq -r '.player.health'; }
alive() { awk -v h="$(health)" 'BEGIN{exit !(h>0)}'; }
said() { basso_text 60 | grep -qF -- "$1"; }
pig() { ctl command "summon minecraft:pig $1 $2 $3 {NoAI:1b,Silent:1b,PersistenceRequired:1b}" >/dev/null; }
clear_pigs() { ctl command "kill @e[type=minecraft:pig]" >/dev/null; }
give_blocks() { ctl command "clear @s" >/dev/null; ctl command "give @s minecraft:cobblestone 64" >/dev/null; }

# nothing of that kind around
clear_pigs
tp 0.5 $Y 0.5; sleep 1
chat "/basso follow entity minecraft:pig"
ctl wait 100 >/dev/null
alive && ok "alive with nothing to follow" || bad "died with nothing to follow"
check "stays put with nothing to follow" near 0.5 0.5 3
chat "/basso cancel"

# it appears later
chat "/basso follow entity minecraft:pig"
ctl wait 40 >/dev/null
pig 15.5 $Y 0.5
ctl wait 300 >/dev/null
check "goes to it once it shows up" near 15.5 0.5 5
chat "/basso cancel"
clear_pigs

# up on a pillar of stone, 3 high
tp 0.5 $Y 0.5; sleep 1
ctl fill 10 $Y 10 10 $((Y + 2)) 10 stone >/dev/null
pig 10.5 $((Y + 3)) 10.5
chat "/basso follow entity minecraft:pig"
ctl wait 500 >/dev/null
check "climbs to something standing high" near 10.5 10.5 4
chat "/basso cancel"
clear_pigs
ctl fill 10 $Y 10 10 $((Y + 2)) 10 air >/dev/null

# across a void of 5, with blocks to bridge
give_blocks
ctl fill 1 $((Y - 1)) -40 5 $((Y - 1)) 40 air >/dev/null
tp 0.5 $Y 0.5; sleep 1
pig 10.5 $Y 0.5
chat "/basso follow entity minecraft:pig"
ctl wait 600 >/dev/null
check "bridges to something over a void" near 10.5 0.5 4
alive && ok "alive after the void" || bad "died crossing the void"
chat "/basso cancel"
clear_pigs
ctl fill 1 $((Y - 1)) -40 5 $((Y - 1)) 40 stone >/dev/null

# it dies while followed: basso carries on without it
tp 0.5 $Y 0.5; sleep 1
pig 20.5 $Y 0.5
chat "/basso follow entity minecraft:pig"
ctl wait 40 >/dev/null
clear_pigs
ctl wait 200 >/dev/null
alive && ok "alive after the target died" || bad "died after the target died"
chat "/basso cancel"

# cancel really stops
tp 0.5 $Y 0.5; sleep 1
pig 35.5 $Y 0.5
chat "/basso follow entity minecraft:pig"
ctl wait 30 >/dev/null
chat "/basso cancel"
ctl wait 5 >/dev/null
a=$(posf | awk '{print $1}')
ctl wait 60 >/dev/null
b=$(posf | awk '{print $1}')
check "stands still after cancel" awk -v a="$a" -v b="$b" 'BEGIN{d=a-b; if (d<0) d=-d; exit !(d<0.5)}'
clear_pigs

# a mover: someone keeps teleporting it about
tp 0.5 $Y 0.5; sleep 1
pig 20.5 $Y 0.5
chat "/basso follow entity minecraft:pig"
for z in 10 -10 15 -15; do
    ctl command "tp @e[type=minecraft:pig,limit=1] 20.5 $Y $z.5" >/dev/null
    ctl wait 120 >/dev/null
done
check "keeps up with a moving target" near 20.5 -15.5 6
alive && ok "alive after the chase" || bad "died in the chase"
chat "/basso cancel"
clear_pigs

# players: none but us
chat "/basso follow players"
ctl wait 60 >/dev/null
alive && ok "alive following players when alone" || bad "died"
chat "/basso cancel"
summary
