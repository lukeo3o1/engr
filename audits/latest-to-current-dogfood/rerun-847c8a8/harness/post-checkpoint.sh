#!/bin/sh
# Checkpoint the migrated workspace, and compare this run's migrated inventory
# against the two previous runs. Only the three Event streams may differ: each
# carries a fresh Event id and admission instant by construction.
set -u
cd /audit/r8/project
sh harness/checkpoint.sh /audit/r8/project /audit/r8/checkpoints/migrated
ls /audit/r8/checkpoints
for prev in r5 r6; do
  echo "--- against $prev ---"
  diff /audit/$prev/evidence/inventory-post-migration.txt \
       /audit/r8/evidence/inventory-post-migration.txt
  echo "diff exit=$?"
done
