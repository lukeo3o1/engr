#!/bin/sh
# A repair code prepared against a damaged projection, retyped after the damage
# is gone.
#
# Round 21 ruled on the same shape for migration: a spent code the screen was
# still showing must not be the destructive path, and `migrate` now answers
# "COMPLETE ... nothing was migrated". `repair`'s eligibility question --
# "is anything damaged?" -- is asked at prepare. This asks whether it is asked
# again at confirm, at both heads, and for both damaged states.
set -u
cd /audit/r7
AU=.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
code_of() { printf '%s' "$1" | sed -n 's/.*CONFIRM \([23456789ABCDEFGHJKLMNPQRSTUVWXYZ]\{6\}\).*/\1/p' | head -1; }

probe() { # $1 = binary, $2 = label, $3 = source workspace, $4 = how it is undone
  b=/audit/bin/engr-$1
  echo
  echo "--- $2: $4 ---"
  rm -rf sr && cp -a "$3" sr
  cp checkpoints/migrated/$AU /tmp/sound.json
  out=$($b --root sr repair 01a05e55-74 2>&1)
  c=$(code_of "$out")
  if [ -z "$c" ]; then echo "  no code offered: $(printf '%s' "$out" | head -1)"; return; fi
  echo "  prepared: $c"
  cp /tmp/sound.json "sr/$AU"
  $b --root sr verify >/dev/null 2>&1; echo "  the record is sound again: verify exit=$?"
  before=$(sha256sum "sr/$AU" | cut -d' ' -f1)
  rev_before=$(sed -n 's/.*"rev":\([0-9]*\).*/\1/p' "sr/$AU" | head -1)
  ev_before=$(wc -l < sr/.engr/eventstore/objects/01a05e55-74eb-7370-b771-0008bd71d149.jsonl)
  echo "  -- what a prepare would say now --"
  $b --root sr repair 01a05e55-74 2>&1 | head -1 | sed 's/^/    /'
  echo "  -- and what retyping the code the first screen gave does --"
  $b --root sr confirm "CONFIRM $c" 2>&1 | head -1 | sed 's/^/    /'
  after=$(sha256sum "sr/$AU" | cut -d' ' -f1)
  rev_after=$(sed -n 's/.*"rev":\([0-9]*\).*/\1/p' "sr/$AU" | head -1)
  ev_after=$(wc -l < sr/.engr/eventstore/objects/01a05e55-74eb-7370-b771-0008bd71d149.jsonl)
  [ "$before" = "$after" ] && echo "    bytes: IDENTICAL" || echo "    bytes: CHANGED"
  echo "    rev $rev_before -> $rev_after, events $ev_before -> $ev_after"
  tail -1 sr/.engr/eventstore/objects/01a05e55-74eb-7370-b771-0008bd71d149.jsonl \
    | sed -n 's/.*"type":"\([a-z._0-9]*\)".*/    last event: \1/p'
}

for bin in current prev; do
  echo "############ engr-$bin ############"
  probe "$bin" "$bin" st-missing   "the projection was absent and is restored"
  probe "$bin" "$bin" st-divergent "the projection was divergent and is put back"
  probe "$bin" "$bin" st-tampered  "the projection was tampered and is put back"
done
