#!/bin/sh
# F7's class, re-surveyed: the same idea spelled differently across the surface.
#
# This run found `repair --json` beside `show --format json` and `backlog show
# --format json`, and `candidate` with no machine-readable form at all -- which
# is why r7's repair-json probe looked up a retired code and refused at exit 3
# for three runs' worth of evidence without anyone noticing.
#
# Ask every command that has a help page which spelling it takes.
set -u
E=/audit/bin/engr-current
cd /audit/r8/project

printf '%-34s %-14s %s\n' COMMAND SPELLING NOTE
printf '%-34s %-14s %s\n' ------- -------- ----
for c in "ls" "show" "verify" "repair" "candidate" "protocol" \
         "backlog ls" "backlog show" "work show" "collection show" "rules ls"; do
  h=$($E $c --help 2>&1)
  if printf '%s' "$h" | grep -q -- '--format'; then
    sp="--format json"
  elif printf '%s' "$h" | grep -q -- '--json'; then
    sp="--json"
  else
    sp="(none)"
  fi
  note=""
  [ "$sp" = "(none)" ] && note="no machine-readable form"
  printf '%-34s %-14s %s\n' "$c" "$sp" "$note"
done

echo
echo "--- how many distinct spellings are in use ---"
for c in "ls" "show" "verify" "repair" "candidate" "protocol" \
         "backlog ls" "backlog show" "work show" "collection show" "rules ls"; do
  h=$($E $c --help 2>&1)
  if printf '%s' "$h" | grep -q -- '--format'; then echo "--format json"
  elif printf '%s' "$h" | grep -q -- '--json'; then echo "--json"
  fi
done | sort -u
