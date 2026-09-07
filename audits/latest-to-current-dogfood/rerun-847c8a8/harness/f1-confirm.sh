#!/bin/sh
# F-1, stated exactly: the whole damaged repair screen, unabridged, and every
# line on it that names a revision.
set -u
cd /audit/r8
C=/audit/bin/engr-current
for pair in "st-missing absent" "st-divergent divergent" "st-tampered tampered"; do
  set -- $pair
  ws=$1; what=$2
  rm -rf f1 && cp -a "$ws" f1
  echo "################ $what, whole screen ################"
  $C --root f1 repair 01a05e55-74 2>&1
  echo "---- lines naming a revision ----"
  $C --root f1 repair 01a05e55-74 2>&1 | grep -niE 'rev[ .]|revision' || echo "  (none)"
  echo
done
