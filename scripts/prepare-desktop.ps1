param(
    [switch]$SkipInstall,
    [string]$RuntimeFrameworkVersion,
    [switch]$SkipFrontendBuild
)
. (Join-Path $PSScriptRoot 'common.ps1')
Assert-OpenUnpackWindows
Import-OpenUnpackMsvc
$taskRoot = Get-OpenUnpackRoot
$workerOut = Join-Path $taskRoot 'artifacts/worker'
Reset-OpenUnpackBuildDirectory -Path $workerOut -Parent (Join-Path $taskRoot 'artifacts')
Copy-Item -LiteralPath (Join-Path $taskRoot 'THIRD_PARTY_NOTICES.md') -Destination $workerOut
if (-not $SkipInstall) {
    Invoke-OpenUnpackTool npm @('--prefix', (Join-Path $taskRoot 'apps/desktop-ui'), 'ci')
}
if (-not $SkipFrontendBuild) {
    Invoke-OpenUnpackTool npm @('--prefix', (Join-Path $taskRoot 'apps/desktop-ui'), 'run', 'build')
}
Write-Output 'Windows 桌面构建资源已准备（纯 Rust 微内核单栈模式）。'
