#!/usr/bin/env bash
# mlg: landing a long fall with a water bucket, and what it must leave alone.
. "$(dirname "$0")/lib.sh"
echo "mlg"
platform 40
ctl command "gamemode survival" >/dev/null
chat "/basso cancel"
health() { ctl state | jq -r '.player.health'; }
full_health() { awk -v h="$(health)" 'BEGIN{exit !(h>=20)}'; }
alive() { awk -v h="$(health)" 'BEGIN{exit !(h>0)}'; }
buckets() { ctl inventory | jq -r '.counts["minecraft:water_bucket"] // 0'; }
# fall from $1 blocks up at x z with an order going, wait for the landing
fall() {
    local height=$1 x=$2 z=$3
    # order after the teleport: an order that ends on the way would not clutch
    tp "$x.5" $((Y + height)) "$z.5"
    chat "/basso goto $((x + 30)) $Y $z"
    ctl wait 160 >/dev/null
    chat "/basso cancel"
    ctl wait 20 >/dev/null
}
reset() { ctl command "clear @s" >/dev/null; ctl command "effect clear @s" >/dev/null; ctl command "give @s minecraft:water_bucket" >/dev/null; sleep 1; }

# a long fall onto stone
reset
fall 40 5 5
check "survives 40 blocks onto stone" full_health
check "has the bucket back" test "$(buckets)" -eq 1
check "leaves no water behind" test "$(block_at 5 $Y 5)" = minecraft:air

# a spread of heights: the ground has to be in reach on some tick of any fall
for h in 12 21 33 47 61 75; do
    reset
    fall $h $((h - 40)) $((40 - h))
    check "survives $h blocks" full_health
    check "bucket back after $h" test "$(buckets)" -eq 1
done

# without a bucket the same fall is deadly: the test is real
ctl command "clear @s" >/dev/null
revive
chat "/basso cancel"
tp 20.5 $((Y + 40)) 20.5
ctl wait 160 >/dev/null
check "the same fall without a bucket hurts" test "$(ctl state | jq -r '.screen')" = dead
revive; platform 40; ctl command "gamemode survival" >/dev/null

# the bucket in another hotbar slot
reset
ctl command "give @s minecraft:cobblestone 64" >/dev/null
ctl command "item replace entity @s hotbar.4 with minecraft:water_bucket" >/dev/null
ctl command "item replace entity @s hotbar.0 with minecraft:cobblestone 64" >/dev/null
sleep 1
fall 30 -11 -11
check "works from another slot" full_health

# the switch
reset
chat "/basso set mlgWaterBucket false"
tp 30.5 $((Y + 40)) 30.5
chat "/basso goto 0 $Y 0"
ctl wait 160 >/dev/null
check "switched off it does not clutch" test "$(ctl state | jq -r '.screen')" = dead
chat "/basso set mlgWaterBucket true"
revive; platform 40; ctl command "gamemode survival" >/dev/null

# a short fall needs nothing
reset
fall 3 8 -9
check "a short fall leaves the bucket in hand" test "$(buckets)" -eq 1
check "short fall is fine" alive

# landing in water already
reset
ctl fill 14 $((Y - 1)) 14 16 $((Y - 1)) 16 water >/dev/null
fall 30 15 15
check "falls into existing water unhurt" full_health
check "keeps the bucket" test "$(buckets)" -eq 1

ctl command "gamemode creative" >/dev/null
summary
