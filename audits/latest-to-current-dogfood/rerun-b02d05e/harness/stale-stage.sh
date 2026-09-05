#!/bin/sh
# The sharper version of the moved-source case, with no hand-editing anywhere.
#
# A crash inside the publication window can stop *before* the barrier that locks
# the released build out: format.json is still there, so the predecessor tool
# still reads and writes. A person whose migration died therefore has a working
# old workspace, and using it is the obvious thing to do. What happens to that
# work when the migration is later resumed?
set -u
cd /audit/r4
CUR=/audit/bin/engr-current
OLD=/audit/bin/engr-latest
CODE=FBR8Z2
export HOME=/audit/home GIT_CONFIG_GLOBAL=/audit/home/.gitconfig
rm -rf ss && mkdir ss

cp -a checkpoints/pre-confirm ss/w
timeout -s KILL 0.850 $CUR --root ss/w confirm "CONFIRM $CODE" >/dev/null 2>&1
echo "crashed with: VERSION=$([ -f ss/w/.engr/VERSION ] && echo yes || echo no)  format.json=$([ -f ss/w/.engr/format.json ] && echo yes || echo no)  staged_dest=$([ -d ss/w/.engr/local/migration/destination ] && echo yes || echo no)  challenge=$([ -f ss/w/.engr/local/challenges/$CODE.json ] && echo yes || echo no)"

echo
echo "===== the released build still works here ====="
$OLD --root ss/w ls --all; echo "exit=$?"

echo
echo "===== so a day's work goes in through its own Human Gate ====="
out=$($OLD --root ss/w prepare --object 01a05e55-74 --add --no-based-on --text "Admitted with the released tool after the migration crashed, and before it was resumed." 2>&1)
printf '%s\n' "$out" | tail -2
c=$(printf '%s' "$out" | sed -n 's/.*CONFIRM \([A-Z0-9]*\).*/\1/p')
$OLD --root ss/w confirm "CONFIRM $c" 2>&1 | head -2; echo "exit=$?"
echo "-- it is really in the predecessor record --"
$OLD --root ss/w show 01a05e55-74 2>&1 | grep -c "Admitted with the released tool"
$OLD --root ss/w verify 2>&1 | head -3
sha_before=$(sha256sum ss/w/.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json | cut -d' ' -f1)
echo "predecessor object digest: $sha_before"

echo
echo "===== now resume the migration that was already staged ====="
$CUR --root ss/w confirm "CONFIRM $CODE" 2>&1 | head -3; echo "exit=$?"

echo
echo "===== is the admitted work still there? ====="
if $CUR --root ss/w show 01a05e55-74 2>&1 | grep -q "Admitted with the released tool"; then
  echo "PRESENT: the work survived the resume"
else
  echo "GONE: the resume published a stage that predates it"
fi
$CUR --root ss/w ls --all; echo "ls exit=$?"
$CUR --root ss/w verify; echo "verify exit=$?"
echo "-- and the old history that would have proved it --"
ls ss/w/.engr/events 2>/dev/null || echo "(predecessor events discarded)"
