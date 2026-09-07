#!/bin/sh
# Prove each binary is the head it is supposed to be.
#
# Both archived trees report `engr latest (unknown)`, so the version string
# cannot tell 9fba620 from 9d7bf06. Ask each binary a question only one of the
# two heads answers differently, picked from the diff between them *before* the
# build: PROTOCOL.md is compiled into the binary, and each round added a
# paragraph.
set -u
for b in latest prev current; do
  echo "--- engr-$b ---"
  /audit/bin/engr-$b --version 2>&1 | head -1
  printf 'round 33 (9d7bf06) sentence: '
  /audit/bin/engr-$b protocol 2>/dev/null | grep -c 'a repair that offers a confirmation code'
  printf 'round 32 (259b8b4) sentence: '
  /audit/bin/engr-$b protocol 2>/dev/null | grep -c 'Every surface that classifies an Object owes the same answer'
  printf 'round 31 (9fba620) sentence: '
  /audit/bin/engr-$b protocol 2>/dev/null | grep -c 'What navigation gives up'
  printf 'sha256: '
  sha256sum /audit/bin/engr-$b | cut -d' ' -f1
done
