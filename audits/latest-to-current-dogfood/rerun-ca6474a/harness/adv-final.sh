#!/bin/sh
set -u
cd /audit/r3
E=/audit/bin/engr-current
export HOME=/audit/home GIT_CONFIG_GLOBAL=/audit/home/.gitconfig
rm -rf advf && cp -a crash-runs/baseline advf
AU=01a05e55-74eb-7370-b771-0008bd71d149
EV=01a05e55-e62b-75c3-8b32-6c388dec4c4b
cd advf
git add -A >/dev/null 2>&1; git commit -q -m "migrated" >/dev/null 2>&1

echo "===== dirty git basis ====="
echo "an uncommitted edit" >> README.md
out=$($E --root . prepare --object $AU --add --text "wording written against an uncommitted tree" 2>&1); echo "exit=$?"
printf '%s\n' "$out" | head -2
echo "-- explicit --no-based-on is accepted instead --"
out=$($E --root . prepare --object $AU --add --no-based-on --text "wording that stands on nothing" 2>&1); echo "exit=$?"
printf '%s\n' "$out" | sed -n '1,4p'
git checkout -- README.md

echo
echo "===== stale / superseded Challenge on the current generation ====="
a=$($E --root . prepare --object $AU --add --no-based-on --text "first pending question" 2>&1 | sed -n 's/.*CONFIRM \([A-Z0-9]*\).*/\1/p')
echo "prepared A=$a"
b=$($E --root . prepare --object $AU --add --no-based-on --text "second pending question" 2>&1 | sed -n 's/.*CONFIRM \([A-Z0-9]*\).*/\1/p')
echo "prepared B=$b"
echo "-- answer B first --"
$E --root . confirm "CONFIRM $b" 2>&1 | head -1
echo "-- now answer the superseded A --"
out=$($E --root . confirm "CONFIRM $a" 2>&1); echo "exit=$? : $(printf '%s' "$out" | head -1)"
echo "-- and how does the screen describe A? --"
$E --root . candidate "$a" 2>&1 | tail -3

echo
echo "===== selective Ref drift: selected vs unselected fields ====="
echo "-- the migrated Ref selects based_on, refs, text; it does not select header --"
h=$($E --root . prepare --object $AU --revise 1 --no-based-on --header "a heading the reference never attested" --text "The question is not whether the migrator runs. It is whether an agent that kept its engineering memory here can still trust that memory afterwards: same identities, same admission provenance, same dependencies, and no fact that was quietly invented or quietly dropped along the way." 2>&1 | sed -n 's/.*CONFIRM \([A-Z0-9]*\).*/\1/p')
$E --root . confirm "CONFIRM $h" 2>&1 | head -1
echo "-- unselected field changed: is the dependent Section still ok? --"
$E --root . verify 2>&1 | grep -A1 "$(printf '01a05e55-e6')" | head -3
echo "-- now change a SELECTED field (text) on the same target --"
s=$($E --root . prepare --object $AU --revise 1 --no-based-on --header "a heading the reference never attested" --text "Rewritten wording, which the reference does attest." 2>&1 | sed -n 's/.*CONFIRM \([A-Z0-9]*\).*/\1/p')
$E --root . confirm "CONFIRM $s" 2>&1 | head -1
$E --root . verify 2>&1 | head -8
