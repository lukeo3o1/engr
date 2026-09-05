# Drive one *governed* mutation all the way through: let the first attempt
# surface the ReviewDigest, review the rules, then repeat the mutation with the
# review outcome and answer the challenge.
#
# This is the real two-step an agent has to perform whenever a project rule
# governs the domain, so the audit performs it rather than describing it.
param(
  [Parameter(Mandatory = $true)][string]$Log,
  [Parameter(Mandatory = $true)][string]$WorkDir,
  [Parameter(Mandatory = $true)][string]$Result,
  [string]$Explanation,
  [int]$Attempt = 1,
  [Parameter(ValueFromRemainingArguments = $true)][string[]]$EngrArgs
)
$first = & "D:\lukeo3o1\engr-audit\t.ps1" -Log $Log -Which current -WorkDir $WorkDir @EngrArgs | Out-String
if ($first -notmatch 'digest (1:[0-9a-f]{64})') {
  Write-Output "GATE2: no review demanded; the first attempt stands"
  Write-Output $first.TrimEnd()
  return
}
$digest = $Matches[1]
$second = @($EngrArgs) + @(
  '--review', $digest,
  '--reviewed-rule', 'audit-scope',
  '--reviewed-rule', 'evidence-discipline',
  '--review-attempt', "$Attempt",
  '--review-result', $Result
)
if ($Explanation) { $second += @('--review-explanation', $Explanation) }
& "D:\lukeo3o1\engr-audit\gate.ps1" -Log $Log -Which current -WorkDir $WorkDir @second
