#!/bin/sh
set -u
cd /audit/r4
E=/audit/bin/engr-current
BASE=crash-runs/baseline          # a clean migrated generation-1 workspace
AU=01a05e55-74eb-7370-b771-0008bd71d149
EV=01a05e55-e62b-75c3-8b32-6c388dec4c4b
rm -rf adv && mkdir adv

# r3's lesson: a literal-match mutation can stop testing without saying so —
# the RefDigest fix moved a pinned constant, sed matched nothing, and the
# scenario "passed" at exit 0 having done nothing. Every probe now hashes the
# workspace either side of its mutation and reports NO-OP! rather than a pass.
probe() { # name, mutation-shell, command...
  name=$1; shift; mut=$1; shift
  rm -rf adv/w && cp -a "$BASE" adv/w
  before=$(find adv/w/.engr -type f | LC_ALL=C sort | xargs sha256sum | sha256sum | cut -d' ' -f1)
  ( cd adv/w && eval "$mut" )
  after=$(find adv/w/.engr -type f | LC_ALL=C sort | xargs sha256sum | sha256sum | cut -d' ' -f1)
  if [ "$before" = "$after" ]; then mutated="NO-OP!"; else mutated="mutated"; fi
  out=$("$@" 2>&1); code=$?
  printf '%-34s %-8s exit=%-3s %s\n' "$name" "$mutated" "$code" "$(printf '%s' "$out" | tr '\n' '|' | cut -c1-140)"
}

echo "===== integrity ====="
probe "object tamper (title)"  "sed -i 's/Migration continuity audit/Migration continuity AUDIT/' .engr/objects/$AU.json" $E --root adv/w show $AU
probe "section tamper (text)"  "sed -i 's/quietly invented/quietly INVENTED/' .engr/objects/$AU.json"                        $E --root adv/w verify
probe "event tamper (rev)"     "sed -i 's/\"rev\":1/\"rev\":9/' .engr/eventstore/objects/$AU.jsonl"                          $E --root adv/w verify
probe "event moved to another stream" "cp .engr/eventstore/objects/$AU.jsonl .engr/eventstore/objects/$EV.jsonl"             $E --root adv/w verify
probe "truncated object file"  "head -c 120 .engr/objects/$AU.json > /tmp/x && mv /tmp/x .engr/objects/$AU.json"             $E --root adv/w show $AU
probe "truncated event file"   "head -c 200 .engr/eventstore/objects/$AU.jsonl > /tmp/x && mv /tmp/x .engr/eventstore/objects/$AU.jsonl" $E --root adv/w verify
probe "malformed JSON"         "echo 'not json' > .engr/objects/$AU.json"                                                    $E --root adv/w show $AU
probe "duplicate JSON key"     "sed -i 's/\"rev\":1,/\"rev\":1,\"rev\":2,/' .engr/objects/$AU.json"                          $E --root adv/w show $AU
probe "noncanonical JSON (reordered)" "sed -i 's/^{\"digest\"/{ \"digest\"/' .engr/objects/$AU.json"                         $E --root adv/w show $AU
probe "unknown member added"   "sed -i 's/\"rev\":1,/\"rev\":1,\"surprise\":true,/' .engr/objects/$AU.json"                  $E --root adv/w show $AU
probe "filename != embedded id" "cp .engr/objects/$AU.json .engr/objects/01a05e55-0000-7000-8000-000000000000.json"          $E --root adv/w ls --all

echo
echo "===== references ====="
probe "invalid Ref digest"     "sed -i 's/\"digest\":\"1:088da847/\"digest\":\"1:00000000/' .engr/objects/$EV.json"          $E --root adv/w verify
probe "Ref repointed to another section" "sed -i 's|obj:01m1f5ax7bedrbew8012yq3ma9:1|obj:01m1f5ax7bedrbew8012yq3ma9:2|' .engr/objects/$EV.json" $E --root adv/w verify
probe "Ref target commit unavailable" "sed -i 's/221a5db38924d39449ed1c76a3beddf11e302113/0000000000000000000000000000000000000000/g' .engr/objects/$EV.json" $E --root adv/w verify
probe "Ref target raw UUID (old dialect)" "sed -i 's|obj:01m1f5ax7bedrbew8012yq3ma9:1|obj:$AU:1|' .engr/objects/$EV.json"    $E --root adv/w verify
probe "Ref fields list emptied" "sed -i 's/\"fields\":\[\"based_on\",\"refs\",\"text\"\]/\"fields\":[]/' .engr/objects/$EV.json" $E --root adv/w verify
probe "Ref fields gains an unknown field" "sed -i 's/\"fields\":\[\"based_on\",\"refs\",\"text\"\]/\"fields\":[\"based_on\",\"invented\",\"refs\",\"text\"]/' .engr/objects/$EV.json" $E --root adv/w verify
