#!/bin/sh
# Round 25's finding 3: the live-Challenge exclusion wrote a *path* where git
# expects a *pattern*, so a workspace under a directory whose name contains
# glob metacharacters was not excluded at all — and the exclusion is the whole
# of what keeps a live challenge code out of `git add -A` before the tracked
# .gitignore can carry it.
#
# The workspace has to be nested rather than moved: a relocated .engr cannot
# resolve historical Ref targets at the paths its own commits recorded, so the
# referencing object is dropped first.
set -u
cd /audit/r4
E=/audit/bin/engr-current
export HOME=/audit/home GIT_CONFIG_GLOBAL=/audit/home/.gitconfig
EV=01a05e55-e62b-75c3-8b32-6c388dec4c4b
rm -rf nested && mkdir nested
cp -a /audit/checkpoints/pre-migration nested/outer
cd nested/outer
rm -rf .engr/local
mkdir -p 'project[1]'
mv .engr 'project[1]/.engr'
rm -f "project[1]/.engr/objects/$EV.json" "project[1]/.engr/events/$EV.jsonl"
git add -A >/dev/null 2>&1
git commit -q -m "the workspace, nested under a name with metacharacters" >/dev/null 2>&1
echo "layout: $(ls -d 'project[1]/.engr')"
echo "clean: $(git status --porcelain | wc -l) changes"

echo
echo "===== prepare a migration inside project[1] ====="
out=$($E --root 'project[1]' migrate 2>&1)
code=$(printf '%s' "$out" | sed -n 's/.*CONFIRM \([A-Z0-9]*\).*/\1/p' | head -1)
echo "prepared: $code"
echo "-- what engr wrote into .git/info/exclude --"
tail -2 .git/info/exclude
echo "-- git's own answer about the live challenge file --"
git check-ignore -v "project[1]/.engr/local/challenges/$code.json"; echo "check-ignore exit=$?"
echo "-- and whether git would pick it up --"
git status --porcelain --untracked-files=all | grep -i 'local' || echo "NOT VISIBLE TO GIT (correct)"

echo
echo "===== the control: the same check with the old, unescaped pattern ====="
grep -v 'project' .git/info/exclude > /tmp/ex && mv /tmp/ex .git/info/exclude
printf '/project[1]/.engr/local/\n' >> .git/info/exclude
tail -1 .git/info/exclude
git check-ignore -v "project[1]/.engr/local/challenges/$code.json"; echo "check-ignore exit=$?"
git status --porcelain --untracked-files=all | grep -i 'local' || echo "NOT VISIBLE TO GIT"
