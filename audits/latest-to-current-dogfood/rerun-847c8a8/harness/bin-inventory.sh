#!/bin/sh
# Which head each binary in bin/ actually is. PROTOCOL.md is compiled in and
# every round since 31 added a paragraph, so the version string ("engr latest
# (unknown)" on every archived tree) never has to be trusted.
#
# For run 8 the pair that matters is engr-prev (a2568f2) against engr-current
# (847c8a8); they are separated by the round-35 paragraph alone, because round
# 34 changed no protocol wording. That also means a2568f2 and 9d7bf06 are NOT
# separable here -- tell those two apart behaviourally instead: a repair screen
# on a divergent projection names a revision at a2568f2 and names none at
# 9d7bf06.
set -u
for b in latest prev current r33 r34 fixed; do
  [ -f "/audit/bin/engr-$b" ] || continue
  printf 'engr-%-9s ' "$b"
  printf 'r31=%s r32=%s r33=%s r35=%s  ' \
    "$(/audit/bin/engr-$b protocol 2>/dev/null | grep -c 'What navigation gives up')" \
    "$(/audit/bin/engr-$b protocol 2>/dev/null | grep -c 'Every surface that classifies an Object owes')" \
    "$(/audit/bin/engr-$b protocol 2>/dev/null | grep -c 'a repair that offers a confirmation code')" \
    "$(/audit/bin/engr-$b protocol 2>/dev/null | grep -c 'There are two crash windows')"
  sha256sum "/audit/bin/engr-$b" | cut -c1-16
done
