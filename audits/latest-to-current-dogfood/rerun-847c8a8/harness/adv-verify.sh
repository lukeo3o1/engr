#!/bin/sh
set -u
# advf is built by adv-final.sh. batch7 ran this script BEFORE that one, so it
# could only ever have measured a leftover workspace from an earlier run; this
# run it found nothing there and every surface answered 'no .engr workspace at .',
# which is a refusal about the harness, not about engr.
if [ ! -d /audit/r8/advf ]; then
  echo 'REFUSING: /audit/r8/advf does not exist -- run adv-final.sh first'
  exit 1
fi
cd /audit/r8/advf
E=/audit/bin/engr-current
export HOME=/audit/home GIT_CONFIG_GLOBAL=/audit/home/.gitconfig
EV=01a05e55-e62b-75c3-8b32-6c388dec4c4b
echo "===== verify, whole workspace ====="
$E --root . verify; echo "exit=$?"
echo
echo "===== verify, just the object whose ref moved ====="
$E --root . verify $EV; echo "exit=$?"
echo
echo "===== show, same object, same moment ====="
$E --root . show $EV | grep -E '§2|refs|advice'
echo
echo "===== ls ====="
$E --root . ls --all
