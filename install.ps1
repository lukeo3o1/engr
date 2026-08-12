<#
.SYNOPSIS
Build engr from this checkout and install it.

.DESCRIPTION
The archives on the `latest` release are the other way to get a binary; this is
the way that works from a clone with no network and nothing to trust. The version
line it prints at the end names the commit the binary was built from, which is
the only thing that identifies a build — there are no version numbers.

It never modifies PATH; it says what to add and leaves that to you.

.PARAMETER BinDir
Where to install. Defaults to %LOCALAPPDATA%\Programs\engr.

.PARAMETER Debug
Build the debug profile instead of release.
#>
[CmdletBinding()]
param(
    [string]$BinDir = (Join-Path $env:LOCALAPPDATA 'Programs\engr'),
    [switch]$DebugProfile
)

$ErrorActionPreference = 'Stop'

# Under `Stop`, Write-Error throws before a following `exit` can set the code, so
# every failure goes through here instead. The codes match install.sh: 2 usage,
# 3 missing toolchain, 8 build or verification failure.
function Fail([string]$message, [int]$code) {
    [Console]::Error.WriteLine($message)
    exit $code
}

$repo = $PSScriptRoot
if (-not (Test-Path (Join-Path $repo 'Cargo.toml'))) {
    Fail 'install.ps1: run this from an engr checkout (no Cargo.toml beside it)' 2
}

if ($null -eq (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Fail 'install.ps1: cargo not found — install Rust from https://rustup.rs' 3
}

if ($DebugProfile) { $profileName = 'debug' } else { $profileName = 'release' }
Write-Host "building    $profileName"

# --target-dir on the command line beats CARGO_TARGET_DIR and any config, so the
# path read below is certain to be the path just written. Guessing `target\`
# instead is worse than a miss: a stale binary left there from an earlier build
# gets installed and then "verified", which is the one thing this must not do.
$targetDir = Join-Path $repo 'target'

Push-Location $repo
try {
    if ($profileName -eq 'release') {
        & cargo build --release --quiet -p engr --target-dir $targetDir
    }
    else {
        & cargo build --quiet -p engr --target-dir $targetDir
    }
    if ($LASTEXITCODE -ne 0) {
        Fail 'install.ps1: the build failed' 8
    }
}
finally {
    Pop-Location
}

$built = Join-Path $targetDir "$profileName\engr.exe"
if (-not (Test-Path $built)) {
    Fail "install.ps1: expected a binary at $built" 8
}

if (-not (Test-Path $BinDir)) {
    New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
}

# Copy to a temporary name and move it into place, so a running engr.exe is
# never overwritten underneath itself.
$target = Join-Path $BinDir 'engr.exe'
$staged = Join-Path $BinDir ".engr.$PID.exe"
Copy-Item $built $staged -Force
Move-Item $staged $target -Force
Write-Host "installed   $target"

# Prove the thing that was installed actually runs, rather than trusting the copy.
$version = & $target --version
if ($LASTEXITCODE -ne 0) {
    Fail 'install.ps1: the installed binary did not run' 8
}
Write-Host "verified    $version"

$onPath = ($env:PATH -split ';') -contains $BinDir
if (-not $onPath) {
    Write-Host ''
    Write-Host "$BinDir is not on your PATH. Add it for this session:"
    Write-Host "  `$env:PATH = `"$BinDir;`$env:PATH`""
    Write-Host 'Or permanently:'
    Write-Host "  [Environment]::SetEnvironmentVariable('PATH', `"$BinDir;`" + [Environment]::GetEnvironmentVariable('PATH','User'), 'User')"
}
