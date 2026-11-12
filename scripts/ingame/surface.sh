#!/usr/bin/env bash
# surface: mining natural terrain from the surface with a pickaxe and a
# water bucket. A path must be found at once (no "No path", search over in a
# few ticks); every ore is dug up. Tries ores at several depths and offsets.
. "$(dirname "$0")/lib.sh"
echo "surface"
SX=${SURFACE_X:-}
SZ=${SURFACE_Z:-}
ctl command "difficulty peaceful" >/dev/null
ctl command "gamemode creative" >/dev/null
chat "/basso cancel"
chat "/basso set verbose 1"
chat "/basso set freecam false"

# The y of the topmost solid, non-leaf block in a column.
ground() {
    local x=$1 z=$2 y b
    for y in $(seq 150 -1 40); do
        b=$(block_at "$x" "$y" "$z")
        case "$b" in
            minecraft:air|minecraft:cave_air|*leaves*|*_log|minecraft:short_grass|minecraft:tall_grass|minecraft:fern|minecraft:snow) ;;
            minecraft:water) echo water; return ;;
            *) echo "$y"; return ;;
        esac
    done
    echo none
}


# A dry spot: ground that is land (and not a cliff) across the area trials use.
find_site() {
    local cx cz dx dz g n
    for cx in 200 -150 400 -300 600; do
        for cz in 120 -200 300 -400; do
            tp $cx 200 $cz
            ctl wait 120 >/dev/null
            n=0
            for dx in -4 0 8 20 26; do
                for dz in -12 0 5 10; do
                    g=$(ground $((cx + dx)) $((cz + dz)))
                    [[ "$g" =~ ^[0-9]+$ ]] && [ "$g" -ge 64 ] && n=$((n + 1))
                done
            done
            if [ "$n" -ge 20 ]; then SX=$cx; SZ=$cz; return 0; fi
        done
    done
    return 1
}
if [ -z "$SX" ]; then
    find_site || { echo "  no dry land found"; exit 1; }
fi
echo "  site $SX $SZ"
# leftover ores from earlier runs would be mined instead
for y in $(seq 40 10 110); do
    ctl command "fill $((SX - 20)) $y $((SZ - 25)) $((SX + 30)) $((y + 9)) $((SZ + 25)) stone replace diamond_ore" >/dev/null
done

trial() { # trial NAME DX DZ DEPTH
    local name=$1 x=$((SX + $2)) z=$((SZ + $3)) depth=$4 g
    g=$(ground "$x" "$z")
    if ! [[ "$g" =~ ^[0-9]+$ ]]; then echo "  skip $name (ground: $g)"; return; fi
    local oy=$((g - depth))
    ctl setblock "$x" "$oy" "$z" diamond_ore >/dev/null
    ctl command "gamemode creative" >/dev/null
    chat "/basso cancel"
    ctl command "clear @s" >/dev/null
    tp "$((SX + 0))" $((g + 3)) "$SZ"
    ctl wait 20 >/dev/null
    local pg
    pg=$(ground "$SX" "$SZ")
    [[ "$pg" =~ ^[0-9]+$ ]] || pg=$g
    tp "$SX" $((pg + 1)) "$SZ"
    ctl command "give @s minecraft:diamond_pickaxe" >/dev/null
    ctl command "give @s minecraft:water_bucket" >/dev/null
    ctl command "gamemode survival" >/dev/null
    ctl wait 20 >/dev/null
    local log=~/.local/share/horizon/instances/lod-dev/logs/latest.log start
    start=$(wc -l < "$log")
    chat "/basso mine 1 diamond_ore"
    local waited=0 done=no
    while [ $waited -lt 1800 ]; do
        ctl wait 20 >/dev/null
        waited=$((waited + 20))
        if tail -n +$((start + 1)) "$log" | grep -qE "Mined 1|Explored without|No more"; then done=yes; break; fi
        if [ "$(block_at "$x" "$oy" "$z")" != minecraft:diamond_ore ]; then done=yes; break; fi
    done
    local text searches nopath partial
    text=$(tail -n +$((start + 1)) "$log" | grep 'CHAT.*Basso')
    searches=$(echo "$text" | grep -c "Search over")
    nopath=$(echo "$text" | grep -c "No path from")
    partial=$(echo "$text" | grep -c "partial")
    echo "  $name: depth $depth at ($2,$3): ground $g ore y $oy: ${waited} ticks, $searches searches ($partial partial), $nopath no-path"
    echo "$text" | grep "Search over" | head -3 | sed 's/^/        /'
    if [ "$(block_at "$x" "$oy" "$z")" != minecraft:diamond_ore ]; then ok "$name: ore dug"; else bad "$name: ore still there after $waited ticks"; fi
    if [ "$nopath" -eq 0 ]; then ok "$name: no 'No path'"; else bad "$name: $nopath no-path failures"; fi
    chat "/basso cancel"
}

trial shallow-near 4 3 3
trial shallow-far 20 10 3
trial mid 8 5 8
trial deep 6 4 14
trial deep-far 18 -12 20
trial straight-down 0 0 12
ctl command "gamemode creative" >/dev/null
summary
