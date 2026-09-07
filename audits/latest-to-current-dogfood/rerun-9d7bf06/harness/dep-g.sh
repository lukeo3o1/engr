#!/bin/sh
# Round 29's third control: a Section that is genuinely absent because an
# admitted deletion Event removed it -- against dep-e (removed and resealed) and
# dep-f (removed unsealed). The first probe of this spelled the flag wrong and
# the deletion never happened, so every surface answered about an untouched
# workspace: the premise has to be checked, not assumed.
set -u
cd /audit/r7
C=/audit/bin/engr-current
P=/audit/bin/engr-prev
T=dep-g/.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
rm -rf dep-g && cp -a checkpoints/migrated dep-g
before=$(sha256sum "$T" | cut -d' ' -f1)
echo "sections before: $(grep -o '"id":[0-9]' "$T" | wc -l)"
out=$($C --root dep-g prepare --object 01a05e55-74 --delete 1 2>&1)
printf '%s\n' "$out" | head -8
code=$(printf '%s' "$out" | sed -n 's/.*CONFIRM \([23456789ABCDEFGHJKLMNPQRSTUVWXYZ]\{6\}\).*/\1/p' | head -1)
[ -n "$code" ] || { echo "NO CODE -- the deletion never reached the gate"; exit 1; }
$C --root dep-g confirm "CONFIRM $code"; echo "confirm exit=$?"
after=$(sha256sum "$T" | cut -d' ' -f1)
[ "$before" = "$after" ] && { echo "NO-OP! nothing was deleted"; exit 1; } || echo "deleted: $before -> $after"
echo "sections after : $(grep -o '"id":[0-9]' "$T" | wc -l)"

echo
echo "--- every surface, at both heads ---"
for bin in "$C current" "$P prev"; do
  set -- $bin
  b=$1; who=$2
  for surface in "ls --verify" "verify" "show 01a05e55-74"; do
    out=$($b --root dep-g $surface 2>&1); c=$?
    printf '  %-8s %-14s exit=%-3s %s\n' "$who" "$surface" "$c" \
      "$(printf '%s' "$out" | head -2 | tr '\n' '|' | cut -c1-92)"
  done
done
echo "--- and repair, which must refuse: nothing here is damaged ---"
$C --root dep-g repair 01a05e55-74 2>&1 | head -1; echo "  repair exit=$?"
