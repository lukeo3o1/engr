#!/bin/sh
# The machine-readable side of the same two screens: it must not say something
# the screen does not, and must not stay silent where the screen speaks.
set -u
cd /audit/r7
C=/audit/bin/engr-current
code_of() { printf '%s' "$1" | sed -n 's/.*CONFIRM \([23456789ABCDEFGHJKLMNPQRSTUVWXYZ]\{6\}\).*/\1/p' | head -1; }

for pair in "st-missing absent" "st-divergent divergent"; do
  set -- $pair
  ws=$1; what=$2
  echo "===== $what ====="
  rm -rf rj && cp -a "$ws" rj
  out=$($C --root rj repair 01a05e55-74 2>&1)
  c=$(code_of "$out")
  echo "-- repair --format json --"
  $C --root rj repair --json 01a05e55-74 2>&1 | head -40
  echo "  exit=$?"
  echo "-- candidate --format json --"
  $C --root rj candidate "$c" 2>&1 | head -40
  echo "  exit=$?"
  echo
done
