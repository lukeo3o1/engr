# Run one engr command and append an exact transcript entry.
#
#   .\t.ps1 -Log 01-historical -Which latest -WorkDir project -- ls --all
#
# Records the argv, stdout, stderr and exit code verbatim. Transcripts are the
# audit's primary evidence, so nothing here reformats or filters output.
param(
  [Parameter(Mandatory = $true)][string]$Log,
  [Parameter(Mandatory = $true)][string]$Which,
  [Parameter(Mandatory = $true)][string]$WorkDir,
  [Parameter(ValueFromRemainingArguments = $true)][string[]]$EngrArgs
)
$ErrorActionPreference = 'Continue'
$dir = "D:\lukeo3o1\engr-audit\transcripts"
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$path = Join-Path $dir "$Log.txt"

$outFile = [System.IO.Path]::GetTempFileName()
$errFile = [System.IO.Path]::GetTempFileName()
$dockerArgs = @(
  'run', '--rm',
  '-v', 'D:\lukeo3o1\engr-audit:/audit',
  '-e', 'HOME=/audit/home',
  '-e', 'GIT_CONFIG_GLOBAL=/audit/home/.gitconfig',
  '-w', "/audit/$WorkDir",
  'engr-rust:latest',
  "/audit/bin/engr-$Which"
) + $EngrArgs
# Start-Process joins an argument array with spaces and quotes nothing, so an
# argument carrying a space would arrive as several. Quote each one here.
$quoted = $dockerArgs | ForEach-Object {
  $a = $_ -replace '(\\*)"', '$1$1\"'
  $a = $a -replace '(\\+)$', '$1$1'
  '"' + $a + '"'
}
$proc = Start-Process -FilePath 'docker' -ArgumentList $quoted -NoNewWindow -Wait -PassThru `
  -RedirectStandardOutput $outFile -RedirectStandardError $errFile
$code = $proc.ExitCode
$out = Get-Content $outFile -Raw
$err = Get-Content $errFile -Raw
Remove-Item $outFile, $errFile -Force

$entry = @()
$entry += "$ engr-$Which " + ($EngrArgs -join ' ') + "   # cwd=$WorkDir"
if ($out) { $entry += "--- stdout ---"; $entry += $out.TrimEnd() }
if ($err) { $entry += "--- stderr ---"; $entry += $err.TrimEnd() }
$entry += "--- exit $code ---"
$entry += ""
Add-Content -Path $path -Encoding utf8 -Value ($entry -join "`n")

if ($out) { Write-Output $out.TrimEnd() }
if ($err) { Write-Output "STDERR: $($err.TrimEnd())" }
Write-Output "EXIT: $code"
