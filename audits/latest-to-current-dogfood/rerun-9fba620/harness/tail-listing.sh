#!/bin/sh
# A legitimate crash tail: the Event is durable, the projection is the one the
# previous revision produced. `show` reconciles it forward. The question this
# probe asks is what the *listing* says, because the listing is the surface an
# agent greps before it acts, and the wording it is grepping for was admitted.
set -u
cd /audit/r6
E=/audit/bin/engr-current
rm -rf tv && cp -a checkpoints/migrated tv
O=01a05e55-ee
F=tv/.engr/objects/01a05e55-ee46-7a91-983f-fa79b297b10c.json
cp "$F" tv/before.json
out=$($E --root tv prepare --object $O --add --no-based-on --text "Wording admitted immediately before the crash, which the projection never caught up to." 2>&1)
code=$(printf '%s' "$out" | sed -n 's/.*CONFIRM \([A-Z0-9]*\).*/\1/p' | head -1)
$E --root tv confirm "CONFIRM $code" | head -1
echo "--- the crash: the projection goes back, the Event stays ---"
before=$(sha256sum "$F" | cut -d' ' -f1)
cp tv/before.json "$F"
after=$(sha256sum "$F" | cut -d' ' -f1)
[ "$before" = "$after" ] && echo "NO-OP! the rewind changed nothing" || echo "projection rewound: $before -> $after"
echo "stored sections: $(grep -o '"id":[0-9]*' "$F" | wc -l), stream events: $(wc -l < tv/.engr/eventstore/objects/01a05e55-ee46-7a91-983f-fa79b297b10c.jsonl)"
rm tv/before.json
