#!/bin/sh
# Confirm each repair and measure what actually landed: the file, its bytes
# against the record the audit carried forward, the resulting revision, the
# Event appended, and whether the workspace verifies afterwards.
set -u
cd /audit/r7
C=/audit/bin/engr-current
ID=01a05e55-74eb-7370-b771-0008bd71d149
GOLD=$(sha256sum checkpoints/migrated/.engr/objects/$ID.json | cut -d' ' -f1)
echo "the migrated bytes this audit carried forward: $GOLD"

run() {
  src=$1; what=$2
  echo
  echo "===== $what ====="
  rm -rf rc && cp -a "$src" rc
  T=rc/.engr/objects/$ID.json
  [ -e "$T" ] && echo "before: $(sha256sum "$T" | cut -d' ' -f1)" || echo "before: absent"
  out=$($C --root rc repair 01a05e55-74 2>&1)
  code=$(printf '%s' "$out" | sed -n 's/.*CONFIRM \([A-Z0-9]*\).*/\1/p' | head -1)
  $C --root rc confirm "CONFIRM $code"; echo "confirm exit=$?"
  if [ -e "$T" ]; then
    now=$(sha256sum "$T" | cut -d' ' -f1)
    echo "after : $now"
    [ "$now" = "$GOLD" ] && echo "        (identical to the migrated bytes)" \
                         || echo "        (differs from the migrated bytes)"
    echo "stored rev: $(sed -n 's/.*"rev":\([0-9]*\).*/\1/p' "$T" | head -1)"
  else
    echo "after : STILL ABSENT (wrong)"
  fi
  echo "stream:"
  sed -n 's/.*"rev":\([0-9]*\).*"type":"\([a-z._0-9]*\)".*/  rev=\1 type=\2/p' \
    rc/.engr/eventstore/objects/$ID.jsonl
  $C --root rc verify >/dev/null 2>&1; echo "verify exit=$?"
  $C --root rc show 01a05e55-74 >/dev/null 2>&1; echo "show   exit=$?"
  $C --root rc show 01a05e55-74 --format json 2>/dev/null | grep '"integrity"'
}

run st-missing   "absent projection"
run st-divergent "divergent projection"
run st-tampered  "tampered projection"
