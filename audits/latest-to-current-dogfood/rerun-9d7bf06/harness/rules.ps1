# Rule Review, all three outcomes: a passing review, a failed one overridden by
# the Human Gate, and an exhausted one under on_exhaustion = human_confirmation.
$ErrorActionPreference = 'Continue'
$T  = "D:\lukeo3o1\engr-audit\t7.ps1"
$GG = "D:\lukeo3o1\engr-audit\gg7.ps1"
$W  = "r7/project"
$O  = "01a05e55-ee"

Write-Output "===== the Rules this workspace now carries ====="
& $T -Log 16-rules -Which current -WorkDir $W rules ls

Write-Output "`n===== a governed mutation with no review is refused ====="
& $T -Log 16-rules -Which current -WorkDir $W prepare --object $O --reopen

Write-Output "`n===== the same mutation, review passed ====="
& $GG -Log 16-rules -WorkDir $W -Result passed -Attempt 1 prepare --object $O --reopen

Write-Output "`n===== wording the Rules reject: failed, then overridden ====="
& $GG -Log 16-rules -WorkDir $W -Result failed -Attempt 2 `
  -Explanation "The wording names no transcript, which evidence-discipline requires." `
  prepare --object $O --add --no-based-on --text "The migration completed and everything looked fine."

Write-Output "`n===== attempts past max_attempts: exhausted ====="
& $GG -Log 16-rules -WorkDir $W -Result exhausted -Attempt 4 `
  -Explanation "Four attempts, and the wording still asserts what was not observed." `
  prepare --object $O --add --no-based-on --text "The migration completed and everything looked fine."

Write-Output "`n===== an attempt number the Rule does not allow ====="
& $T -Log 16-rules -Which current -WorkDir $W prepare --object $O --reopen --review "1:0000000000000000000000000000000000000000000000000000000000000000" --reviewed-rule audit-scope --reviewed-rule evidence-discipline --review-attempt 1 --review-result passed
