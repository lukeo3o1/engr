#!/bin/sh
# The machine-readable side of the repair screen: it must not say something the
# screen does not, and must not stay silent where the screen speaks.
#
# r7 minted a code with a plain `repair`, then called `repair --json`, which
# mints a SECOND code and retires the first, and then looked the first one up.
# Every `candidate` call refused with exit 3 -- a refusal for the wrong reason,
# which hides whatever the probe was for. The code now comes from the JSON that
# minted it.
#
# Note `repair` spells it `--json` while `show` and `backlog show` spell it
# `--format json`, and `candidate` has no machine-readable form at all.
set -u
cd /audit/r8
C=/audit/bin/engr-current

for pair in "st-missing absent" "st-divergent divergent" "st-tampered tampered"; do
  set -- $pair
  ws=$1; what=$2
  echo "===== $what ====="
  rm -rf rj && cp -a "$ws" rj

  json=$($C --root rj repair --json 01a05e55-74 2>&1); jrc=$?
  printf '%s\n' "$json" | head -40
  echo "  repair --json exit=$jrc"

  c=$(printf '%s' "$json" | sed -n 's/.*"id": *"\([23456789ABCDEFGHJKLMNPQRSTUVWXYZ]\{6\}\)".*/\1/p' | head -1)
  if [ -z "$c" ]; then
    echo "  REFUSING: the JSON minted no challenge id, so there is no pending code to render"
    echo
    continue
  fi
  echo "  the code the JSON minted: $c"

  echo "-- the same pending code, rendered as the human screen --"
  screen=$($C --root rj candidate "$c" 2>&1); src=$?
  printf '%s\n' "$screen" | head -40
  echo "  candidate exit=$src"

  echo "-- do the two agree? --"
  jdig=$(printf '%s' "$json" | sed -n 's/.*"digest": *"\(1:[0-9a-f]*\)".*/\1/p' | head -1)
  printf '  JSON challenge digest : %s\n' "$jdig"
  printf '  screen offers the code: %s\n' \
    "$(printf '%s' "$screen" | grep -c "CONFIRM $c")"
  printf '  screen names a revision: %s\n' \
    "$(printf '%s' "$screen" | grep -c 'admitted as rev')"
  printf '  JSON names the same object: %s\n' \
    "$(printf '%s' "$json" | grep -c '01a05e55-74eb-7370-b771-0008bd71d149')"
  echo
done
