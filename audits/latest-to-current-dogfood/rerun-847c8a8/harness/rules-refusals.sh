#!/bin/sh
# The review refusals, each on a mutation the subject could really have taken --
# a refusal that fires for an unrelated reason hides whatever it was written to
# find. Every probe below is a governed `--add`, which is always available.
#
# Each probe captures engr's exit code before anything is piped. r7 read `$?`
# after `| head -2`, which is head's status and is always 0, so five refusals
# reported an exit code that was not engr's.
set -u
cd /audit/r8/project
E=/audit/bin/engr-current
TXT="A section whose only purpose is to be refused, named in transcript rules-refusals."

probe() { # $1 = label, then the flags after `--add --no-based-on --text "$TXT"`
  label=$1; shift
  echo
  echo "--- $label ---"
  out=$($E prepare --object 01a05e55-ee --add --no-based-on --text "$TXT" "$@" 2>&1); rc=$?
  printf '%s\n' "$out" | head -2
  echo "  exit=$rc"
}

echo "--- what the real digest for this mutation is ---"
real=$($E prepare --object 01a05e55-ee --add --no-based-on --text "$TXT" 2>&1 \
       | sed -n 's/.*digest \(1:[0-9a-f]*\).*/\1/p')
echo "  $real"
[ -n "$real" ] || { echo "REFUSING: no review was demanded, so there is no refusal to measure"; exit 1; }

probe "a digest that is not this mutation's" \
  --review "1:0000000000000000000000000000000000000000000000000000000000000000" \
  --reviewed-rule audit-scope --reviewed-rule evidence-discipline \
  --review-attempt 1 --review-result passed

probe "the right digest, but only one of the two governing Rules named" \
  --review "$real" --reviewed-rule audit-scope \
  --review-attempt 1 --review-result passed

probe "failed, with no explanation" \
  --review "$real" --reviewed-rule audit-scope --reviewed-rule evidence-discipline \
  --review-attempt 2 --review-result failed

probe "attempt 0, which no review can be" \
  --review "$real" --reviewed-rule audit-scope --reviewed-rule evidence-discipline \
  --review-attempt 0 --review-result passed

probe "an attempt past max_attempts, without declaring exhaustion" \
  --review "$real" --reviewed-rule audit-scope --reviewed-rule evidence-discipline \
  --review-attempt 4 --review-result passed

echo
echo "--- and the review provenance that reached the record ---"
grep -o '"review":{[^}]*}' .engr/eventstore/objects/01a05e55-ee46-7a91-983f-fa79b297b10c.jsonl
