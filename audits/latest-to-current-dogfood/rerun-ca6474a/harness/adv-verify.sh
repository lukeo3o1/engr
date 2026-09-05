#!/bin/sh
set -u
cd /audit/r3/advf
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
