<#
.SYNOPSIS
Install engr.

.DESCRIPTION
By default this downloads the archive for your platform from the `latest`
release, checks it against the published SHA256SUMS, and installs it: no
checkout and no Rust toolchain needed. -FromSource builds from a checkout
instead, which is the path that works with no network and nothing to trust.

There are no version numbers. `latest` moves, and the binary reports the commit
it was built from; that is what the version line at the end names.

It never modifies PATH; it says what to add and leaves that to you.

Exit codes: 2 usage, 3 this environment cannot do it, 8 the download, build or
verification failed.

.PARAMETER BinDir
Where to install. Defaults to %LOCALAPPDATA%\Programs\engr.

.PARAMETER FromSource
Build from an engr checkout instead of downloading.

.PARAMETER DebugProfile
With -FromSource, build the debug profile instead of release.
#>
[CmdletBinding()]
param(
    [string]$BinDir = (Join-Path $env:LOCALAPPDATA 'Programs\engr'),
    [switch]$FromSource,
    [switch]$DebugProfile
)

$ErrorActionPreference = 'Stop'

# Under `Stop`, Write-Error throws before a following `exit` can set the code, so
# every failure goes through here instead. The codes match install.sh.
function Fail([string[]]$messages, [int]$code) {
    foreach ($message in $messages) { [Console]::Error.WriteLine($message) }
    exit $code
}

$release = 'https://github.com/lukeo3o1/engr/releases/download/latest'
$repo = $PSScriptRoot

if ($DebugProfile -and -not $FromSource) {
    Fail 'install.ps1: -DebugProfile only applies to -FromSource' 2
}

# $work is created inside the try so the finally clears it on every path out,
# including the `exit` inside Fail.
$work = $null
try {
    if ($FromSource) {
        if (-not (Test-Path (Join-Path $repo 'Cargo.toml'))) {
            Fail 'install.ps1: -FromSource needs an engr checkout (no Cargo.toml beside this script)' 2
        }
        if ($null -eq (Get-Command cargo -ErrorAction SilentlyContinue)) {
            Fail 'install.ps1: cargo not found - install Rust from https://rustup.rs' 3
        }

        if ($DebugProfile) { $profileName = 'debug' } else { $profileName = 'release' }
        Write-Host "building    $profileName"

        # --target-dir on the command line beats CARGO_TARGET_DIR and any config,
        # so the path read below is certain to be the path just written. Guessing
        # `target\` instead is worse than a miss: a stale binary left there from
        # an earlier build gets installed and then "verified", which is the one
        # thing this must not do.
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
    }
    else {
        # Only x86-64 is published. Windows on ARM runs it under emulation, which
        # is a real answer; anything else is not.
        $arch = $env:PROCESSOR_ARCHITECTURE
        if ($arch -ne 'AMD64' -and $arch -ne 'ARM64') {
            Fail "install.ps1: no archive is published for $arch; use -FromSource" 3
        }
        $archive = 'engr-x86_64-pc-windows-msvc.zip'

        $work = Join-Path ([System.IO.Path]::GetTempPath()) ('engr-' + [guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $work -Force | Out-Null

        # Windows PowerShell 5.1 can still default to TLS 1.0, which GitHub
        # refuses. Add to whatever is set rather than replacing it.
        if ($PSVersionTable.PSVersion.Major -lt 6) {
            [Net.ServicePointManager]::SecurityProtocol =
            [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
        }
        # The progress bar makes Invoke-WebRequest roughly an order of magnitude
        # slower under 5.1.
        $ProgressPreference = 'SilentlyContinue'

        Write-Host "downloading $archive"
        try {
            Invoke-WebRequest -UseBasicParsing -Uri "$release/$archive" `
                -OutFile (Join-Path $work $archive)
            Invoke-WebRequest -UseBasicParsing -Uri "$release/SHA256SUMS" `
                -OutFile (Join-Path $work 'SHA256SUMS')
        }
        catch {
            Fail @("install.ps1: could not download $archive",
                '            if no `latest` release has been published yet, use -FromSource') 8
        }

        # The sums come from the same release as the archive, so this catches a
        # truncated or corrupted download, not a compromised release. Worth doing
        # for the first; do not read it as the second.
        $expected = Get-Content (Join-Path $work 'SHA256SUMS') |
        Where-Object { $_ -match ('\s' + [regex]::Escape($archive) + '$') } |
        ForEach-Object { ($_ -split '\s+')[0] }
        if (-not $expected) {
            Fail "install.ps1: SHA256SUMS does not list $archive" 8
        }
        # sha256sum writes lowercase and Get-FileHash returns uppercase; -ne on
        # strings is case-insensitive, which is what makes this comparison work.
        $actual = (Get-FileHash (Join-Path $work $archive) -Algorithm SHA256).Hash
        if ($actual -ne $expected) {
            Fail @("install.ps1: checksum mismatch for $archive",
                "            expected $expected",
                "            got      $actual") 8
        }
        Write-Host 'checksum    ok'

        Expand-Archive -Path (Join-Path $work $archive) -DestinationPath $work -Force
        $built = Join-Path $work 'engr.exe'
        if (-not (Test-Path $built)) {
            Fail "install.ps1: $archive does not contain engr.exe" 8
        }
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
}
finally {
    if ($work -and (Test-Path $work)) {
        Remove-Item $work -Recurse -Force -Confirm:$false
    }
}

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
