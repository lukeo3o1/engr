#!/bin/sh
# What the round-31 repairs newly touch, probed for the state each one added.
#
#   a) a stage holding only the new published-over.json and nothing else
#   b) an Object path that exists and is not a regular file
#   c) the same, asked of every surface, so none of them disagrees
set -u
cd /audit/r6
E=/audit/bin/engr-current
P=/audit/bin/engr-prev

echo "===== a) a stage left holding only published-over.json ====="
rm -rf ns1 && cp -a checkpoints/migrated ns1
mkdir -p ns1/.engr/local/migration
printf '["objects/whatever.json"]\n' > ns1/.engr/local/migration/published-over.json
echo "-- what a read says --"
$E --root ns1 ls --all 2>&1 | head -2; echo "ls exit=$?"
$E --root ns1 verify 2>&1 | head -2; echo "verify exit=$?"
echo "-- and is the leftover discarded or does it wedge? --"
ls ns1/.engr/local/migration 2>/dev/null || echo "(stage gone)"

echo
echo "===== b) an Object path that is a directory, not a file ====="
rm -rf ns2 && cp -a checkpoints/migrated ns2
T=ns2/.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
rm "$T" && mkdir "$T"
for surface in "ls --all" "ls --verify" "verify" "show 01a05e55-74"; do
  out=$($E --root ns2 $surface 2>&1); code=$?
  printf '  %-16s exit=%-3s %s\n' "$surface" "$code" "$(printf '%s' "$out" | head -1 | cut -c1-110)"
done
echo "-- and the previously audited head, for comparison --"
for surface in "ls --all" "ls --verify" "verify"; do
  out=$($P --root ns2 $surface 2>&1); code=$?
  printf '  %-16s exit=%-3s %s\n' "$surface" "$code" "$(printf '%s' "$out" | head -1 | cut -c1-110)"
done
