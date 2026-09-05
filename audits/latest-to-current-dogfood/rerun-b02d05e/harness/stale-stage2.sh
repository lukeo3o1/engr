#!/bin/sh
# The window the barrier-window probe found: the destination is staged and the
# barrier is NOT yet installed, so the released build still reads *and writes*
# the predecessor. A crash there leaves somebody a working old workspace and a
# stage nobody can see. What happens to the work they do in it?
#
# No hand-editing anywhere in this scenario: every byte is written by one of the
# two shipped binaries through its own Human Gate.
set -u
cd /audit/r4
CUR=/audit/bin/engr-current
OLD=/audit/bin/engr-latest
CODE=FBR8Z2
export HOME=/audit/home GIT_CONFIG_GLOBAL=/audit/home/.gitconfig
rm -rf ss2 && mkdir ss2

found=""
for ms in 800 810 790 820 780 830 770 840; do
  rm -rf ss2/w && cp -a checkpoints/pre-confirm ss2/w
  timeout -s KILL "$(awk -v m=$ms 'BEGIN{printf "%.3f", m/1000}')" $CUR --root ss2/w confirm "CONFIRM $CODE" >/dev/null 2>&1
  if [ -d ss2/w/.engr/local/migration/destination ] && $OLD --root ss2/w ls --all >/dev/null 2>&1; then
    echo "landed at ${ms}ms: destination staged, and the released build still works here"
    found=$ms; break
  fi
done
[ -n "$found" ] || { echo "did not land in the window this time; the sweep is timing-dependent"; exit 1; }

echo "format.json: $(cat ss2/w/.engr/format.json | tr -d '\n ')"
echo
echo "===== a day's work, admitted through the released tool's own Human Gate ====="
out=$($OLD --root ss2/w prepare --object 01a05e55-74 --add --no-based-on --text "Admitted with the released tool after the migration crashed, and before anyone resumed it." 2>&1)
c=$(printf '%s' "$out" | sed -n 's/.*CONFIRM \([A-Z0-9]*\).*/\1/p')
printf '%s\n' "$out" | tail -1
$OLD --root ss2/w confirm "CONFIRM $c" 2>&1 | head -2; echo "exit=$?"
echo "-- and it is in the predecessor record --"
$OLD --root ss2/w show 01a05e55-74 2>&1 | tail -4
$OLD --root ss2/w verify 2>&1 | head -3
cp -a ss2/w ss2/before

echo
echo "===== later, somebody resumes the migration ====="
$CUR --root ss2/w confirm "CONFIRM $CODE" 2>&1 | head -3; echo "exit=$?"

echo
echo "===== is the admitted work in the migrated record? ====="
if $CUR --root ss2/w show 01a05e55-74 2>&1 | grep -q "Admitted with the released tool"; then
  echo "PRESENT: the resume carried it forward"
else
  echo "GONE: the resume published a stage that predates it"
fi
$CUR --root ss2/w ls --all
$CUR --root ss2/w verify; echo "verify exit=$?"
echo
echo "-- what the predecessor held just before the resume --"
grep -c "Admitted with the released tool" ss2/before/.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
echo "-- and whether any of it is recoverable afterwards --"
ls ss2/w/.engr/events 2>/dev/null || echo "(predecessor events discarded)"
grep -rc "Admitted with the released tool" ss2/w/.engr 2>/dev/null | grep -v ':0' || echo "(the wording appears in no file under .engr)"
