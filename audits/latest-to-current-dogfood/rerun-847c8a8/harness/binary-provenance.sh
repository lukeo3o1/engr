#!/bin/sh
# Prove each binary is the head it is supposed to be.
#
# Every archived tree reports `engr latest (unknown)`, so the version string can
# never tell two heads apart. Ask each binary a question only one head answers
# differently, picked from the diff between the trees *before* the build:
# PROTOCOL.md is compiled into the binary and most rounds add a paragraph.
#
# Run 8's intended set:
#
#   engr-latest    e7d9f99   the released predecessor
#   engr-prev      a2568f2   round 34, the previously audited head
#   engr-current   847c8a8   round 35, the head under test
#   engr-r33       9d7bf06   round 33, kept so round 34's findings keep a
#                            failing control
#
# The round-35 paragraph is what separates current from prev. Nothing in the
# protocol text separates prev (a2568f2) from r33 (9d7bf06) -- round 34 changed
# no wording -- so those two are separated behaviourally at the end.
set -u
for b in latest prev current r33; do
  [ -f "/audit/bin/engr-$b" ] || continue
  echo "--- engr-$b ---"
  /audit/bin/engr-$b --version 2>&1 | head -1
  printf 'round 35 (847c8a8) sentence: '
  /audit/bin/engr-$b protocol 2>/dev/null | grep -c 'There are two crash windows'
  printf 'round 33 (9d7bf06) sentence: '
  /audit/bin/engr-$b protocol 2>/dev/null | grep -c 'a repair that offers a confirmation code'
  printf 'round 32 (259b8b4) sentence: '
  /audit/bin/engr-$b protocol 2>/dev/null | grep -c 'Every surface that classifies an Object owes the same answer'
  printf 'round 31 (9fba620) sentence: '
  /audit/bin/engr-$b protocol 2>/dev/null | grep -c 'What navigation gives up'
  printf 'sha256: '
  sha256sum /audit/bin/engr-$b | cut -d' ' -f1
done

echo
echo "--- prev (a2568f2) against r33 (9d7bf06), which the protocol text cannot separate ---"
echo "round 34 made the repair screen name a revision on the divergent projection too."
cd /audit/r8
for b in prev r33 current; do
  [ -f "/audit/bin/engr-$b" ] || continue
  [ -d st-divergent ] || { echo "  (st-divergent not prepared yet; skipped)"; break; }
  rm -rf bp && cp -a st-divergent bp
  n=$(/audit/bin/engr-$b --root bp repair 01a05e55-74 2>&1 | grep -c 'at rev 1, admitted as rev 2')
  printf '  engr-%-8s divergent repair screen names the revision: %s\n' "$b" "$n"
done
rm -rf bp
