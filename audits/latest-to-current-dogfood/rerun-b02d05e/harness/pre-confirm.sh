#!/bin/sh
# What the workspace looks like after `migrate` and before `confirm`:
# nothing published, the live code hidden from git, the subject frozen.
set -u
CODE="${1:?usage: pre-confirm.sh <challenge-code>}"
cd /audit/r4/project
export HOME=/audit/home GIT_CONFIG_GLOBAL=/audit/home/.gitconfig
echo "--- git status --porcelain (must be empty) ---"
git status --porcelain
echo "--- .git/info/exclude ---"
cat .git/info/exclude
echo "--- git check-ignore -v on the live challenge ---"
git check-ignore -v ".engr/local/challenges/$CODE.json"
echo "check-ignore exit=$?"
echo "--- git status --untracked-files=all, anything under local/ ---"
git status --porcelain --untracked-files=all | grep -i local || echo "NOT VISIBLE TO GIT (correct)"
echo "--- the frozen subject ---"
cat ".engr/local/challenges/$CODE.json"
echo
echo "--- staged destination must not exist yet ---"
if [ -d .engr/local/migration/destination ]; then echo "PRESENT (wrong)"; else echo "absent (correct)"; fi
ls .engr/local/migration 2>/dev/null || echo "(no migration stage at all)"
