#!/bin/sh
# The five answers the classifier has, asked of every surface that classifies an
# Object, at both heads.
#
# Round 32's rule: "every surface that classifies an Object owes the same
# answer". It also re-pointed `show --format json` at `object_fault`, which is a
# new consumer for the four answers that already existed -- so the four are
# measured here too, not just the new one.
set -u
cd /audit/r8
C=/audit/bin/engr-current
P=/audit/bin/engr-prev
ID=01a05e55-74eb-7370-b771-0008bd71d149

# The control state: a projection that is merely behind its history. Built here
# because it needs an admitted mutation, and node is not in this image.
echo "===== building st-behind: admit, then rewind the projection ====="
rm -rf st-behind && cp -a checkpoints/migrated st-behind
B=st-behind/.engr/objects/$ID.json
cp "$B" st-behind/behind.json
out=$($C --root st-behind prepare --object 01a05e55-74 --add --no-based-on \
      --text "Admitted, and the projection never caught up." 2>&1)
code=$(printf '%s' "$out" | sed -n 's/.*CONFIRM \([A-Z0-9]*\).*/\1/p' | head -1)
$C --root st-behind confirm "CONFIRM $code" | head -1
before=$(sha256sum "$B" | cut -d' ' -f1)
mv st-behind/behind.json "$B"
after=$(sha256sum "$B" | cut -d' ' -f1)
[ "$before" = "$after" ] && echo "NO-OP! the rewind changed nothing" || echo "rewound: $before -> $after"

probe() {
  bin=$1; ws=$2; who=$3
  printf '  %-9s %-12s' "$who" "$ws"
  $bin --root "$ws" verify >/dev/null 2>&1; v=$?
  $bin --root "$ws" ls --verify >/dev/null 2>&1; l=$?
  $bin --root "$ws" show 01a05e55-74 >/dev/null 2>&1; s=$?
  $bin --root "$ws" show 01a05e55-74 --format json >/dev/null 2>&1; j=$?
  key=$($bin --root "$ws" show 01a05e55-74 --format json 2>/dev/null \
        | sed -n 's/.*"integrity": *"\([a-z_]*\)".*/\1/p' | head -1)
  row=$($bin --root "$ws" ls --verify 2>/dev/null | grep -i '01a05e55-74' | head -1 \
        | sed 's/  */ /g' | cut -c1-60)
  bang=$($bin --root "$ws" show 01a05e55-74 2>/dev/null | grep '^!!' | head -1 | cut -c1-72)
  printf 'verify=%-2s ls--verify=%-2s show=%-2s json=%-2s integrity=%-19s\n' \
    "$v" "$l" "$s" "$j" "${key:-<none>}"
  printf '    ls row : %s\n' "${row:-<not named>}"
  printf '    show !!: %s\n' "${bang:-<no banner>}"
}

echo
echo "===== the matrix: exit codes and the word each surface uses ====="
for ws in st-ok st-behind st-missing st-tampered st-divergent st-unreplayable; do
  probe "$C" "$ws" current
  probe "$P" "$ws" prev
  echo
done
