#!/bin/sh
# Round 35's P2, built without a timed kill.
#
# A repair appends its Event and then saves the projection. The state between
# those two writes is: a durable `object.repaired.v1`, the still-damaged
# projection, and the Challenge not yet discarded. It is constructed here by
# saving those two files, confirming a real repair, and restoring only them --
# the EventStore the confirm produced is kept.
set -u
cd /audit/r8
E=/audit/bin/engr-current
ID=01a05e55-74eb-7370-b771-0008bd71d149
T=.engr/objects/$ID.json
S=.engr/eventstore/objects/$ID.jsonl
code_of() { printf '%s' "$1" | sed -n 's/.*CONFIRM \([23456789ABCDEFGHJKLMNPQRSTUVWXYZ]\{6\}\).*/\1/p' | head -1; }

probe() { # $1 = source workspace, $2 = label
  ws=$1; what=$2
  echo
  echo "===== $what ====="
  rm -rf r35 && cp -a "$ws" r35
  out=$($E --root r35 repair 01a05e55-74 2>&1)
  c=$(code_of "$out")
  [ -n "$c" ] || { echo "  no code offered: $(printf '%s' "$out" | head -1)"; return; }
  # The two files the interrupted write would not have touched yet.
  mkdir -p r35/saved
  cp "r35/$T" r35/saved/projection.json 2>/dev/null || echo "  (no projection to save)"
  cp "r35/.engr/local/challenges/$c.json" r35/saved/challenge.json
  $E --root r35 confirm "CONFIRM $c" >/dev/null 2>&1
  echo "  the uninterrupted repair landed: rev $(sed -n 's/.*"rev":\([0-9]*\).*/\1/p' "r35/$T" | head -1), events $(wc -l < "r35/$S")"
  repaired=$(sha256sum "r35/$T" | cut -d' ' -f1)
  # Rewind exactly the two writes the crash would have preceded.
  if [ -f r35/saved/projection.json ]; then cp r35/saved/projection.json "r35/$T"; else rm -f "r35/$T"; fi
  mkdir -p r35/.engr/local/challenges
  cp r35/saved/challenge.json "r35/.engr/local/challenges/$c.json"
  rm -rf r35/saved
  echo "  the crash state: projection rev $(sed -n 's/.*"rev":\([0-9]*\).*/\1/p' "r35/$T" 2>/dev/null | head -1 || echo absent), events $(wc -l < "r35/$S"), challenge $c on disk"
  echo "  -- what the pending code's screen says --"
  $E --root r35 candidate "$c" 2>&1 | grep -iE 'ALREADY APPLIED|Retype|UNANSWERABLE|SETTLED|DEAD|Type this' | head -2 | sed 's/^/     /'
  echo "  -- and retyping it --"
  __out=$($E --root r35 confirm "CONFIRM $c" 2>&1); __rc=$?
  printf '%s
' "$__out" | head -2 | sed 's/^/     /'
  echo "     exit=$__rc"
  now=$(sha256sum "r35/$T" 2>/dev/null | cut -d' ' -f1)
  [ "$now" = "$repaired" ] && echo "     projection: finished (identical to the uninterrupted repair)" \
                           || echo "     projection: STILL DAMAGED"
  echo "     events now: $(wc -l < "r35/$S")"
  [ -f "r35/.engr/local/challenges/$c.json" ] && echo "     challenge: STILL PENDING" || echo "     challenge: cleared"
  $E --root r35 verify >/dev/null 2>&1; echo "     verify exit=$?"
}

probe st-tampered  "unsealed projection"
probe st-divergent "resealed-divergent projection"
probe st-missing   "absent projection (the control that already worked)"
