#!/bin/sh
# The input is the input: the restored predecessor, hashed, against the
# inventory the committed audit recorded. Every later difference is the code's.
set -u
cd /audit/r5/project
sh harness/inventory.sh . > /audit/r5/evidence/inventory-pre-migration.txt
echo "--- restored predecessor inventory ---"
cat /audit/r5/evidence/inventory-pre-migration.txt
echo "--- against the committed audit's own record ---"
if diff -u /audit/r5/evidence/inventory-pre-migration-committed.txt \
         /audit/r5/evidence/inventory-pre-migration.txt; then
  echo "IDENTICAL: the predecessor input is byte-for-byte the one the committed audit used"
else
  echo "DIFFERENT: the fixture is not the committed one -- stop and find out why"
fi
echo "--- git of the fixture ---"
git log --oneline -3
git status --porcelain | head
