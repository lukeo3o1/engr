# Drive one mutation through the real Human Gate: prepare, read the code off the
# screen a person would read, then answer with the exact `CONFIRM <code>`.
#
#   .\gate.ps1 -Log 01-historical -Which latest -WorkDir project -- prepare --new --text "..."
#
# The code is taken from the rendered screen rather than from --json, because
# the screen is what the gate actually shows a human and reading it is part of
# what is being audited.
param(
  [Parameter(Mandatory = $true)][string]$Log,
  [Parameter(Mandatory = $true)][string]$Which,
  [Parameter(Mandatory = $true)][string]$WorkDir,
  [Parameter(ValueFromRemainingArguments = $true)][string[]]$EngrArgs
)
$screen = & "D:\lukeo3o1\engr-audit\t.ps1" -Log $Log -Which $Which -WorkDir $WorkDir @EngrArgs | Out-String
Write-Output $screen.TrimEnd()
if ($screen -notmatch 'CONFIRM\s+([23456789ABCDEFGHJKLMNPQRSTUVWXYZ]{6})') {
  Write-Output "GATE: no challenge code on the screen; nothing confirmed"
  return
}
$code = $Matches[1]
& "D:\lukeo3o1\engr-audit\t.ps1" -Log $Log -Which $Which -WorkDir $WorkDir -- confirm "CONFIRM $code"
