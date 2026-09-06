# Backlog: the staging domain, and the two-level `expect` token that guards it.
# Every step re-reads its own token rather than reusing what the last step saw.
$ErrorActionPreference = 'Continue'
$T = "D:\lukeo3o1\engr-audit\t6.ps1"
$W = "r6/project"

& $T -Log 18-backlog -Which current -WorkDir $W backlog new --help
& $T -Log 18-backlog -Which current -WorkDir $W backlog new --title "Open questions this run raised" --text "Whether ls --verify should report an object whose projection is missing, as verify does."
$listing = & $T -Log 18-backlog -Which current -WorkDir $W backlog ls | Out-String
Write-Output $listing.TrimEnd()
$b = ($listing -split "`n" | Where-Object { $_ -match 'Open questions this run raised' }) -replace '^\s*(\S+).*', '$1'
$b = $b.Trim()
Write-Output "BACKLOG: $b"
$b | Set-Content -Encoding utf8 "D:\lukeo3o1\engr-audit\r6\evidence\backlog-id.txt"

$json = & $T -Log 18-backlog -Which current -WorkDir $W backlog show $b --format json | Out-String
Write-Output $json.TrimEnd()
if ($json -notmatch '"add":\s*"([0-9a-f]{64})"') { Write-Output "NO TOPIC-LEVEL ADD TOKEN"; return }
$addToken = $Matches[1]
Write-Output "topic-level add token: $addToken"

& $T -Log 18-backlog -Which current -WorkDir $W backlog add $b --text "Whether repair should say what verify says about the same object." --expect $addToken --subject engr:obj:01m1f5ax7bedrbew8012yq3ma9
Write-Output "===== the same token again: it was spent when the topic moved ====="
& $T -Log 18-backlog -Which current -WorkDir $W backlog add $b --text "stale token attempt" --expect $addToken
