#!/bin/sh
set -u
cd /audit/r3
E=/audit/bin/engr-current
BASE=crash-runs/baseline
rm -rf advy && cp -a "$BASE" advy

probe() {
  name=$1; front=$2
  printf -- '---\n%s---\n\n# Policy\n\nSay it.\n' "$front" > advy/.engr/rules/probe.md
  out=$($E --root advy rules ls 2>&1); code=$?
  printf '%-42s exit=%-3s %s\n' "$name" "$code" "$(printf '%s' "$out" | tr '\n' '|' | cut -c1-110)"
}

echo "===== the restricted YAML profile ====="
probe "plain block spelling (must LOAD)"   'id: probe
applies:
  domains:
    - object
'
probe "plain flow spelling (must LOAD)"    'id: probe
applies: { domains: [object] }
'
probe "anchor, block"                      'id: probe
applies:
  domains: &d
    - object
'
probe "anchor, flow"                       'id: probe
applies: { domains: [&d object] }
'
probe "anchor on explicit key, block"      '? &k id
: probe
applies:
  domains: [object]
'
probe "anchor on explicit key, flow"       '{ ? &k id : probe, applies: { domains: [object] } }
'
probe "alias, block"                       'id: probe
applies:
  domains: &d
    - object
spare: *d
'
probe "alias on explicit key, flow"        '{ ? *k : probe, applies: { domains: [object] } }
'
probe "custom tag"                         'id: probe
applies:
  domains: !!seq
    - object
'
probe "tag on explicit key, block"         '? !probe-key id
: probe
applies:
  domains: [object]
'
probe "duplicate key, block"               'id: probe
id: other
applies:
  domains: [object]
'
probe "duplicate key, flow"                'id: probe
applies: { domains: [object], domains: [backlog] }
'
probe "duplicate key spelled two ways"     'id: probe
"id": other
applies:
  domains: [object]
'
probe "block scalar"                       'id: probe
applies:
  domains: [object]
spare: |
  id: smuggled
'
probe "second document"                    'id: probe
applies:
  domains: [object]
...
'
probe "explicit document start"            '---
id: probe
applies:
  domains: [object]
'
probe "YAML directive"                     '%YAML 1.2
---
id: probe
applies:
  domains: [object]
'
rm -f advy/.engr/rules/probe.md
