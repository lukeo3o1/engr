#!/bin/sh
# What else moved. Confirmation now asks the question `prepare` asks, so any
# state where the two used to disagree changes -- and disagreeing is what the
# fix is about, so each one has to be looked at rather than assumed benign.
#
#   a) prepared while divergent, then the file becomes unreadable
#   b) prepared while divergent, then the file is removed
#   c) prepared while divergent, then history stops replaying
set -u
cd /audit/r8
F=/audit/bin/engr-current
C=/audit/bin/engr-r33
# A comparison probe whose two binaries are the same measures nothing and
# reports green. r8's `engr-fixed` and `engr-current` are byte-identical, and
# this probe used to name both. Hash either side and refuse.
if [ "$(sha256sum "$F" | cut -d' ' -f1)" = "$(sha256sum "$C" | cut -d' ' -f1)" ]; then
  echo "REFUSING: \$F and \$C are the same binary -- this probe would measure nothing"
  exit 1
fi
T=.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
code_of() { printf '%s' "$1" | sed -n 's/.*CONFIRM \([23456789ABCDEFGHJKLMNPQRSTUVWXYZ]\{6\}\).*/\1/p' | head -1; }

probe() { # $1 binary, $2 label, $3 what happens after the code is minted
  b=$1; who=$2; after=$3
  rm -rf fd && cp -a st-divergent fd
  out=$($b --root fd repair 01a05e55-74 2>&1)
  c=$(code_of "$out")
  case $after in
    unreadable) printf 'this is not an object\n' > "fd/$T" ;;
    removed)    rm "fd/$T" ;;
    unreplayable) cp tail/.engr/eventstore/objects/01a05e55-74eb-7370-b771-0008bd71d149.jsonl \
                     fd/.engr/eventstore/objects/01a05e55-74eb-7370-b771-0008bd71d149.jsonl ;;
  esac
  screen=$($b --root fd candidate "$c" 2>&1)
  offers=$(printf '%s' "$screen" | grep -c 'Type this exactly to confirm')
  $b --root fd confirm "CONFIRM $c" >/dev/null 2>&1; rc=$?
  present=no; [ -f "fd/$T" ] && present=yes
  printf '  %-12s %-8s screen offers code=%s  confirm exit=%-3s  projection present after=%s\n' \
    "$after" "$who" "$offers" "$rc" "$present"
}

for after in unreadable removed unreplayable; do
  probe "$F" fixed "$after"
  probe "$C" r33 "$after"
  rm -rf fd && cp -a st-divergent fd
  case $after in
    unreadable) printf 'this is not an object\n' > "fd/$T" ;;
    removed)    rm "fd/$T" ;;
    unreplayable) cp tail/.engr/eventstore/objects/01a05e55-74eb-7370-b771-0008bd71d149.jsonl \
                     fd/.engr/eventstore/objects/01a05e55-74eb-7370-b771-0008bd71d149.jsonl ;;
  esac
  printf '  %-12s %-8s ' "$after" "prepare"
  $F --root fd repair 01a05e55-74 2>&1 | head -1 | cut -c1-96
  echo
done
