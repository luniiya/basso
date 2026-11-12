#!/usr/bin/env bash
# mine: several ores at once, counted, a shaft above, a far ore, none nearby.
. "$(dirname "$0")/lib.sh"
echo "mine"
platform 40
tp 0.5 $Y 0.5
chat "/basso cancel"
gone() { test "$(block_at "$1" "$2" "$3")" != "minecraft:coal_ore"; }
gone_all() { gone 5 $Y 0 && gone -4 $Y 3 && gone 2 $Y -6; }

# three ores around, count of 3
ctl setblock 5 $Y 0 coal_ore >/dev/null
ctl setblock -4 $Y 3 coal_ore >/dev/null
ctl setblock 2 $Y -6 coal_ore >/dev/null
out=$(run_until "mine 3 coal_ore" 900 "Mined")
echo "  -> $out"
check "mines 3 known ores" gone_all

# an ore straight above the player (the shaft)
tp 0.5 $Y 0.5
ctl setblock 0 $((Y + 2)) 0 coal_ore >/dev/null
out=$(run_until "mine 1 coal_ore" 300 "Mined")
echo "  -> $out"
check "mines the ore above from below" gone 0 $((Y + 2)) 0

# a far ore (30 blocks), and one in a stone pocket
tp 0.5 $Y 0.5
ctl setblock 30 $Y 0 coal_ore >/dev/null
out=$(run_until "mine 1 coal_ore" 900 "Mined")
echo "  -> $out"
check "walks to a far ore" gone 30 $Y 0
ctl fill 20 $Y 10 24 $((Y + 4)) 14 stone >/dev/null
ctl setblock 22 $Y 12 coal_ore >/dev/null
tp 0.5 $Y 0.5
out=$(run_until "mine 1 coal_ore" 1200 "Mined")
echo "  -> $out"
check "digs into a stone pocket" gone 22 $Y 12
chat "/basso cancel"
summary
