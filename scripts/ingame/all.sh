#!/usr/bin/env bash
# Every in-game script in turn, with a total at the end. The game must be
# running the lod-dev build in a world with cheats on.
cd "$(dirname "$0")" || exit 1
failed=0
for s in goto mine hazards parkour place mlg misc follow mine2 edge terrain surface; do
    out=$("./$s.sh" 2>&1)
    echo "$out" | grep -E "FAIL|passed"
    echo "$out" | tail -1 | grep -q " 0 failed" || failed=1
done
exit $failed
