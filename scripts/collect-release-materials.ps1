param([switch]$Offline)
. (Join-Path $PSScriptRoot 'common.ps1')
Assert-OpenUnpackWindows
$taskRoot = Get-OpenUnpackRoot
$taskOutput = Join-Path $taskRoot 'artifacts/release-materials'
New-OpenUnpackDirectory $taskOutput
Push-Location $taskRoot
try {
    $taskMetadata = Join-Path $taskRoot 'artifacts/release-cargo-metadata.json'
    $metaJson = & cargo metadata --locked --offline --format-version 1 --filter-platform x86_64-pc-windows-msvc
    [IO.File]::WriteAllText($taskMetadata, ($metaJson -join "`n"), [Text.UTF8Encoding]::new($false))
    if ($LASTEXITCODE -ne 0) { throw '许可证收集需要已获取的 Windows Cargo.lock 依赖。先运行 cargo fetch --locked --target x86_64-pc-windows-msvc。' }
    $taskArgs = @((Join-Path $PSScriptRoot 'collect-release-materials.mjs'), $taskRoot, $taskMetadata, $taskOutput)
    if ($Offline) { $taskArgs += '--offline' }
    Invoke-OpenUnpackTool node @taskArgs
    $auditArgs = @((Join-Path $PSScriptRoot 'audit-release-materials.mjs'), $taskOutput)
    Invoke-OpenUnpackTool node @auditArgs
    Write-OpenUnpackChecksums $taskOutput
} finally { Pop-Location }
