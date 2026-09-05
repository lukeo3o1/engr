#!/bin/sh
# Two checks that a previous round put in and that nothing since has re-observed:
#
#  1. round 25's finding 5 — `verify` walked the *stored* projection for
#     dependencies, so a Ref admitted in a crash tail went unchecked. The probe
#     has to have no Ref in the stored bytes at all, or walking them would find
#     one by accident.
#  2. the predecessor history rules the released build itself accepted: a purged
#     *prefix* is legal and a *gap* is not.
set -u
cd /audit/r4
E=/audit/bin/engr-current
OLD=/audit/bin/engr-latest
export HOME=/audit/home GIT_CONFIG_GLOBAL=/audit/home/.gitconfig
AU=01a05e55-74eb-7370-b771-0008bd71d149
EV=01a05e55-e62b-75c3-8b32-6c388dec4c4b

echo "===== 1. a Ref that exists only in the durable tail ====="
rm -rf tp && cp -a crash-runs/baseline tp
cp "tp/.engr/objects/$EV.json" /tmp/rev1.json
out=$($E --root tp prepare --object $EV --add --no-based-on --text "Wording that depends on the audit object, admitted just before the crash." --ref 01a05e55-74:1 text 2>&1)
c=$(printf '%s' "$out" | sed -n 's/.*CONFIRM \([A-Z0-9]*\).*/\1/p' | head -1)
$E --root tp confirm "CONFIRM $c" | head -1
echo "-- now the crash: the projection goes back, the Event stays --"
cp /tmp/rev1.json "tp/.engr/objects/$EV.json"
echo "stored sections: $(grep -o '"id":[0-9]*' "tp/.engr/objects/$EV.json" | wc -l), stream events: $(wc -l < tp/.engr/eventstore/objects/$EV.jsonl)"
echo "-- and the target is made unreadable --"
sed -i 's/"state":"open"/"state":"not-a-state"/' "tp/.engr/objects/$AU.json"
echo "-- verify must still check the dependency the tail admitted --"
$E --root tp verify $EV 2>&1 | head -6; echo "exit=$?"

echo
echo "===== 2. a purged predecessor prefix is legal; a gap is not ====="
rm -rf tpp && cp -a /audit/checkpoints/pre-migration tpp
rm -rf tpg && cp -a /audit/checkpoints/pre-migration tpg
rm -f tpp/.engr/local/challenges/* tpg/.engr/local/challenges/* 2>/dev/null
f=.engr/events/01a05e55-ee46-7a91-983f-fa79b297b10c.jsonl
echo "the stream has $(wc -l < tpp/$f) events"
tail -n +3 "tpp/$f" > /tmp/p && mv /tmp/p "tpp/$f"
echo "prefix purged, $(wc -l < tpp/$f) left; migrating:"
$E --root tpp migrate 2>&1 | tail -2; echo "exit=$?"
sed -n '1,2p;4p' "tpg/$f" > /tmp/g && mv /tmp/g "tpg/$f"
echo "gap made, $(wc -l < tpg/$f) left; migrating:"
$E --root tpg migrate 2>&1 | head -2; echo "exit=$?"
