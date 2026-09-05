#!/bin/sh
# What the migration left behind, and when it says it happened.
set -u
cd /audit/r4/project
export HOME=/audit/home GIT_CONFIG_GLOBAL=/audit/home/.gitconfig
echo "--- predecessor files that must be gone ---"
for p in .engr/format.json .engr/lock .engr/candidates .engr/events; do
  if [ -e "$p" ]; then echo "$p  STILL PRESENT (wrong)"; else echo "$p  gone"; fi
done
echo "--- the migrated .engr/.gitignore ---"
cat .engr/.gitignore
echo "--- what a fresh init writes, for comparison ---"
rm -rf /audit/r4/scratch/fresh && mkdir -p /audit/r4/scratch/fresh && cd /audit/r4/scratch/fresh
git init -q . && git config core.autocrlf false && git config core.eol lf
/audit/bin/engr-current init >/dev/null 2>&1
cat .engr/.gitignore
cd /audit/r4/project
echo "--- diff (empty means identical) ---"
diff .engr/.gitignore /audit/r4/scratch/fresh/.engr/.gitignore && echo "IDENTICAL"
echo "--- every migrated Event's admitted.at ---"
cat .engr/eventstore/objects/*.jsonl | sed -n 's/.*"admitted":{"at":"\([^"]*\)".*/\1/p'
echo "pre-confirm  wallclock: $(cat /audit/r4/evidence/pre-confirm-wallclock.txt)"
echo "post-confirm wallclock: $(cat /audit/r4/evidence/post-confirm-wallclock.txt)"
echo "--- each Event's type and rev ---"
cat .engr/eventstore/objects/*.jsonl | sed -n 's/.*"rev":\([0-9]*\).*"type":"\([a-z._0-9]*\)".*/rev=\1 type=\2/p'
sed -n 's/.*"type":"\([a-z._0-9]*\)".*/type=\1/p' .engr/eventstore/objects/*.jsonl
echo "--- git status of the migrated workspace ---"
git status --porcelain | head -20
echo "--- and nothing under local/ is visible ---"
git status --porcelain --untracked-files=all | grep -i 'engr/local' || echo "NOT VISIBLE TO GIT (correct)"
