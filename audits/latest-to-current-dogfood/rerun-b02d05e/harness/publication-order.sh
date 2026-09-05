#!/bin/sh
# At the instant the skip is bought — one published Event stream — what has
# publication actually reached? If the predecessor Object files are still their
# own bytes, then the comparison the skip forgoes was still possible.
set -u
cd /audit/r4
CUR=/audit/bin/engr-current
CODE=FBR8Z2
rm -rf po && mkdir po
for ms in 855 860 865 870 880; do
  rm -rf po/w && cp -a checkpoints/pre-confirm po/w
  timeout -s KILL "$(awk -v m=$ms 'BEGIN{printf "%.3f", m/1000}')" $CUR --root po/w confirm "CONFIRM $CODE" >/dev/null 2>&1
  streams=$(ls po/w/.engr/eventstore/objects 2>/dev/null | wc -l)
  pred=0; dest=0
  for f in po/w/.engr/objects/*.json; do
    if grep -q '"digest"' "$f" 2>/dev/null; then dest=$((dest+1)); else pred=$((pred+1)); fi
  done
  barrier=$(grep -q 'migration-in-progress' po/w/.engr/format.json 2>/dev/null && echo up || echo down)
  printf 'kill@%-5sms barrier=%-5s published_streams=%s/3  object files still predecessor-shaped=%s destination-shaped=%s\n' \
    "$ms" "$barrier" "$streams" "$pred" "$dest"
done
