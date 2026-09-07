#!/bin/sh
# The P3 the sixth run found, and its control: a projection that is absent and
# one that is merely a revision behind, on one workspace at one instant.
set -u
cd /audit/r8
E=${1:-/audit/bin/engr-current}
rm -rf fx && cp -a as fx
echo "===== show, on the Object whose projection is absent ====="
$E --root fx show 01a05e55-74 | head -4
$E --root fx show 01a05e55-74 >/dev/null 2>&1; echo "  exit=$?"
$E --root fx show 01a05e55-74 2>&1 >/dev/null | head -2
echo "===== its JSON integrity ====="
$E --root fx show 01a05e55-74 --format json 2>/dev/null | grep '"integrity"'
echo "===== repair, which the screen now names ====="
__out=$($E --root fx repair 01a05e55-74); __rc=$?
printf '%s\n' "$__out" | head -10; echo "  exit=$__rc"
echo "===== the control: a projection one revision behind is still sound ====="
$E --root fx show 01a05e55-ee >/dev/null 2>&1; echo "  show -ee exit=$?"
$E --root fx show 01a05e55-ee --format json 2>/dev/null | grep '"integrity"'
