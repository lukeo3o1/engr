# r2 variant of gate.ps1: prepare, read the code off the rendered screen, confirm.
param(
  [Parameter(Mandatory = $true)][string]$Log,
  [Parameter(Mandatory = $true)][string]$Which,
  [Parameter(Mandatory = $true)][string]$WorkDir,
  [Parameter(ValueFromRemainingArguments = $true)][string[]]$EngrArgs
)
$screen = & "D:\lukeo3o1\engr-audit\t2.ps1" -Log $Log -Which $Which -WorkDir $WorkDir @EngrArgs | Out-String
Write-Output $screen.TrimEnd()
if ($screen -notmatch 'CONFIRM\s+([23456789ABCDEFGHJKLMNPQRSTUVWXYZ]{6})') {
  Write-Output "GATE: no challenge code on the screen; nothing confirmed"
  return
}
$code = $Matches[1]
& "D:\lukeo3o1\engr-audit\t2.ps1" -Log $Log -Which $Which -WorkDir $WorkDir -- confirm "CONFIRM $code"
