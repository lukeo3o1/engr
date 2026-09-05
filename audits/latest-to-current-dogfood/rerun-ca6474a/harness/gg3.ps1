# r3 two-step governed mutation.
#
# EngrArgs carries the only explicit Position, which makes every other parameter
# name-only. Without that, PowerShell binds the first unnamed argument to
# whichever declared parameter is next in order — so `prepare` silently became
# the -Explanation value and `--classify` became -Attempt.
param(
  [Parameter(Mandatory = $true)][string]$Log,
  [Parameter(Mandatory = $true)][string]$WorkDir,
  [Parameter(Mandatory = $true)][string]$Result,
  [string]$Explanation,
  [int]$Attempt = 1,
  [string[]]$Rules = @('audit-scope', 'evidence-discipline'),
  [Parameter(Position = 0, ValueFromRemainingArguments = $true)][string[]]$EngrArgs
)
$first = & "D:\lukeo3o1\engr-audit\t3.ps1" -Log $Log -Which current -WorkDir $WorkDir @EngrArgs | Out-String
if ($first -notmatch 'digest (1:[0-9a-f]{64})') {
  Write-Output "GATE2: no review demanded; the first attempt stands"
  Write-Output $first.TrimEnd()
  return
}
$digest = $Matches[1]
$second = @($EngrArgs) + @('--review', $digest)
foreach ($r in $Rules) { $second += @('--reviewed-rule', $r) }
$second += @('--review-attempt', "$Attempt", '--review-result', $Result)
if ($Explanation) { $second += @('--review-explanation', $Explanation) }
& "D:\lukeo3o1\engr-audit\g3.ps1" -Log $Log -Which current -WorkDir $WorkDir @second
