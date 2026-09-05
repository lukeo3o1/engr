#!/bin/sh
# The frozen subject pins every predecessor file by digest. So what happens when
# one of those files moves between the question and the answer?
#
# Two routes to the same instant, and the difference is whether the destination
# was already staged when the source moved:
#   a) prepared, not yet staged  -> confirm
#   b) prepared, staged, crashed -> confirm (a resume)
set -u
cd /audit/r4
E=/audit/bin/engr-current
CODE=FBR8Z2
AU=.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
rm -rf sm && mkdir sm

echo "===== a) the source moves before anything is staged ====="
cp -a checkpoints/pre-confirm sm/a
echo "staged destination: $([ -d sm/a/.engr/local/migration/destination ] && echo yes || echo no)"
before=$(sha256sum "sm/a/$AU" | cut -d' ' -f1)
sed -i 's/Migration continuity audit/Migration continuity audit (edited under the plan)/' "sm/a/$AU"
after=$(sha256sum "sm/a/$AU" | cut -d' ' -f1)
echo "source digest: $before -> $after"
echo "the frozen subject says: $(grep -o '"objects/01a05e55-74[^"]*":"1:[0-9a-f]*"' "sm/a/.engr/local/challenges/$CODE.json")"
$E --root sm/a confirm "CONFIRM $CODE" 2>&1 | head -3; echo "confirm exit=$?"
echo "title on disk now: $(grep -o '"title":"[^"]*"' "sm/a/$AU" 2>/dev/null || grep -o 'Migration continuity audit[^"]*' "sm/a/$AU" | head -1)"

echo
echo "===== b) the source moves after a crash left the destination staged ====="
cp -a wi/w sm/b
echo "staged destination: $([ -d sm/b/.engr/local/migration/destination ] && echo yes || echo no)"
sed -i 's/Migration continuity audit/Migration continuity audit (edited under the plan)/' "sm/b/$AU"
$E --root sm/b confirm "CONFIRM $CODE" 2>&1 | head -3; echo "confirm exit=$?"
echo "-- did the edit survive, or was it published over? --"
grep -o 'Migration continuity audit[^"]*' "sm/b/$AU" | head -1
echo "-- and what does the record say now? --"
$E --root sm/b verify 2>&1 | head -4; echo "verify exit=$?"
