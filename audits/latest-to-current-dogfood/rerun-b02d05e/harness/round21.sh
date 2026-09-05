#!/bin/sh
# Round 21's own state, rebuilt: a crash between writing VERSION and sweeping the
# spent Challenge leaves a *current* workspace beside a Challenge and a stage
# that both still read fine. Retyping the code the tool is still showing was the
# natural recovery and, before the fix, the destructive one.
#
# The sweep alone never reaches it, because the Challenge is removed before the
# stage is. So: kill inside the window, keep only the runs that landed there,
# then move the workspace on by one admitted revision before retyping.
set -u
cd /audit/r4
E=/audit/bin/engr-current
CODE=FBR8Z2
rm -rf r21 && mkdir r21
hash_of() { (cd "$1" && find .engr/objects .engr/eventstore -type f | LC_ALL=C sort | xargs sha256sum | sha256sum | cut -d' ' -f1); }

found=""
for ms in 1000 1010 1020 1030 1040 1015 1025 1035 1005; do
  rm -rf r21/w && cp -a checkpoints/pre-confirm r21/w
  secs=$(awk -v m="$ms" 'BEGIN{printf "%.3f", m/1000}')
  timeout -s KILL "$secs" $E --root r21/w confirm "CONFIRM $CODE" >/dev/null 2>&1
  if [ -f r21/w/.engr/VERSION ] && [ -f "r21/w/.engr/local/challenges/$CODE.json" ]; then
    echo "landed in round 21's window at ${ms}ms: VERSION written, Challenge still on disk, stage still on disk"
    found=$ms; break
  fi
done
[ -n "$found" ] || { echo "never landed in the window; widen the sweep"; exit 1; }

echo "--- the workspace reads as current, and the spent question is still there ---"
$E --root r21/w ls --all; echo "ls exit=$?"
ls r21/w/.engr/local/challenges/
test -d r21/w/.engr/local/migration/destination && echo "stage: still on disk"

echo
echo "--- move it on by one admitted revision, the way a day's work would ---"
out=$($E --root r21/w prepare --object 01a05e55-ee --add --no-based-on --text "Work admitted after the migration completed, which the spent code must not be able to undo." 2>&1)
c=$(printf '%s' "$out" | sed -n 's/.*CONFIRM \([A-Z0-9]*\).*/\1/p')
$E --root r21/w confirm "CONFIRM $c" | head -1
before=$(hash_of r21/w)
echo "record hash before retyping the spent code: $before"
cp -a r21/w r21/before

echo
echo "--- now retype the code the screen is still showing ---"
$E --root r21/w confirm "CONFIRM $CODE"; echo "exit=$?"
after=$(hash_of r21/w)
echo "record hash after : $after"
if [ "$before" = "$after" ]; then echo "IDENTICAL: not one byte of the record moved"; else echo "CHANGED: the record was rewritten"; diff -r r21/before/.engr/objects r21/w/.engr/objects; fi
echo
echo "--- and the workspace still verifies ---"
$E --root r21/w verify; echo "verify exit=$?"
echo "--- leftovers ---"
ls r21/w/.engr/local/ 2>/dev/null
test -d r21/w/.engr/local/migration && echo "stage: STILL PRESENT" || echo "stage: retired"
