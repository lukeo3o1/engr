#!/bin/sh
# Round 25's finding 2, rebuilt by unlinking exactly what an interrupted
# withdrawal unlinks, in that order: the Challenge first, then the manifest,
# with the directory left behind. Before the fix this wedged the workspace —
# `staged` and `validate_format` both refused to move past a stage with no
# manifest, so the predecessor could no longer be read *or* migrated.
set -u
cd /audit/r4
E=/audit/bin/engr-current
CODE=FBR8Z2
rm -rf wd && cp -a checkpoints/pre-confirm wd

echo "===== the prepared state ====="
ls wd/.engr/local/challenges/ wd/.engr/local/migration/ 2>&1
echo
echo "===== unlink what the interruption unlinks ====="
rm -f "wd/.engr/local/challenges/$CODE.json"
rm -f wd/.engr/local/migration/manifest.json
echo "left behind: $(find wd/.engr/local -type d | LC_ALL=C sort | tr '\n' ' ')"
echo "files: $(find wd/.engr/local -type f | wc -l)"
echo
echo "===== does a read describe it as the predecessor again? ====="
$E --root wd ls 2>&1 | head -2; echo "ls exit=$?"
echo
echo "===== and does migrate mint a fresh code rather than pointing at the one that is gone? ====="
out=$($E --root wd migrate 2>&1); echo "migrate exit=$?"
printf '%s\n' "$out" | tail -3
new=$(printf '%s' "$out" | sed -n 's/.*CONFIRM \([A-Z0-9]*\).*/\1/p')
echo "new code: $new  (old: $CODE)"
[ "$new" != "$CODE" ] && echo "FRESH: the spent code was not reused" || echo "REUSED: the old code came back"
echo
echo "===== and the old code is refused ====="
$E --root wd confirm "CONFIRM $CODE" 2>&1 | head -1; echo "exit=$?"
echo
echo "===== an orphan stage: a destination with no manifest and no challenge ====="
rm -rf orph && cp -a checkpoints/pre-confirm orph
rm -f "orph/.engr/local/challenges/$CODE.json"
mkdir -p orph/.engr/local/migration/destination/objects
echo '{"not":"a manifest"}' > orph/.engr/local/migration/destination/objects/stray.json
rm -f orph/.engr/local/migration/manifest.json
$E --root orph ls 2>&1 | head -2; echo "ls exit=$?"
out=$($E --root orph migrate 2>&1); echo "migrate exit=$?"
printf '%s\n' "$out" | tail -2
