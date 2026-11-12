#!/usr/bin/env bash
# terrain: blocks that are not plain cubes: stairs, slabs, doors, gates,
# fences, carpets, cobwebs, cactus, magma.
. "$(dirname "$0")/lib.sh"
echo "terrain"
ctl command "gamemode survival" >/dev/null
chat "/basso cancel"
health() { ctl state | jq -r '.player.health'; }
alive() { awk -v h="$(health)" 'BEGIN{exit !(h>0)}'; }
height() { ctl state | jq -r '.player.position[1] | floor'; }
reset() { platform 40; ctl command "gamemode survival" >/dev/null; ctl command "clear @s" >/dev/null; tp 0.5 $Y 0.5; sleep 1; }
go() { run_until "goto $1 $2 $3" ${4:-900} "Arrived" >/dev/null; }

# a staircase of stairs going up along +x, 5 steps, with solid fill under
reset
for i in 1 2 3 4 5; do
    ctl fill $((2 * i)) $Y -1 $((2 * i + 1)) $((Y + i - 1)) 1 stone >/dev/null
    ctl setblock $((2 * i)) $((Y + i - 1)) 0 "oak_stairs[facing=east]" >/dev/null
done
go 12 $((Y + 5)) 0
check "walks up a staircase of stairs" test "$(height)" -ge $((Y + 5))
alive && ok "alive on the stairs" || bad "died on the stairs"

# a path of bottom slabs half a block up, 10 long
reset
ctl fill 2 $((Y - 1)) 0 12 $((Y - 1)) 0 stone >/dev/null
ctl fill 2 $Y 0 12 $Y 0 "oak_slab[type=bottom]" >/dev/null
go 12 $Y 0
check "walks over slabs" near 12.5 0.5 2

# slabs rising: bottom slab then full block then slab
reset
ctl setblock 3 $Y 0 "oak_slab[type=bottom]" >/dev/null
ctl setblock 5 $Y 0 stone >/dev/null
ctl setblock 7 $Y 0 stone >/dev/null; ctl setblock 7 $((Y + 1)) 0 "oak_slab[type=bottom]" >/dev/null
go 9 $((Y + 1)) 0
check "climbs a mix of slabs and blocks" test "$(height)" -ge $((Y + 1))

# a closed door in a wall: through it or around
reset
ctl fill 10 $Y -20 10 $((Y + 3)) 20 stone >/dev/null
ctl setblock 10 $Y 0 "oak_door[half=lower,facing=east]" >/dev/null
ctl setblock 10 $((Y + 1)) 0 "oak_door[half=upper,facing=east]" >/dev/null
chat "/basso set allowBreak false"
go 15 $Y 0 900
check "gets through a door in a wall without breaking it" near 15.5 0.5 2
chat "/basso set allowBreak true"

# a fence line the player cannot jump over (1.5 high)
reset
ctl fill 8 $Y -15 8 $Y 15 oak_fence >/dev/null
go 14 $Y 0 900
check "goes around a fence" near 14.5 0.5 2
alive && ok "alive near the fence" || bad "died near the fence"

# a fence gate, closed
reset
ctl fill 8 $Y -15 8 $Y 15 oak_fence >/dev/null
ctl setblock 8 $Y 0 "oak_fence_gate[facing=east]" >/dev/null
chat "/basso set allowBreak false"
go 14 $Y 0 900
check "uses a fence gate or goes around" near 14.5 0.5 2
chat "/basso set allowBreak true"

# carpet on the floor is just floor
reset
ctl fill 2 $Y 0 12 $Y 0 white_carpet >/dev/null
go 12 $Y 0
check "walks over carpet" near 12.5 0.5 2

# cobwebs across the way: it should prefer not to walk through them
reset
ctl fill 8 $Y -1 8 $((Y + 1)) 1 cobweb >/dev/null
go 14 $Y 0 900
check "gets past a patch of cobweb" near 14.5 0.5 2

# a field of cactus and magma around a straight line
reset
ctl fill 6 $Y -1 10 $Y 1 cactus >/dev/null
ctl fill 6 $((Y - 1)) 2 10 $((Y - 1)) 3 magma_block >/dev/null
go 14 $Y 0 900
check "reaches the goal past cactus and magma" near 14.5 0.5 2
alive && ok "alive after the cactus" || bad "hurt by the cactus or magma"
check "took no damage" test "$(health | awk '{print ($1 >= 20)}')" = 1

# snow layers and trapdoors on the floor
reset
ctl fill 2 $Y 0 6 $Y 0 "snow[layers=1]" >/dev/null
ctl fill 7 $Y 0 12 $Y 0 "oak_trapdoor[half=bottom,open=false]" >/dev/null
go 12 $Y 0
check "walks over snow and trapdoors" near 12.5 0.5 2

chat "/basso cancel"
summary
