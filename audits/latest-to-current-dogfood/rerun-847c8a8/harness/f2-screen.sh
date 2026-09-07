#!/bin/sh
# F-2, with the whole screen rather than its first six lines: does the pending
# code's re-render still offer the code after the damage is gone?
set -u
cd /audit/r8
C=/audit/bin/engr-current
rm -rf f2 && cp -a st-divergent f2
T=f2/.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
out=$($C --root f2 repair 01a05e55-74 2>&1)
c=$(printf '%s' "$out" | sed -n 's/.*CONFIRM \([23456789ABCDEFGHJKLMNPQRSTUVWXYZ]\{6\}\).*/\1/p' | head -1)
echo "pending code: $c"
cp checkpoints/migrated/.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json "$T"
echo "===== the whole candidate screen, untruncated ====="
$C --root f2 candidate "$c" 2>&1
echo "===== and confirming it ====="
$C --root f2 confirm "CONFIRM $c" 2>&1
