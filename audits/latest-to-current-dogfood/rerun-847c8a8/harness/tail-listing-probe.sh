#!/bin/sh
# A projection that is behind its history, asked of every surface: navigation
# must stay cheap and say `unchecked` rather than claim soundness, and the
# assessment surfaces must call it behind rather than missing -- the control
# that keeps "reported" from meaning "reported as anything".
set -u
cd /audit/r8
C=/audit/bin/engr-current
P=/audit/bin/engr-prev
for bin in "$C current" "$P prev"; do
  set -- $bin
  b=$1; who=$2
  for surface in "ls" "ls --sections" "ls --verify" "verify" "show 01a05e55-ee"; do
    out=$($b --root tv $surface 2>&1); code=$?
    printf '  %-8s %-16s exit=%-3s %s\n' "$who" "$surface" "$code" \
      "$(printf '%s' "$out" | grep -iE '01a05e55-ee|unproject|reconcil|MISSING|note' | head -1 | cut -c1-100)"
  done
  echo
done
echo "--- and the JSON integrity member for the behind Object ---"
$C --root tv show 01a05e55-ee --format json 2>/dev/null | grep -E '"integrity"|"rev"'
