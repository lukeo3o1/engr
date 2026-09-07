#!/bin/sh
# Round 31's second P2: flags an action cannot carry, read and then discarded.
# Four actions that carry no wording against the four flags that build Content,
# on a workspace where every input is valid — the target of the --ref exists at
# HEAD, --based-on HEAD resolves, and every action is one its subject could
# really take. Both binaries, and the workspace is hashed either side so a
# Challenge minted by the old one is visible as a change rather than inferred.
set -u
cd /audit/r8
CUR=/audit/bin/engr-current
PREV=/audit/bin/engr-prev
REF=01a05e55-e62b-75c3-8b32-6c388dec4c4b:1
printf 'wording from a file\n' > scratch/wording.txt

probe() {
  bin=$1; label=$2
  rm -rf cf && cp -a checkpoints/migrated cf
  before=$(find cf/.engr -type f | LC_ALL=C sort | xargs sha256sum | sha256sum | cut -d' ' -f1)
  echo "===== $label ====="
  for action in "--object 01a05e55-74 --delete 1" \
                "--object 01a05e55-74 --close" \
                "--object 01a05e55-e6 --reopen" \
                "--object 01a05e55-ee --classify --type design --state draft"; do
    for carried in "--text wording-it-cannot-carry" \
                   "--text-file /audit/r8/scratch/wording.txt" \
                   "--based-on HEAD" \
                   "--ref $REF text"; do
      out=$($bin --root cf prepare $action $carried 2>&1); code=$?
      printf '  %-52s exit=%-3s %s\n' "$(echo "$action" | cut -d' ' -f3-) $(echo "$carried" | cut -d' ' -f1)" \
        "$code" "$(printf '%s' "$out" | head -1 | cut -c1-96)"
    done
  done
  after=$(find cf/.engr -type f | LC_ALL=C sort | xargs sha256sum | sha256sum | cut -d' ' -f1)
  if [ "$before" = "$after" ]; then echo "  workspace unchanged"; else echo "  WORKSPACE CHANGED: something was minted"; fi
}

probe "$PREV" "prev (a2568f2)"
probe "$CUR" "current (847c8a8)"
