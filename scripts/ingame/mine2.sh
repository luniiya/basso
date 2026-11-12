#!/usr/bin/env bash
# mine2: what the drops do, shafts down, ores by lava, the wrong tool,
# more asked than there is.
. "$(dirname "$0")/lib.sh"
echo "mine2"
platform 40
ctl command "gamemode survival" >/dev/null
chat "/basso cancel"
health() { ctl state | jq -r '.player.health'; }
alive() { awk -v h="$(health)" 'BEGIN{exit !(h>0)}'; }
said() { basso_text 60 | grep -qF -- "$1"; }
have() { ctl inventory | jq -r --arg i "$1" '.counts[$i] // 0'; }
is() { test "$(block_at "$1" "$2" "$3")" = "minecraft:$4"; }
pick() { ctl command "give @s minecraft:diamond_pickaxe" >/dev/null; }
reset() { platform 40; ctl command "gamemode survival" >/dev/null; ctl command "clear @s" >/dev/null; tp 0.5 $Y 0.5; sleep 1; }

# the drops end up in the inventory
reset; pick
ctl setblock 6 $Y 0 coal_ore >/dev/null
run_until "mine 1 coal_ore" 600 "Mined" >/dev/null
sleep 1
check "picks up the coal it dug" test "$(have minecraft:coal)" -ge 1

# an ore at the bottom of a 3 deep hole in the floor
reset; pick
ctl fill 8 $((Y - 8)) 8 8 $((Y - 1)) 8 stone >/dev/null
ctl fill 8 $((Y - 3)) 8 8 $((Y - 1)) 8 air >/dev/null
ctl setblock 8 $((Y - 4)) 8 coal_ore >/dev/null
run_until "mine 1 coal_ore" 900 "Mined" >/dev/null
check "mines the ore at the bottom of a hole" test "$(block_at 8 $((Y - 4)) 8)" != minecraft:coal_ore
alive && ok "alive after the hole" || bad "died in the hole"

# an ore under solid ground: it has to dig down, not fall into a void
reset; pick
ctl fill -6 $((Y - 9)) -6 -6 $((Y - 2)) -6 stone >/dev/null
ctl setblock -6 $((Y - 5)) -6 coal_ore >/dev/null
run_until "mine 1 coal_ore" 1200 "Mined" >/dev/null
check "digs a shaft down to an ore" test "$(block_at -6 $((Y - 5)) -6)" != minecraft:coal_ore
alive && ok "alive after the shaft" || bad "died in the shaft"

# an ore with lava right behind it: it must not open the lava on itself
reset; pick
ctl fill 12 $Y 0 14 $((Y + 2)) 2 stone >/dev/null
ctl setblock 12 $Y 1 coal_ore >/dev/null
ctl fill 13 $Y 1 14 $Y 1 lava >/dev/null
run_until "mine 1 coal_ore" 900 "Mined" >/dev/null
alive && ok "alive next to lava" || bad "died next to lava"
check "did not wade into the lava" test "$(posf | awk '{print ($1 > 12.0 && $1 < 15.0 && $3 > 0.9 && $3 < 2.1)}')" = 0

# iron needs a stone pickaxe: with bare hands nothing drops, so it is skipped or said
reset
ctl setblock 7 $Y 7 iron_ore >/dev/null
chat "/basso mine 1 iron_ore"
ctl wait 600 >/dev/null
chat "/basso cancel"
alive && ok "alive with the wrong tool" || bad "died with the wrong tool"
check "no iron picked up by bare hands" test "$(have minecraft:raw_iron)" = 0

# more asked than there is: it digs what there is and says so, then stops
reset; pick
ctl setblock 5 $Y 5 coal_ore >/dev/null
ctl setblock -5 $Y 5 coal_ore >/dev/null
chat "/basso mine 5 coal_ore"
ctl wait 900 >/dev/null
chat "/basso cancel"
check "got the two there were" test "$(have minecraft:coal)" -ge 2
alive && ok "alive after asking for too many" || bad "died asking for too many"

# a block that is not an ore or does not exist
reset
chat "/basso mine 1 not_a_block"
ctl wait 20 >/dev/null
alive && ok "alive after a bad block name" || bad "died on a bad block name"
chat "/basso cancel"
summary
