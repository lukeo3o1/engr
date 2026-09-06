#!/bin/sh
# The migrated record, hashed, and compared with the fourth run's.
# Streams carry a fresh Event id and admission instant per run by construction;
# everything else must be identical bytes or the code moved them.
set -u
cd /audit/r5/project
export HOME=/audit/home GIT_CONFIG_GLOBAL=/audit/home/.gitconfig
sh harness/inventory.sh . > /audit/r5/evidence/inventory-post-migration.txt
echo "--- migrated inventory (this run) ---"
cat /audit/r5/evidence/inventory-post-migration.txt
echo "--- against rerun-b02d05e (r4) ---"
diff -u /audit/r4/evidence/inventory-post-migration.txt \
        /audit/r5/evidence/inventory-post-migration.txt \
  && echo "IDENTICAL, streams included"
echo "--- commit the migrated workspace ---"
git add -A
git commit -q -m "migrated to generation 1" && git log --oneline -1
git status --porcelain | head
echo "--- checkpoint it ---"
rm -rf /audit/r5/checkpoints/migrated
sh harness/checkpoint.sh /audit/r5/project /audit/r5/checkpoints/migrated
ls /audit/r5/checkpoints
