<#
.SYNOPSIS
    Builds a Windows x64 headless Host, CLI and self-contained Worker directory.
#>
param(
    [switch]$Release = $true,
    [switch]$SkipPrepare
)

. (Join-Path $PSScriptRoot 'common.ps1')
. (Join-Path $PSScriptRoot 'runtime-policy.ps1')
Assert-OpenUnpackWindows
Import-OpenUnpackMsvc
$taskRoot = Get-OpenUnpackRoot
if (-not $SkipPrepare) {
    & (Join-Path $PSScriptRoot 'prepare-desktop.ps1') -SkipFrontendBuild
}

$taskWorker = Join-Path $taskRoot 'artifacts/worker'
# When legacy Worker binaries are present, verify them; otherwise run in native Rust microkernel mode.
if (Test-Path -LiteralPath (Join-Path $taskWorker 'OpenUnpack.Worker.exe') -PathType Leaf) {
    foreach ($taskRequired in @('OpenUnpack.Worker.exe', 'OpenUnpack.Worker.dll', 'OpenUnpack.Engine.dll',
        '7z.dll', '7zip-License.txt', '7zip-provenance.json', 'THIRD_PARTY_NOTICES.md')) {
        if (-not (Test-Path -LiteralPath (Join-Path $taskWorker $taskRequired) -PathType Leaf)) {
            throw "缺少已准备的 Worker 文件：$taskRequired。请运行 prepare-desktop.ps1 或省略 -SkipPrepare。"
        }
    }
    $null = Get-OpenUnpackPublishedRuntime -WorkerDirectory $taskWorker
}
foreach ($taskRequired in @('LICENSE', 'THIRD_PARTY_NOTICES.md', 'README.md')) {
    if (-not (Test-Path -LiteralPath (Join-Path $taskRoot $taskRequired) -PathType Leaf)) {
        throw "缺少无头构建说明或许可文件：$taskRequired"
    }
}

Push-Location $taskRoot
try {
    $taskMetadata = & cargo metadata --format-version 1 --no-deps --locked
    if ($LASTEXITCODE -ne 0) { throw '无法确定 Cargo 构建输出目录。' }
    $taskTarget = ($taskMetadata | ConvertFrom-Json).target_directory
    if (-not $taskTarget -or -not [IO.Path]::IsPathFullyQualified($taskTarget)) {
        throw 'Cargo 输出目录无效。'
    }

    $taskCargoArgs = @('build', '-p', 'openunpack-desktop', '-p', 'openunpack-cli', '--locked')
    if ($Release) { $taskCargoArgs += '--release' }
    Invoke-OpenUnpackTool cargo $taskCargoArgs
    & (Join-Path $PSScriptRoot 'collect-release-materials.ps1')

    $taskProfile = if ($Release) { 'release' } else { 'debug' }
    $taskBinaries = Join-Path $taskTarget $taskProfile
    $taskDaemon = Join-Path $taskBinaries 'openunpack-desktop.exe'
    $taskCli = Join-Path $taskBinaries 'openunpack.exe'
    foreach ($taskBinary in @($taskDaemon, $taskCli)) {
        if (-not (Test-Path -LiteralPath $taskBinary -PathType Leaf)) {
            throw 'Cargo 未输出所需的 Windows Host 或 CLI，未生成无头构建目录。'
        }
    }

    $taskOutput = Join-Path $taskRoot 'artifacts/headless-server'
    Reset-OpenUnpackBuildDirectory -Path $taskOutput -Parent (Join-Path $taskRoot 'artifacts')
    Copy-Item -LiteralPath $taskDaemon -Destination (Join-Path $taskOutput 'openunpack-daemon.exe')
    Copy-Item -LiteralPath $taskCli -Destination (Join-Path $taskOutput 'openunpack.exe')
    if (Test-Path -LiteralPath (Join-Path $taskRoot 'tools') -PathType Container) {
        Copy-Item -LiteralPath (Join-Path $taskRoot 'tools') -Destination $taskOutput -Recurse -Force
    }
    foreach ($taskDocument in @('LICENSE', 'THIRD_PARTY_NOTICES.md', 'README.md')) {
        Copy-Item -LiteralPath (Join-Path $taskRoot $taskDocument) -Destination $taskOutput
    }
    Copy-Item -LiteralPath (Join-Path $taskRoot 'artifacts/release-materials') -Destination $taskOutput -Recurse
    Copy-Item -LiteralPath (Join-Path $taskRoot 'scripts/integrations') -Destination $taskOutput -Recurse
    Write-Output "Windows x64 无头构建目录：$taskOutput"
    Write-Output '启动服务：.\openunpack-daemon.exe --headless（目录授权和 API 密钥配置见 README）'
    Write-Output '独立 CLI：.\openunpack.exe status'
}
finally { Pop-Location }
