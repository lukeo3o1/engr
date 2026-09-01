#!/bin/sh
set -u
cd /audit
E=/audit/bin/engr-current
rm -rf crash-instant && mkdir crash-instant
cp -a checkpoints/crash-a crash-instant/run

echo "wall clock before the answer : $(date -u +%Y-%m-%dT%H:%M:%S.%NZ)"
timeout -s KILL 0.700 $E --root crash-instant/run confirm "CONFIRM 8DZM7N" >/dev/null 2>&1
echo "killed with exit $? ; destination staged: $([ -d crash-instant/run/.engr/local/migration/destination ] && echo yes || echo no)"
echo "wall clock after the kill    : $(date -u +%Y-%m-%dT%H:%M:%S.%NZ)"

echo "...waiting 6 seconds, as a crashed machine would before somebody retried..."
sleep 6
echo "wall clock at resume         : $(date -u +%Y-%m-%dT%H:%M:%S.%NZ)"
$E --root crash-instant/run confirm "CONFIRM 8DZM7N"
echo "resumed event admitted.at    : $(cat crash-instant/run/.engr/eventstore/objects/*.jsonl | sed -n 's/.*"admitted":{"at":"\([^"]*\)".*/\1/p' | sort -u)"

echo
echo "===== does the resumed result match an uninterrupted one, apart from the two facts that cannot match? ====="
cp -a checkpoints/crash-a crash-instant/clean
$E --root crash-instant/clean confirm "CONFIRM 8DZM7N" >/dev/null 2>&1
norm() { cat "$1"/.engr/eventstore/objects/*.jsonl | sed 's/"id":"[0-9a-f-]\{36\}"/"id":"<event-uuid>"/; s/"at":"[^"]*"/"at":"<instant>"/g; s/"digest":"1:[0-9a-f]*"/"digest":"<seal>"/' | LC_ALL=C sort; }
norm crash-instant/run   > /tmp/a
norm crash-instant/clean > /tmp/b
if diff -q /tmp/a /tmp/b >/dev/null; then echo "eventstore: IDENTICAL once Event id and admission instant are normalised"; else echo "eventstore: DIFFERS"; diff /tmp/a /tmp/b | head -5; fi
if diff -r -q crash-instant/run/.engr/objects crash-instant/clean/.engr/objects >/dev/null; then echo "objects/: BYTE-IDENTICAL"; else echo "objects/: DIFFERS"; diff -r crash-instant/run/.engr/objects crash-instant/clean/.engr/objects | head -5; fi
