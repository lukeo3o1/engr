#!/bin/sh
# Where the carried-forward workspace ends up, and what git can see of it.
set -u
cd /audit/r4/project
E=/audit/bin/engr-current
export HOME=/audit/home GIT_CONFIG_GLOBAL=/audit/home/.gitconfig
echo "===== verify ====="
$E verify; echo "exit=$?"
echo
echo "===== ls --all ====="
$E ls --all; echo "exit=$?"
echo
echo "===== every stream, and its revisions ====="
for f in .engr/eventstore/objects/*.jsonl; do
  printf '%s  %s events  revs %s\n' "$(basename "$f" .jsonl)" "$(wc -l < "$f")" "$(sed -n 's/.*"rev":\([0-9]*\).*/\1/p' "$f" | tr '\n' ' ')"
done
echo
echo "===== what git tracks, and what it must never see ====="
git add -A
git status --porcelain | head -20
echo "-- anything under local/? --"
git status --porcelain --untracked-files=all | grep -i 'engr/local' || echo "NOT VISIBLE TO GIT (correct)"
git ls-files | grep -i 'engr/local' || echo "and nothing under local/ is tracked (correct)"
git reset -q
echo
echo "===== inventory ====="
sh harness/inventory.sh .
