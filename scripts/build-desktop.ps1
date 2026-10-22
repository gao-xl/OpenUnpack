param([switch]$SkipPrepare, [switch]$SkipInstall, [switch]$Installer, [switch]$OfflineInstaller, [switch]$OfflineMaterials)
. (Join-Path $PSScriptRoot 'common.ps1')
Assert-OpenUnpackWindows
Import-OpenUnpackMsvc
if ($OfflineInstaller -and -not $Installer) { throw '-OfflineInstaller 需要同时使用 -Installer。' }
$taskRoot = Get-OpenUnpackRoot
if (-not $SkipPrepare) { & (Join-Path $PSScriptRoot 'prepare-desktop.ps1') -SkipInstall:$SkipInstall -SkipFrontendBuild }
$taskHadTargetOverride = Test-Path Env:CARGO_TARGET_DIR
$taskOriginalTargetOverride = $env:CARGO_TARGET_DIR
Push-Location $taskRoot
try {
    & (Join-Path $PSScriptRoot 'collect-release-materials.ps1') -Offline:$OfflineMaterials
    $taskMetadata = & cargo metadata --format-version 1 --no-deps --locked
    if ($LASTEXITCODE -ne 0) { throw '无法确定 Cargo 构建输出目录。' }
    $taskTarget = ($taskMetadata | ConvertFrom-Json).target_directory
    if (-not $taskTarget -or -not [IO.Path]::IsPathFullyQualified($taskTarget)) { throw 'Cargo 输出目录无效。' }
    # Tauri runs Cargo from apps/desktop-host. A relative override must not
    # resolve to a second checkout-local target and leave an old installer here.
    $env:CARGO_TARGET_DIR = $taskTarget
    $taskConfig = Get-Content -Raw -LiteralPath apps/desktop-host/tauri.conf.json | ConvertFrom-Json
    $taskVersion = $taskConfig.version
    $taskRelease = Join-Path $taskRoot 'artifacts/release'
    New-OpenUnpackDirectory $taskRelease
    if ($Installer) {
        $taskWebViewMode = if ($OfflineInstaller) { 'offlineInstaller' } else { 'embedBootstrapper' }
        $taskFlavor = if ($OfflineInstaller) { 'offline' } else { 'standard' }
        $taskOverride = Join-Path $taskRoot 'artifacts/release-installer.config.json'
        @{ bundle = @{ licenseFile = '../../LICENSE'; resources = @{ '../../artifacts/release-materials/' = 'release-materials/'; '../../LICENSE' = 'LICENSE'; '../../THIRD_PARTY_NOTICES.md' = 'THIRD_PARTY_NOTICES.md' }; windows = @{ webviewInstallMode = @{ type = $taskWebViewMode }; nsis = @{ installMode = 'currentUser'; languages = @('SimpChinese', 'English') } } } } |
            ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $taskOverride -Encoding utf8NoBOM
        # Tauri owns the fresh frontend hook and final Host build for installers.
        # Do not prebuild either here: its installer config can change the Host.
        Push-Location (Join-Path $taskRoot 'apps/desktop-host')
        try { Invoke-OpenUnpackTool (Join-Path $taskRoot 'apps/desktop-ui/node_modules/.bin/tauri.cmd') @('build', '--bundles', 'nsis', '--features', 'custom-protocol', '--config', $taskOverride, '--ci', '--', '--locked') }
        finally { Pop-Location }
        $taskSetup = Join-Path $taskTarget "release/bundle/nsis/$($taskConfig.productName)_${taskVersion}_x64-setup.exe"
        if (-not (Test-Path -LiteralPath $taskSetup -PathType Leaf)) { throw 'Tauri 未输出本次版本的 NSIS 安装包。' }
        $taskSetupInfo = [Diagnostics.FileVersionInfo]::GetVersionInfo($taskSetup)
        if ($taskSetupInfo.ProductName -ne $taskConfig.productName -or $taskSetupInfo.ProductVersion -ne $taskVersion) { throw '安装包产品或版本与本次构建不符。' }
        Copy-Item -LiteralPath $taskSetup -Destination (Join-Path $taskRelease "OpenUnpack-$taskVersion-windows-x64-$taskFlavor-setup.exe") -Force
    } else {
        # SkipPrepare skips dependency preparation, never fresh frontend compilation.
        Invoke-OpenUnpackTool npm @('--prefix', (Join-Path $taskRoot 'apps/desktop-ui'), 'run', 'build')
        Invoke-OpenUnpackTool cargo @('build', '-p', 'openunpack-desktop', '--release', '--features', 'custom-protocol', '--locked')
    }
    # Tauri can rebuild the Host for the installer config and patch its bundle type.
    # Freeze the portable payload only after that final Host has been produced.
    $portable = Join-Path $taskRoot 'artifacts/OpenUnpack-portable'
    Reset-OpenUnpackBuildDirectory -Path $portable -Parent (Join-Path $taskRoot 'artifacts')
    Copy-Item -LiteralPath (Join-Path $taskTarget 'release/openunpack-desktop.exe') -Destination (Join-Path $portable 'OpenUnpack.exe')
    Copy-Item -LiteralPath (Join-Path $taskRoot 'artifacts/worker') -Destination $portable -Recurse -Force
    Copy-Item -LiteralPath (Join-Path $taskRoot 'LICENSE') -Destination $portable
    Copy-Item -LiteralPath (Join-Path $taskRoot 'THIRD_PARTY_NOTICES.md') -Destination $portable
    Copy-Item -LiteralPath (Join-Path $taskRoot 'apps/desktop-host/installer-LICENSE-MIT.txt') -Destination $portable
    Copy-Item -LiteralPath (Join-Path $taskRoot 'README.md') -Destination $portable
    Copy-Item -LiteralPath (Join-Path $taskRoot 'artifacts/release-materials') -Destination $portable -Recurse -Force
    Write-OpenUnpackChecksums $portable
    $taskZip = Join-Path $taskRelease "OpenUnpack-$taskVersion-windows-x64-portable.zip"
    if (Test-Path -LiteralPath $taskZip) { Remove-Item -LiteralPath (Assert-OpenUnpackOwnedPath -Path $taskZip -Parent $taskRelease) -Force }
    Compress-Archive -LiteralPath $portable -DestinationPath $taskZip -CompressionLevel Optimal
    & (Join-Path $PSScriptRoot 'package-source.ps1') -Version $taskVersion
    Write-OpenUnpackChecksums $taskRelease
    Write-Output "桌面程序：$portable\OpenUnpack.exe"
    Write-Output "发行文件：$taskRelease"
} finally {
    if ($taskHadTargetOverride) { $env:CARGO_TARGET_DIR = $taskOriginalTargetOverride }
    else { Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue }
    Pop-Location
}
