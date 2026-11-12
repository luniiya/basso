#!/usr/bin/env bash
# goto: a short walk, a long walk, a goal behind a wall, an unreachable goal.
. "$(dirname "$0")/lib.sh"
echo "goto"
platform 40
tp 0.5 $Y 0.5
chat "/basso cancel"

run_until "goto 10 $Y 0" 300 "Arrived" >/dev/null; check "arrives at a near goal" near 10.5 0.5 1.5

tp 0.5 $Y 0.5
run_until "goto 60 $Y 0" 900 "Arrived" >/dev/null; check "arrives across a partial path" near 60.5 0.5 2

# a wall with a gap, the goal behind it
tp 0.5 $Y 0.5
ctl fill 10 $Y -8 10 $((Y + 3)) 8 stone >/dev/null
ctl fill 10 $Y 8 10 $((Y + 3)) 9 stone >/dev/null
run_until "goto 20 $Y 0" 900 "Arrived" >/dev/null; check "goes around a wall" near 20.5 0.5 2
ctl fill 10 $Y -8 10 $((Y + 3)) 9 air >/dev/null

# a wall appears in the way while walking
tp 0.5 $Y 0.5
chat "/basso goto 30 $Y 0"
ctl wait 40 >/dev/null
ctl fill 12 $Y -6 12 $((Y + 3)) 6 stone >/dev/null
for i in $(seq 1 30); do near 30.5 0.5 2 && break; ctl wait 20 >/dev/null; done
check "replans around a wall placed mid-walk" near 30.5 0.5 2
ctl fill 12 $Y -6 12 $((Y + 3)) 6 air >/dev/null
chat "/basso cancel"
summary
