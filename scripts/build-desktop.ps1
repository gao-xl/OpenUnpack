param([switch]$SkipPrepare, [switch]$SkipInstall, [switch]$Installer, [switch]$OfflineInstaller, [switch]$OfflineMaterials)
. (Join-Path $PSScriptRoot 'common.ps1')
Assert-OpenUnpackWindows
Import-OpenUnpackMsvc
if ($OfflineInstaller -and -not $Installer) { throw '-OfflineInstaller 需要同时使用 -Installer。' }
$taskRoot = Get-OpenUnpackRoot
if (-not $SkipPrepare) { & (Join-Path $PSScriptRoot 'prepare-desktop.ps1') -SkipInstall:$SkipInstall }
Push-Location $taskRoot
try {
    & (Join-Path $PSScriptRoot 'collect-release-materials.ps1') -Offline:$OfflineMaterials
    Invoke-OpenUnpackTool cargo @('build', '-p', 'openunpack-desktop', '--release', '--features', 'custom-protocol', '--locked')
    $portable = Join-Path $taskRoot 'artifacts/OpenUnpack-portable'
    Reset-OpenUnpackBuildDirectory -Path $portable -Parent (Join-Path $taskRoot 'artifacts')
    Copy-Item -LiteralPath (Join-Path $taskRoot 'target/release/openunpack-desktop.exe') -Destination (Join-Path $portable 'OpenUnpack.exe')
    Copy-Item -LiteralPath (Join-Path $taskRoot 'artifacts/worker') -Destination $portable -Recurse -Force
    Copy-Item -LiteralPath (Join-Path $taskRoot 'LICENSE') -Destination $portable
    Copy-Item -LiteralPath (Join-Path $taskRoot 'THIRD_PARTY_NOTICES.md') -Destination $portable
    Copy-Item -LiteralPath (Join-Path $taskRoot 'README.md') -Destination $portable
    Copy-Item -LiteralPath (Join-Path $taskRoot 'artifacts/release-materials') -Destination $portable -Recurse -Force
    Write-OpenUnpackChecksums $portable
    $taskVersion = (Get-Content -Raw -LiteralPath apps/desktop-host/tauri.conf.json | ConvertFrom-Json).version
    $taskRelease = Join-Path $taskRoot 'artifacts/release'
    New-OpenUnpackDirectory $taskRelease
    $taskZip = Join-Path $taskRelease "OpenUnpack-$taskVersion-windows-x64-portable.zip"
    if (Test-Path -LiteralPath $taskZip) { Remove-Item -LiteralPath (Assert-OpenUnpackOwnedPath -Path $taskZip -Parent $taskRelease) -Force }
    Compress-Archive -LiteralPath $portable -DestinationPath $taskZip -CompressionLevel Optimal
    if ($Installer) {
        $taskWebViewMode = if ($OfflineInstaller) { 'offlineInstaller' } else { 'embedBootstrapper' }
        $taskFlavor = if ($OfflineInstaller) { 'offline' } else { 'standard' }
        $taskOverride = Join-Path $taskRoot 'artifacts/release-installer.config.json'
        @{ bundle = @{ licenseFile = '../../LICENSE'; resources = @{ '../../artifacts/release-materials/' = 'release-materials/'; '../../LICENSE' = 'LICENSE'; '../../THIRD_PARTY_NOTICES.md' = 'THIRD_PARTY_NOTICES.md' }; windows = @{ webviewInstallMode = @{ type = $taskWebViewMode }; nsis = @{ installMode = 'currentUser'; languages = @('SimpChinese', 'English') } } } } |
            ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $taskOverride -Encoding utf8NoBOM
        Push-Location (Join-Path $taskRoot 'apps/desktop-host')
        try { Invoke-OpenUnpackTool (Join-Path $taskRoot 'apps/desktop-ui/node_modules/.bin/tauri.cmd') @('build', '--bundles', 'nsis', '--config', $taskOverride, '--ci', '--', '--locked') }
        finally { Pop-Location }
        $taskInstallers = @(Get-ChildItem -LiteralPath (Join-Path $taskRoot 'target/release/bundle/nsis') -File -Filter '*-setup.exe')
        if ($taskInstallers.Count -eq 0) { throw 'Tauri 未输出 NSIS 安装包。' }
        foreach ($taskSetup in $taskInstallers) {
            Copy-Item -LiteralPath $taskSetup.FullName -Destination (Join-Path $taskRelease "OpenUnpack-$taskVersion-windows-x64-$taskFlavor-setup.exe") -Force
        }
    }
    & (Join-Path $PSScriptRoot 'package-source.ps1') -Version $taskVersion
    Write-OpenUnpackChecksums $taskRelease
    Write-Output "桌面程序：$portable\OpenUnpack.exe"
    Write-Output "发行文件：$taskRelease"
} finally { Pop-Location }
