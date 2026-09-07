#!/bin/sh
# The migrated record, hashed, and compared with the fourth run's.
# Streams carry a fresh Event id and admission instant per run by construction;
# everything else must be identical bytes or the code moved them.
set -u
cd /audit/r8/project
export HOME=/audit/home GIT_CONFIG_GLOBAL=/audit/home/.gitconfig
sh harness/inventory.sh . > /audit/r8/evidence/inventory-post-migration.txt
echo "--- migrated inventory (this run) ---"
cat /audit/r8/evidence/inventory-post-migration.txt
echo "--- against rerun-0086bb6 (r5), the established baseline ---"
diff -u /audit/r5/evidence/inventory-post-migration.txt \
        /audit/r8/evidence/inventory-post-migration.txt \
  && echo "IDENTICAL to r5, streams included"
echo "--- against r7 (9d7bf06), the previous run ---"
diff -u /audit/r7/evidence/inventory-post-migration.txt \
        /audit/r8/evidence/inventory-post-migration.txt \
  && echo "IDENTICAL to r7, streams included"
echo "--- commit the migrated workspace ---"
git add -A
git commit -q -m "migrated to generation 1" && git log --oneline -1
git status --porcelain | head
echo "--- checkpoint it ---"
rm -rf /audit/r8/checkpoints/migrated
sh harness/checkpoint.sh /audit/r8/project /audit/r8/checkpoints/migrated
ls /audit/r8/checkpoints
