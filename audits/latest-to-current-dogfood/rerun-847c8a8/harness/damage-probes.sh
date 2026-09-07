#!/bin/sh
# What every surface says about each prepared damage, at both heads.
#
# The workspaces come from the host-side preps (node is not in this image);
# dep-g comes from dep-g.sh, because a genuinely absent Section needs an
# admitted deletion Event, not an edit.
set -u
cd /audit/r8
C=/audit/bin/engr-current
P=/audit/bin/engr-prev
AU=01a05e55-74eb-7370-b771-0008bd71d149
EE=01a05e55-ee46-7a91-983f-fa79b297b10c

echo "===== dep-g: a Section removed through an admitted deletion Event ====="
# r7 found that `--section 1 --delete` deletes nothing -- the flag is
# `--delete 1` -- and rebuilt this control in dep-g.sh. It left the wrong
# spelling standing HERE. So this block prepared nothing, confirmed an empty
# code ("the response must be exactly `CONFIRM <code>`"), and then reported a
# verify exit for whatever dep-g.sh happened to have left behind. Require the
# built state and check it, rather than building it a second, different way.
if [ ! -d dep-g ]; then
  echo "REFUSING: dep-g does not exist -- run dep-g.sh first"; exit 1
fi
# `"id":[0-9]*` also matches the Object's own `"id":"<uuid>"`, because `*` matches
# zero digits -- it counted one section too many. Require at least one digit.
secs=$(grep -o '"id":[0-9][0-9]*' "dep-g/.engr/objects/$AU.json" | wc -l)
evs=$(wc -l < "dep-g/.engr/eventstore/objects/$AU.jsonl")
del=$(grep -c 'section.deleted.v1' "dep-g/.engr/eventstore/objects/$AU.jsonl" || true)
printf '  sections=%s  events=%s  section.deleted.v1=%s\n' "$secs" "$evs" "$del"
if [ "$del" -lt 1 ]; then
  echo "REFUSING: dep-g holds no admitted deletion -- run dep-g.sh first"; exit 1
fi
$C --root dep-g verify >/dev/null 2>&1; echo "  dep-g verify exit=$?"

ask() {
  ws=$1; what=$2; obj=$3
  echo
  echo "--- $ws: $what ---"
  for bin in "$C current" "$P prev"; do
    set -- $bin
    b=$1; who=$2
    for surface in "ls" "ls --sections" "ls --verify" "verify" "show $obj"; do
      out=$($b --root "$ws" $surface 2>&1); code=$?
      printf '  %-8s %-14s exit=%-3s %s\n' "$who" "$surface" "$code" \
        "$(printf '%s' "$out" | grep -iE 'MISSING|TAMPER|DIVERG|UNREPLAY|FAIL|error|not what|no stored' | head -1 | cut -c1-96)"
    done
  done
}

ask dep-b "projection removed, stream intact"        01a05e55-74
ask dep-c "projection present and unreadable"        01a05e55-74
ask dep-d "one Section edited, nothing resealed"     01a05e55-74
ask dep-e "a Section removed and the Object resealed" 01a05e55-74
ask dep-f "the same removal, unsealed"               01a05e55-74
ask dep-g "removed through an admitted deletion"     01a05e55-74
ask ev    "an Event id in the uppercase spelling"    01a05e55-ee
ask tail  "a sealed, contiguous Event that cannot be applied" 01a05e55-74

echo
echo "===== and what repair says about each ====="
for ws in dep-b dep-c dep-d dep-e dep-f dep-g tail; do
  out=$($C --root "$ws" repair 01a05e55-74 2>&1); code=$?
  printf '  %-6s exit=%-3s %s\n' "$ws" "$code" \
    "$(printf '%s' "$out" | grep -iE 'Integrity|error' | head -1 | cut -c1-104)"
done
