#!/usr/bin/env bash
# parkour: gaps of growing width along +x, an up-jump, a gap over lava, and a
# staircase of single blocks. Each course is on its own z line.
. "$(dirname "$0")/lib.sh"
echo "parkour"
platform 40
ctl command "gamemode survival" >/dev/null
chat "/basso cancel"
health() { ctl state | jq -r '.player.health'; }
alive() { awk -v h="$(health)" 'BEGIN{exit !(h>0)}'; }
# remove the ground from x=5 on, so only the islands exist
clear_ground() { ctl fill 1 $((Y - 1)) "$1" 60 $((Y - 1)) "$1" air >/dev/null; }
island() { ctl fill "$1" $((Y - 1)) "$3" "$2" $((Y - 1)) "$3" stone >/dev/null; }

course() { # name z gap goal_x
    local name=$1 z=$2 gap=$3 goal=$4
    clear_ground "$z"
    island -3 0 "$z"
    island $((1 + gap)) $((3 + gap)) "$z"
    tp 0.5 $Y $((z)).5
    sleep 1
    run_until "goto $((2 + gap)) $Y $z" 600 "Arrived" >/dev/null
    check "$name" near $((2 + gap)).5 $z.5 2
    alive || bad "$name: died"
}
course "jumps a 1 block gap" 0 1 3
course "jumps a 2 block gap" 4 2 4
course "jumps a 3 block gap" 8 3 5

# one block up over a gap
clear_ground 12
island -3 0 12
island 3 5 12
ctl fill 3 $Y 12 5 $Y 12 stone >/dev/null
tp 0.5 $Y 12.5
sleep 1
run_until "goto 4 $((Y + 1)) 12" 600 "Arrived" >/dev/null
check "jumps up over a gap" near 4.5 12.5 2

# over lava
clear_ground 16
island -3 0 16
island 3 6 16
ctl fill 1 $((Y - 1)) 16 2 $((Y - 1)) 16 lava >/dev/null
tp 0.5 $Y 16.5
sleep 1
run_until "goto 4 $Y 16" 600 "Arrived" >/dev/null
check "jumps over lava" near 4.5 16.5 2
alive && ok "alive after the lava" || bad "died in the lava"

# a chain of single-block islands
clear_ground 20
island -3 0 20
for x in 3 6 9 12 15; do island $x $x 20; done
tp 0.5 $Y 20.5
sleep 1
run_until "goto 15 $Y 20" 900 "Arrived" >/dev/null
check "crosses a chain of islands" near 15.5 20.5 2
ctl command "gamemode creative" >/dev/null
chat "/basso cancel"
# from a one block island there is little run-up
standing() { # name z gap goal_x
    local name=$1 z=$2 gap=$3
    ctl command "clear @s" >/dev/null
    clear_ground "$z"
    ctl fill -40 $((Y - 1)) "$z" -1 $((Y - 1)) "$z" air >/dev/null
    island 0 0 "$z"
    island $((1 + gap)) $((3 + gap)) "$z"
    tp 0.5 $Y $((z)).5
    sleep 1
    run_until "goto $((2 + gap)) $Y $z" 600 "Arrived" >/dev/null
}
standing x 24 2
check "jumps 2 blocks from a standstill" near 4.5 24.5 2
standing x 28 3
check "jumps 3 blocks from a standstill" near 5.5 28.5 2
alive && ok "alive after standing jumps" || bad "died on a standing jump"
standing x 32 4
# a sprint jump from the edge of one block still carries four blocks
check "crosses 4 blocks from a standstill" near 6.5 32.5 2.5
alive && ok "alive after the 4 block standing jump" || bad "died on the 4 block standing jump"
chat "/basso cancel"
summary
