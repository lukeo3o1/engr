$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$installer = Join-Path $repoRoot 'install.ps1'
$tokens = $null
$errors = $null
[System.Management.Automation.Language.Parser]::ParseFile($installer, [ref]$tokens, [ref]$errors) | Out-Null
if ($errors.Count -gt 0) {
    throw ($errors | ForEach-Object Message | Out-String)
}

$content = Get-Content -Raw -LiteralPath $installer
foreach ($required in @('Get-FileHash', 'Expand-Archive', 'release-manifest.json', 'Get-TargetRecord', 'implementation_version')) {
    if (-not $content.Contains($required)) {
        throw "Windows installer is missing required verification behavior: $required"
    }
}

. $installer
$binary = Join-Path $repoRoot 'target\debug\engr.exe'
if (-not (Test-Path -LiteralPath $binary)) {
    Write-Output 'Windows installer syntax and verification hooks verified (runtime binary unavailable for integration check).'
    exit 0
}

$testRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("engr-installer-test-" + [Guid]::NewGuid().ToString('N'))
$releaseDir = Join-Path $testRoot 'release'
$installDir = Join-Path $testRoot 'bin'
try {
    New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null
    $target = 'x86_64-pc-windows-msvc'
    $archiveName = "engr-0.1.0-$target.zip"
    $stage = Join-Path $testRoot 'stage'
    New-Item -ItemType Directory -Force -Path $stage | Out-Null
    Copy-Item -LiteralPath $binary -Destination (Join-Path $stage 'engr.exe')
    Compress-Archive -LiteralPath (Join-Path $stage 'engr.exe') -DestinationPath (Join-Path $releaseDir $archiveName)
    $hash = (Get-FileHash -LiteralPath (Join-Path $releaseDir $archiveName) -Algorithm SHA256).Hash.ToLowerInvariant()
    Set-Content -LiteralPath (Join-Path $releaseDir "$archiveName.sha256") -Value "$hash  $archiveName" -Encoding ascii
    @{
        version = '0.1.0'
        protocol = 1
        targets = @{
            $target = @{
                path = $archiveName
                sha256 = $hash
                sbom = "sbom-$target.cdx.json"
            }
        }
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $releaseDir 'release-manifest.json') -Encoding utf8

    function Invoke-WebRequest {
        param([hashtable]$Headers, [string]$Uri, [string]$OutFile)
        Copy-Item -LiteralPath (Join-Path $releaseDir ([System.IO.Path]::GetFileName($Uri))) -Destination $OutFile
    }

    $Version = '0.1.0'
    $BinDir = $installDir
    $Target = $target
    $Help = $false
    Install-Engr
    $installed = Join-Path $installDir 'engr.exe'
    if (-not (Test-Path -LiteralPath $installed)) {
        throw 'Windows installer did not create engr.exe.'
    }
    $installedVersion = (& $installed version --json | ConvertFrom-Json).implementation_version
    if ($installedVersion -ne '0.1.0') {
        throw "Windows installer installed an unexpected version: $installedVersion"
    }

    Set-Content -LiteralPath (Join-Path $releaseDir "$archiveName.sha256") -Value (('0' * 64) + "  $archiveName") -Encoding ascii
    $rejected = $false
    try {
        Install-Engr
    }
    catch {
        if ($_.Exception.Message -notmatch 'Checksum file and release manifest disagree') {
            throw
        }
        $rejected = $true
    }
    if (-not $rejected) {
        throw 'Windows installer accepted a checksum file that disagreed with the manifest.'
    }
}
finally {
    Remove-Item -LiteralPath $testRoot -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Output 'Windows installer syntax, checksum verification, rejection, and installation verified.'
