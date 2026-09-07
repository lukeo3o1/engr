#!/bin/sh
# Round 33's ruling, on both repair screens.
#
# "A repair that offers a confirmation code MUST first show what it will write"
# and "It MUST also distinguish the revision admitted history derives from the
# revision confirming produces". Both repair screens are asked, whole and
# unabridged, plus the re-render a pending code gets, at both heads.
set -u
cd /audit/r8
C=/audit/bin/engr-current
P=/audit/bin/engr-prev

echo "############ 1. absent projection, at the head under test ############"
rm -rf rs1 && cp -a st-missing rs1
$C --root rs1 repair 01a05e55-74; echo "  exit=$?"

echo
echo "############ 2. the same, at the previously audited head ############"
rm -rf rs2 && cp -a st-missing rs2
$P --root rs2 repair 01a05e55-74 2>&1; echo "  exit=$?"

echo
echo "############ 3. the pending code shown again (current) ############"
rm -rf rs3 && cp -a st-missing rs3
out=$($C --root rs3 repair 01a05e55-74 2>&1)
code=$(printf '%s' "$out" | sed -n 's/.*CONFIRM \([A-Z0-9]*\).*/\1/p' | head -1)
echo "  the pending code is $code"
$C --root rs3 candidate "$code"; echo "  exit=$?"
echo "--- is the re-render the same screen? ---"
printf '%s\n' "$out" > /tmp/first.txt
$C --root rs3 candidate "$code" > /tmp/again.txt 2>&1
diff /tmp/first.txt /tmp/again.txt && echo "IDENTICAL" || echo "(differs -- above)"

echo
echo "############ 4. the damaged screen, for the same two MUSTs ############"
rm -rf rs4 && cp -a st-divergent rs4
$C --root rs4 repair 01a05e55-74; echo "  exit=$?"

echo
echo "############ 5. the tampered screen ############"
rm -rf rs5 && cp -a st-tampered rs5
$C --root rs5 repair 01a05e55-74; echo "  exit=$?"

echo
echo "############ 6. which screens name a revision at all ############"
for pair in "rs1 absent" "rs4 divergent" "rs5 tampered"; do
  set -- $pair
  ws=$1; what=$2
  n=$($C --root "$ws" repair 01a05e55-74 2>&1 | grep -ci 'rev ')
  printf '  %-10s lines naming a revision: %s\n' "$what" "$n"
done
