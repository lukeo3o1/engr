#!/bin/sh
# The states the round-33 repair screen can be rendered in that the ordinary
# path does not reach, each asked through `candidate <code>` -- the re-render the
# new rule says must tell the same truth as the first screen.
set -u
cd /audit/r7
C=/audit/bin/engr-current
NEW=$(ls project/.engr/objects | grep -v '^01a05e55' | head -1 | sed 's/\.json$//')
echo "the Object with no Sections at all: $NEW"

code_of() { printf '%s' "$1" | sed -n 's/.*CONFIRM \([23456789ABCDEFGHJKLMNPQRSTUVWXYZ]\{6\}\).*/\1/p' | head -1; }

echo
echo "===== 1. an absent projection whose Object has zero Sections ====="
rm -rf re1 && cp -a project re1
rm -f "re1/.engr/objects/$NEW.json"
$C --root re1 repair "$NEW" 2>&1; echo "  exit=$?"

echo
echo "===== 2. the projection comes back while the code is pending ====="
rm -rf re2 && cp -a checkpoints/migrated re2
T=re2/.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
cp "$T" /tmp/back.json && rm "$T"
out=$($C --root re2 repair 01a05e55-74 2>&1)
c=$(code_of "$out")
echo "  pending code: $c"
cp /tmp/back.json "$T"
echo "  -- and what the pending code renders now --"
$C --root re2 candidate "$c" 2>&1 | head -6; echo "  exit=$?"
echo "  -- and whether confirming it is still allowed --"
$C --root re2 confirm "CONFIRM $c" 2>&1 | head -2; echo "  exit=$?"

echo
echo "===== 3. history stops replaying while the code is pending ====="
rm -rf re3 && cp -a checkpoints/migrated re3
T3=re3/.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
S3=re3/.engr/eventstore/objects/01a05e55-74eb-7370-b771-0008bd71d149.jsonl
rm "$T3"
out=$($C --root re3 repair 01a05e55-74 2>&1)
c3=$(code_of "$out")
echo "  pending code: $c3"
cp tail/.engr/eventstore/objects/01a05e55-74eb-7370-b771-0008bd71d149.jsonl "$S3"
echo "  -- and what the pending code renders now --"
$C --root re3 candidate "$c3" 2>&1 | head -6; echo "  exit=$?"
echo "  -- and whether confirming it is still allowed --"
$C --root re3 confirm "CONFIRM $c3" 2>&1 | head -2; echo "  exit=$?"

echo
echo "===== 4. the damaged screen through candidate, for the same MUSTs ====="
rm -rf re4 && cp -a st-divergent re4
out=$($C --root re4 repair 01a05e55-74 2>&1)
c4=$(code_of "$out")
$C --root re4 candidate "$c4" 2>&1
echo "  exit=$?"
