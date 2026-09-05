#!/bin/sh
# The four Work lists: PROTOCOL now says they are omitted when empty and that an
# explicit [] is not a spelling this generation has. Every case is collected and
# printed, rather than stopping at the first refusal, so one run maps all four.
set -u
cd /audit/r4/wk
E=/audit/bin/engr-current
F=.engr/work/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
REF=engr:obj:01m1f5ax7bedrbew8012yq3ma9
GOOD='{"next_item_id":1,"state":"active","updated_at":"2026-09-05T17:43:57.378978618Z"}'

probe() {
  name=$1; body=$2
  printf '%s' "$body" > "$F"
  out=$($E work show "$REF" 2>&1); code=$?
  printf '%-46s exit=%-3s %s\n' "$name" "$code" "$(printf '%s' "$out" | tr '\n' '|' | cut -c1-120)"
}

echo "===== the sidecar the write path produces ====="
printf '%s' "$GOOD" > "$F"; cat "$F"; echo
probe "as written (four lists omitted)"        "$GOOD"
probe "explicit dependencies: []"              '{"dependencies":[],"next_item_id":1,"state":"active","updated_at":"2026-09-05T17:43:57.378978618Z"}'
probe "explicit blockers: []"                  '{"blockers":[],"next_item_id":1,"state":"active","updated_at":"2026-09-05T17:43:57.378978618Z"}'
probe "explicit items: []"                     '{"items":[],"next_item_id":1,"state":"active","updated_at":"2026-09-05T17:43:57.378978618Z"}'
probe "item with explicit commits: []"         '{"items":[{"commits":[],"id":1,"state":"pending","text":"x"}],"next_item_id":2,"state":"active","updated_at":"2026-09-05T17:43:57.378978618Z"}'
probe "item with commits omitted (must LOAD)"  '{"items":[{"id":1,"state":"pending","text":"x"}],"next_item_id":2,"state":"active","updated_at":"2026-09-05T17:43:57.378978618Z"}'
printf '%s' "$GOOD" > "$F"
