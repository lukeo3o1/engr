# Run one r7 harness script inside the pinned container and tee its whole output
# to a named file under r7\. Docker is driven from PowerShell, never from Git
# Bash, which rewrites /audit/... into a Windows path.
param(
  [Parameter(Mandatory = $true)][string]$Script,
  [Parameter(Mandatory = $true)][string]$Out,
  [Parameter(ValueFromRemainingArguments = $true)][string[]]$ScriptArgs
)
$A = "D:\lukeo3o1\engr-audit"
$path = Join-Path $A "r7\$Out"
New-Item -ItemType Directory -Force -Path (Split-Path $path) | Out-Null
$dockerArgs = @(
  'run', '--rm',
  '-v', "${A}:/audit",
  '-e', 'HOME=/audit/home',
  '-e', 'GIT_CONFIG_GLOBAL=/audit/home/.gitconfig',
  '-w', '/audit',
  'engr-rust:latest',
  'sh', "/audit/r7/harness/$Script"
) + $ScriptArgs
& docker @dockerArgs > $path 2>&1
$code = $LASTEXITCODE
Add-Content -Path $path -Value "--- script exit $code ---"
Get-Content $path
