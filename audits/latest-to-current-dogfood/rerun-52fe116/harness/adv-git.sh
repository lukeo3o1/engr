#!/bin/sh
set -u
cd /audit/r2
E=/audit/bin/engr-current
export HOME=/audit/home GIT_CONFIG_GLOBAL=/audit/home/.gitconfig
rm -rf advg && mkdir advg

echo "===== linked worktree: does the local exclude land where git reads it? ====="
cp -a crash-runs/baseline advg/main
cd advg/main
git worktree add -q ../linked -b audit-worktree 2>&1 | head -2
cd /audit/r2
echo "linked worktree .git is a file: $(head -c 60 advg/linked/.git)"
echo "-- preparing a migration inside the linked worktree --"
$E --root advg/linked migrate 2>&1 | sed -n 's/.*\(CONFIRM [A-Z0-9]*\).*/prepared: \1/p'
echo "-- where did the exclusion go? --"
echo "per-worktree admin dir  : $(ls advg/main/.git/worktrees/linked/info/exclude 2>/dev/null || echo 'absent (correct)')"
echo "common dir info/exclude : $(grep -c 'engr/local' advg/main/.git/info/exclude 2>/dev/null || echo 0) matching lines"
echo "-- is the live challenge visible to git from inside the worktree? --"
( cd advg/linked && git status --porcelain --untracked-files=all | grep -i 'local' || echo "NOT VISIBLE TO GIT (correct)" )
( cd advg/linked && git status --porcelain | head -3 )
echo "-- and it still confirms from there --"
code=$(ls advg/linked/.engr/local/challenges | sed 's/\.json//')
$E --root advg/linked confirm "CONFIRM $code" 2>&1 | head -2
( cd advg/linked && git status --porcelain --untracked-files=all | grep -i 'local' || echo "still NOT VISIBLE after publication" )

echo
echo "===== dirty git basis ====="
cd /audit/r2/advg/main
echo "a local edit nobody committed" >> README.md
$E --root . prepare --object 01a05e55-74 --add --text "wording written against an uncommitted tree" 2>&1 | head -3
echo "exit=$?"
echo "-- and with an explicit basis it is accepted --"
$E --root . prepare --object 01a05e55-74 --add --no-based-on --text "wording that says it stands on nothing" 2>&1 | sed -n '1,4p'
