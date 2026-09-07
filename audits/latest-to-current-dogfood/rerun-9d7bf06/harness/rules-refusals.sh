#!/bin/sh
# The review refusals, each on a mutation the subject could really have taken --
# a refusal that fires for an unrelated reason hides whatever it was written to
# find. Every probe below is a governed `--add`, which is always available.
set -u
cd /audit/r7/project
E=/audit/bin/engr-current
TXT="A section whose only purpose is to be refused, named in transcript rules-refusals."

echo "--- what the real digest for this mutation is ---"
real=$($E prepare --object 01a05e55-ee --add --no-based-on --text "$TXT" 2>&1 \
       | sed -n 's/.*digest \(1:[0-9a-f]*\).*/\1/p')
echo "  $real"

echo
echo "--- a digest that is not this mutation's ---"
$E prepare --object 01a05e55-ee --add --no-based-on --text "$TXT" \
  --review "1:0000000000000000000000000000000000000000000000000000000000000000" \
  --reviewed-rule audit-scope --reviewed-rule evidence-discipline \
  --review-attempt 1 --review-result passed 2>&1 | head -2
echo "  exit=$?"

echo
echo "--- the right digest, but only one of the two governing Rules named ---"
$E prepare --object 01a05e55-ee --add --no-based-on --text "$TXT" \
  --review "$real" --reviewed-rule audit-scope \
  --review-attempt 1 --review-result passed 2>&1 | head -2
echo "  exit=$?"

echo
echo "--- failed, with no explanation ---"
$E prepare --object 01a05e55-ee --add --no-based-on --text "$TXT" \
  --review "$real" --reviewed-rule audit-scope --reviewed-rule evidence-discipline \
  --review-attempt 2 --review-result failed 2>&1 | head -2
echo "  exit=$?"

echo
echo "--- attempt 0, which no review can be ---"
$E prepare --object 01a05e55-ee --add --no-based-on --text "$TXT" \
  --review "$real" --reviewed-rule audit-scope --reviewed-rule evidence-discipline \
  --review-attempt 0 --review-result passed 2>&1 | head -2
echo "  exit=$?"

echo
echo "--- and the review provenance that reached the record ---"
grep -o '"review":{[^}]*}' .engr/eventstore/objects/01a05e55-ee46-7a91-983f-fa79b297b10c.jsonl
