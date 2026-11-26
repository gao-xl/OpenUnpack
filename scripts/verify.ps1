param([switch]$SkipInstall, [switch]$WorkerProtocol, [switch]$SkipPrepare, [switch]$SkipSecurityAudit, [switch]$SkipMcpTest)
. (Join-Path $PSScriptRoot 'common.ps1')
. (Join-Path $PSScriptRoot 'runtime-policy.ps1')
Assert-OpenUnpackWindows
Import-OpenUnpackMsvc
$taskRoot = Get-OpenUnpackRoot
$taskBuildTmp = Join-Path $taskRoot 'artifacts/build-tmp'
New-OpenUnpackDirectory $taskBuildTmp
$env:TEMP = $taskBuildTmp
$env:TMP = $taskBuildTmp
$taskLogs = Join-Path $taskRoot 'artifacts/verification'
New-OpenUnpackDirectory $taskLogs
$taskPreviousWorker = [Environment]::GetEnvironmentVariable('OPENUNPACK_TEST_WORKER', 'Process')
$taskPreviousEngine = [Environment]::GetEnvironmentVariable('OPENUNPACK_TEST_ENGINE', 'Process')
$taskPreviousCli = [Environment]::GetEnvironmentVariable('OPENUNPACK_7Z_PATH', 'Process')
Push-Location $taskRoot
try {
    $null = Get-OpenUnpackNativeEngineIdentity -EngineDirectory (Join-Path $taskRoot 'tools/7zip')
    if (-not $SkipPrepare) { & (Join-Path $PSScriptRoot 'prepare-desktop.ps1') -SkipInstall:$SkipInstall }
    if (-not $SkipInstall) {
        Invoke-OpenUnpackTool cargo @('fetch', '--locked', '--target', 'x86_64-pc-windows-msvc')
    }
    & (Join-Path $PSScriptRoot 'check-contracts.ps1') -RunCargoTest
    Invoke-OpenUnpackTool node @((Join-Path $PSScriptRoot 'check-code-standards.mjs'))
    & (Join-Path $PSScriptRoot 'test-release-validation.ps1')
    & (Join-Path $PSScriptRoot 'collect-release-materials.ps1') -Offline:$SkipInstall
    if (-not $SkipSecurityAudit) {
        & (Join-Path $PSScriptRoot 'audit-security.ps1') -Offline:$SkipInstall -DenyWarnings
    }
    if ($WorkerProtocol) {
        Invoke-OpenUnpackTool cargo @('test', '-p', 'openunpack-infrastructure', '--locked', '--', '--test-threads=1')
    }
    Push-Location (Join-Path $taskRoot 'apps/desktop-ui')
    try {
        Invoke-OpenUnpackTool npm @('run', 'format:check')
        Invoke-OpenUnpackTool npm @('run', 'lint')
        Invoke-OpenUnpackTool npm @('test')
    } finally {
        Pop-Location
    }
    # Connector compilation and real stdio tests are release gates as well.
    # The explicit skip remains available for narrowly scoped local diagnosis.
    if (-not $SkipMcpTest) {
        Push-Location (Join-Path $taskRoot 'apps/mcp-connector')
        try {
            if (-not $SkipInstall) { Invoke-OpenUnpackTool npm @('ci') }
            Invoke-OpenUnpackTool npm @('test')
        } finally {
            Pop-Location
        }
    }
    # Single-stack native Rust integration suites.
    Invoke-OpenUnpackTool cargo @('test', '--workspace', '--locked', '--no-fail-fast', '--', '--test-threads=1')
    $verifySummary = @{ status = 'passed'; at = [DateTimeOffset]::UtcNow.ToString('o'); scope = 'Synthetic archives, current-user DPAPI, native Rust microkernel, frontend unit tests. No physical HDD/SSD or clean-machine installation acceptance.' }
    Write-OpenUnpackUtf8NoBom -Path (Join-Path $taskLogs 'summary.json') -Content (($verifySummary | ConvertTo-Json) + "`n")
} finally {
    [Environment]::SetEnvironmentVariable('OPENUNPACK_TEST_WORKER', $taskPreviousWorker, 'Process')
    [Environment]::SetEnvironmentVariable('OPENUNPACK_TEST_ENGINE', $taskPreviousEngine, 'Process')
    [Environment]::SetEnvironmentVariable('OPENUNPACK_7Z_PATH', $taskPreviousCli, 'Process')
    Pop-Location
}
