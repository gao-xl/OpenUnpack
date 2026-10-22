<#
.SYNOPSIS
    Compiles OpenUnpack headless daemon and CLI tools without GUI/installer requirements.
#>
param(
    [switch]$Release = $true
)

$ErrorActionPreference = 'Stop'
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$rootDir = (Get-Item $scriptDir).Parent.FullName

Push-Location $rootDir
try {
    Write-Host "==================================================" -ForegroundColor Cyan
    Write-Host " Building OpenUnpack Headless Daemon & CLI Tools" -ForegroundColor Cyan
    Write-Host "==================================================" -ForegroundColor Cyan

    $cargoArgs = @('build', '-p', 'openunpack-desktop', '-p', 'openunpack-cli')
    if ($Release) {
        $cargoArgs += '--release'
    }

    Write-Host "Running cargo $($cargoArgs -join ' ')..." -ForegroundColor Yellow
    & cargo $cargoArgs
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo build failed with exit code $LASTEXITCODE"
    }

    $targetSubdir = if ($Release) { 'release' } else { 'debug' }
    $binDir = Join-Path $rootDir "target/$targetSubdir"
    $outDir = Join-Path $rootDir "artifacts/headless-server"

    if (-not (Test-Path $outDir)) {
        New-Item -ItemType Directory -Path $outDir -Force | Out-Null
    }

    $ext = if ($IsWindows -or $env:OS -like "*Windows*") { ".exe" } else { "" }
    
    $daemonSrc = Join-Path $binDir "openunpack-desktop$ext"
    $cliSrc = Join-Path $binDir "openunpack$ext"

    if (Test-Path $daemonSrc) {
        Copy-Item -LiteralPath $daemonSrc -Destination (Join-Path $outDir "openunpack-daemon$ext") -Force
        Write-Host " Copied Daemon -> $outDir/openunpack-daemon$ext" -ForegroundColor Green
    }

    if (Test-Path $cliSrc) {
        Copy-Item -LiteralPath $cliSrc -Destination (Join-Path $outDir "openunpack$ext") -Force
        Write-Host " Copied CLI    -> $outDir/openunpack$ext" -ForegroundColor Green
    }

    # Copy integration scripts
    $integrationsSrc = Join-Path $rootDir "scripts/integrations"
    if (Test-Path $integrationsSrc) {
        Copy-Item -Path $integrationsSrc -Destination $outDir -Recurse -Force
        Write-Host " Copied Integrations suite" -ForegroundColor Green
    }

    Write-Host "==================================================" -ForegroundColor Cyan
    Write-Host " Headless Server Build Complete: $outDir" -ForegroundColor Green
    Write-Host " Run daemon with: ./openunpack-daemon --headless --port 18420" -ForegroundColor Cyan
    Write-Host " Run CLI with:    ./openunpack status" -ForegroundColor Cyan
    Write-Host "==================================================" -ForegroundColor Cyan
}
finally {
    Pop-Location
}
