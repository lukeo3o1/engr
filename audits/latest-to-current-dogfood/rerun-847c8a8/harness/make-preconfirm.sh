#!/bin/sh
# Mint this run's pre-confirm checkpoint: the predecessor fixture with a pending
# migration Challenge, prepared by the head under test.
#
# It is minted per run rather than copied forward, because a Challenge carries
# the generator fingerprint of the binary that made it -- carrying r6's forward
# would make every crash and barrier probe below run against a Challenge this
# head might refuse as incompatible, and they would fail for that reason instead
# of measuring what they are about.
set -u
cd /audit/r8
C=/audit/bin/engr-current
rm -rf pc && cp -a /audit/checkpoints/pre-migration pc
# keep .git, README.md and harness: the git-visibility and exclusion probes need them
out=$($C --root pc migrate 2>&1)
printf '%s\n' "$out" | tail -3
code=$(printf '%s' "$out" | sed -n 's/.*CONFIRM \([A-Z0-9]*\).*/\1/p' | head -1)
[ -n "$code" ] || { echo "NO CODE -- stop"; exit 1; }
echo "$code" > /audit/r8/evidence/preconfirm-code.txt
rm -rf checkpoints/pre-confirm
cp -a pc checkpoints/pre-confirm
echo "pre-confirm checkpoint minted with code $code"
ls checkpoints/pre-confirm/.engr
ls checkpoints/pre-confirm/.engr/local/challenges
