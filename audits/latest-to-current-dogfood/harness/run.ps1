# Run one engr binary inside the pinned container against the audit tree.
#
#   .\run.ps1 <latest|current> <workdir-relative-to-/audit> <args...>
#
# Everything under D:\lukeo3o1\engr-audit is mounted at /audit, so the two
# implementations and every workspace they touch share one filesystem view and
# transcripts are reproducible.
param(
  [Parameter(Mandatory = $true)][string]$Which,
  [Parameter(Mandatory = $true)][string]$WorkDir,
  [Parameter(ValueFromRemainingArguments = $true)][string[]]$EngrArgs
)
$bin = "/audit/bin/engr-$Which"
docker run --rm `
  -v "D:\lukeo3o1\engr-audit:/audit" `
  -e HOME=/audit/home `
  -e GIT_CONFIG_GLOBAL=/audit/home/.gitconfig `
  -w "/audit/$WorkDir" `
  engr-rust:latest `
  $bin @EngrArgs
exit $LASTEXITCODE
