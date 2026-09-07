# Rule Review identity is artifact-exact: any byte of an applicable Rule file
# changes the ReviewDigest, and the review that was attested is then a review of
# something else. Restoring the byte restores the digest, which is the half that
# proves the refusal was about the artifact and not about anything incidental.
$ErrorActionPreference = 'Continue'
$T = "D:\lukeo3o1\engr-audit\t7.ps1"
$W = "r7/project"
$rule = "D:\lukeo3o1\engr-audit\r7\project\.engr\rules\audit-scope.md"
$text = "A claim that names evidence/crash-sweep.txt, as evidence-discipline requires."
$args0 = @('prepare','--object','01a05e55-ee','--add','--no-based-on','--text',$text)

$before = [System.IO.File]::ReadAllBytes($rule)
Write-Output ("rule sha256 before: " + (Get-FileHash $rule -Algorithm SHA256).Hash.ToLower())

Write-Output "===== first attempt: the review is demanded, and named ====="
$first = & $T -Log 17-rule-drift -Which current -WorkDir $W @args0 | Out-String
Write-Output $first.TrimEnd()
if ($first -notmatch 'digest (1:[0-9a-f]{64})') { Write-Output "NO DIGEST SURFACED"; return }
$digest = $Matches[1]

Write-Output "===== one byte of the Rule moves ====="
$txt = [System.IO.File]::ReadAllText($rule)
[System.IO.File]::WriteAllText($rule, $txt.Replace("is out of scope", "is  out of scope"))
$after = (Get-FileHash $rule -Algorithm SHA256).Hash.ToLower()
Write-Output "rule sha256 after : $after"
$second = @($args0) + @('--review',$digest,'--reviewed-rule','audit-scope','--reviewed-rule','evidence-discipline','--review-attempt','1','--review-result','passed')
& $T -Log 17-rule-drift -Which current -WorkDir $W @second

Write-Output "===== the byte goes back ====="
[System.IO.File]::WriteAllBytes($rule, $before)
Write-Output ("rule sha256 restored: " + (Get-FileHash $rule -Algorithm SHA256).Hash.ToLower())
& "D:\lukeo3o1\engr-audit\g7.ps1" -Log 17-rule-drift -Which current -WorkDir $W @second
