[CmdletBinding()]
param(
    [string]$Version = $env:ENGR_VERSION,
    [string]$BinDir = $env:ENGR_INSTALL_DIR,
    [string]$Target = $env:ENGR_TARGET,
    [switch]$Help
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$Repository = 'lukeo3o1/engr'

function Show-Usage {
    @'
Usage: install.ps1 [-Version VERSION] [-BinDir PATH] [-Target TARGET]

Installs a verified Engr release on Windows without administrator privileges.

Parameters:
  -Version VERSION  Release version to install (default: latest GitHub release)
  -BinDir PATH      Destination directory (default: $ENGR_INSTALL_DIR or $HOME\.local\bin)
  -Target TARGET    Exact release target to install. Defaults to the native Windows target.
  -Help             Show this help text

Environment equivalents: ENGR_VERSION, ENGR_INSTALL_DIR, and ENGR_TARGET.
'@ | Write-Output
}

function Normalize-Version([string]$Candidate) {
    $normalized = $Candidate -replace '^v', ''
    if ($normalized -notmatch '^[0-9]+\.[0-9]+\.[0-9]+([-.+][0-9A-Za-z.-]+)?$') {
        throw "Invalid release version: $Candidate"
    }
    return $normalized
}

function Resolve-Version([string]$RequestedVersion) {
    if (-not [string]::IsNullOrWhiteSpace($RequestedVersion)) {
        return Normalize-Version $RequestedVersion
    }
    $latest = Invoke-RestMethod -Headers @{ 'User-Agent' = 'engr-installer' } -Uri "https://api.github.com/repos/$Repository/releases/latest"
    return Normalize-Version ([string]$latest.tag_name)
}

function Detect-Target {
    switch ([System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture) {
        'X64' { return 'x86_64-pc-windows-msvc' }
        'Arm64' { return 'aarch64-pc-windows-msvc' }
        default { throw "Unsupported Windows architecture: $([System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture)" }
    }
}

function Get-TargetRecord($Manifest, [string]$SelectedTarget) {
    $property = @($Manifest.targets.PSObject.Properties | Where-Object { $_.Name -eq $SelectedTarget }) | Select-Object -First 1
    if ($null -eq $property) {
        throw "Release v$Version has no artifact for target $SelectedTarget"
    }
    return $property.Value
}

function Test-PathEntry([string]$Directory) {
    return @($env:Path -split ';' | ForEach-Object { $_.TrimEnd('\') }) -contains $Directory.TrimEnd('\')
}

function Install-Engr {
    if ($Help) {
        Show-Usage
        return
    }

    $resolvedVersion = Resolve-Version $Version
    $resolvedTarget = if ([string]::IsNullOrWhiteSpace($Target)) { Detect-Target } else { $Target }
    $resolvedBinDir = if ([string]::IsNullOrWhiteSpace($BinDir)) { Join-Path $HOME '.local\bin' } else { $BinDir }
    if ([string]::IsNullOrWhiteSpace($resolvedBinDir)) {
        throw 'Installation directory is empty; set -BinDir or ENGR_INSTALL_DIR.'
    }

    $temporary = Join-Path ([System.IO.Path]::GetTempPath()) ("engr-install-" + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $temporary | Out-Null
    try {
        $releaseBase = "https://github.com/$Repository/releases/download/v$resolvedVersion"
        $manifestPath = Join-Path $temporary 'release-manifest.json'
        Invoke-WebRequest -Headers @{ 'User-Agent' = 'engr-installer' } -Uri "$releaseBase/release-manifest.json" -OutFile $manifestPath
        $manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
        if ([string]$manifest.version -ne $resolvedVersion) {
            throw 'Release manifest version does not match the requested version.'
        }
        if ([int]$manifest.protocol -ne 1) {
            throw 'Release manifest protocol is not supported by this installer.'
        }

        $record = Get-TargetRecord $manifest $resolvedTarget
        $expectedArtifact = "engr-$resolvedVersion-$resolvedTarget.zip"
        if ([string]$record.path -ne $expectedArtifact) {
            throw "Unexpected artifact name in release manifest: $($record.path)"
        }
        $expectedHash = ([string]$record.sha256).ToLowerInvariant()
        if ($expectedHash -notmatch '^[0-9a-f]{64}$') {
            throw 'Release manifest contains an invalid SHA-256 value.'
        }

        $checksumPath = Join-Path $temporary "$expectedArtifact.sha256"
        $archivePath = Join-Path $temporary $expectedArtifact
        Invoke-WebRequest -Headers @{ 'User-Agent' = 'engr-installer' } -Uri "$releaseBase/$expectedArtifact.sha256" -OutFile $checksumPath
        $checksumFromFile = ((Get-Content -LiteralPath $checksumPath -TotalCount 1) -split '\s+')[0].ToLowerInvariant()
        if ($checksumFromFile -ne $expectedHash) {
            throw 'Checksum file and release manifest disagree.'
        }
        Invoke-WebRequest -Headers @{ 'User-Agent' = 'engr-installer' } -Uri "$releaseBase/$expectedArtifact" -OutFile $archivePath
        $actualHash = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actualHash -ne $expectedHash) {
            throw 'Downloaded artifact SHA-256 does not match the release manifest.'
        }

        $extract = Join-Path $temporary 'extract'
        Expand-Archive -LiteralPath $archivePath -DestinationPath $extract -Force
        $files = @(Get-ChildItem -LiteralPath $extract -Recurse -File)
        if ($files.Count -ne 1 -or $files[0].Name -ne 'engr.exe') {
            throw 'Release archive must contain exactly one engr.exe binary.'
        }

        New-Item -ItemType Directory -Force -Path $resolvedBinDir | Out-Null
        $destination = Join-Path $resolvedBinDir 'engr.exe'
        $temporaryDestination = Join-Path $resolvedBinDir (".engr-" + [Guid]::NewGuid().ToString('N') + '.tmp')
        Copy-Item -LiteralPath $files[0].FullName -Destination $temporaryDestination
        Move-Item -LiteralPath $temporaryDestination -Destination $destination -Force

        $reported = & $destination version --json
        if ($LASTEXITCODE -ne 0) {
            throw 'Installed engr binary did not run successfully.'
        }
        $reportedVersion = ($reported | ConvertFrom-Json).implementation_version
        if ([string]$reportedVersion -ne $resolvedVersion) {
            throw 'Installed engr binary reported a version different from the requested release.'
        }

        Write-Output "Installed Engr $resolvedVersion for $resolvedTarget at $destination"
        if (-not (Test-PathEntry $resolvedBinDir)) {
            Write-Output "Add $resolvedBinDir to PATH to invoke engr without its full path."
        }
    }
    finally {
        Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
    }
}

if ($MyInvocation.InvocationName -ne '.') {
    try {
        Install-Engr
    }
    catch {
        Write-Error "engr installer: $($_.Exception.Message)"
        exit 1
    }
}
