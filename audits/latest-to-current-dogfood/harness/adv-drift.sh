#!/bin/sh
set -u
cd /audit
E=/audit/bin/engr-current
export HOME=/audit/home GIT_CONFIG_GLOBAL=/audit/home/.gitconfig
cd advf
AU=01a05e55-74eb-7370-b771-0008bd71d149
EV=01a05e55-e62b-75c3-8b32-6c388dec4c4b
echo "===== what does the dependent Section's Ref actually point at now? ====="
sed -n 's/.*"refs":\(\[[^]]*\]\).*/\1/p' .engr/objects/$EV.json
echo
echo "===== current target §1 text ====="
sed -n 's/.*"id":1,"text":"\([^"]*\)".*/\1/p' .engr/objects/$AU.json | head -c 120
echo
echo "===== show the dependent object =====" 
$E --root . show $EV 2>&1 | tail -8
echo
echo "===== verify with every surface it offers ====="
$E --root . verify --help 2>&1 | head -20
