#!/bin/sh
# F-4's exact instant, with the whole screen kept rather than its first line:
# publication has begun (one or more Event streams published), every predecessor
# Object file is still its own bytes, and one of them is edited out of band.
# d3be38a's answer was to publish and *report* each source file written over.
set -u
cd /audit/r5
CUR=/audit/bin/engr-current
CODE=DBMUMJ
AU=.engr/objects/01a05e55-74eb-7370-b771-0008bd71d149.json
rm -rf bsf && mkdir bsf
for ms in 960 980 1000; do
  rm -rf bsf/w && cp -a checkpoints/pre-confirm bsf/w
  timeout -s KILL "$(awk -v m=$ms 'BEGIN{printf "%.3f", m/1000}')" $CUR --root bsf/w confirm "CONFIRM $CODE" >/dev/null 2>&1
  streams=$(ls bsf/w/.engr/eventstore/objects 2>/dev/null | wc -l)
  [ -d bsf/w/.engr/local/migration/destination ] || { echo "kill@$ms not staged; skipped"; continue; }
  echo "================ kill@${ms}ms   published_streams=$streams/3"
  before=$(sha256sum "bsf/w/$AU" | cut -d' ' -f1)
  sed -i 's/Migration continuity audit/Migration continuity audit (edited out of band)/' "bsf/w/$AU"
  after=$(sha256sum "bsf/w/$AU" | cut -d' ' -f1)
  [ "$before" = "$after" ] && { echo "NO-OP! the edit changed nothing"; continue; }
  echo "-- the whole screen the resume prints --"
  $CUR --root bsf/w confirm "CONFIRM $CODE"; echo "exit=$?"
  echo "-- did the edit survive? --"
  grep -c 'edited out of band' "bsf/w/$AU" 2>/dev/null || echo 0
  echo "-- and does the published record verify? --"
  $CUR --root bsf/w verify; echo "exit=$?"
done
