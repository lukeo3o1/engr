#!/bin/sh
# Confirming the repair the screen names: does the projection come back, and are
# the bytes exactly what the record already held?
set -u
cd /audit/r7
E=/audit/bin/engr-current
rm -rf rx && cp -a as rx
T=rx/.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
echo "--- before: is the file there? ---"
[ -e "$T" ] && echo "PRESENT (wrong)" || echo "absent"
echo "--- what checkpoints/migrated holds for the same Object, for comparison ---"
sha256sum checkpoints/migrated/.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json | cut -d' ' -f1
echo "--- the repair screen ---"
out=$($E --root rx repair 01a05e55-74 2>&1); echo "$out" | head -6
code=$(printf '%s' "$out" | sed -n 's/.*CONFIRM \([A-Z0-9]*\).*/\1/p' | head -1)
echo "--- confirm ---"
$E --root rx confirm "CONFIRM $code"; echo "exit=$?"
echo "--- after: the file, and its bytes ---"
[ -e "$T" ] && sha256sum "$T" | cut -d' ' -f1 || echo "STILL ABSENT (wrong)"
echo "--- and does the workspace verify now? ---"
$E --root rx verify; echo "verify exit=$?"
echo "--- the stream, so the repair is visible as an admitted event ---"
sed -n 's/.*"rev":\([0-9]*\).*"type":"\([a-z._0-9]*\)".*/rev=\1 type=\2/p' rx/.engr/eventstore/objects/01a05e55-74eb-7370-b771-0008bd71d149.jsonl
