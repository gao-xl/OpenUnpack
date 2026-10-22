param([switch]$SkipInstall, [switch]$WorkerProtocol, [switch]$SkipPrepare, [switch]$SkipSecurityAudit, [switch]$SkipMcpTest)
. (Join-Path $PSScriptRoot 'common.ps1')
Assert-OpenUnpackWindows
Import-OpenUnpackMsvc
$taskRoot = Get-OpenUnpackRoot
$taskLogs = Join-Path $taskRoot 'artifacts/verification'
New-OpenUnpackDirectory $taskLogs
$taskPreviousWorker = [Environment]::GetEnvironmentVariable('OPENUNPACK_TEST_WORKER', 'Process')
$taskPreviousEngine = [Environment]::GetEnvironmentVariable('OPENUNPACK_TEST_ENGINE', 'Process')
Push-Location $taskRoot
try {
    if (-not $SkipPrepare) { & (Join-Path $PSScriptRoot 'prepare-desktop.ps1') -SkipInstall:$SkipInstall }
    Invoke-OpenUnpackTool cargo @('fetch', '--locked', '--target', 'x86_64-pc-windows-msvc')
    & (Join-Path $PSScriptRoot 'check-contracts.ps1') -RunCargoTest
    & (Join-Path $PSScriptRoot 'test-release-validation.ps1')
    & (Join-Path $PSScriptRoot 'collect-release-materials.ps1')
    if (-not $SkipSecurityAudit) {
        & (Join-Path $PSScriptRoot 'audit-security.ps1') -Offline:$SkipInstall -DenyWarnings
    }
    Invoke-OpenUnpackTool dotnet @('build', 'src/OpenUnpack.Worker/OpenUnpack.Worker.csproj', '-c', 'Release', '--no-restore')
    # Every native suite must use the prepared candidate, including suites
    # which deliberately require an explicit Worker instead of a dev fallback.
    # Preserve explicit caller overrides and restore the process environment.
    $taskTestWorker = if ([string]::IsNullOrWhiteSpace($taskPreviousWorker)) { Join-Path $taskRoot 'artifacts/worker/OpenUnpack.Worker.exe' } else { $taskPreviousWorker }
    $taskTestEngine = if ([string]::IsNullOrWhiteSpace($taskPreviousEngine)) { Join-Path $taskRoot 'tools/7zip/7z.dll' } else { $taskPreviousEngine }
    foreach ($taskPayload in @($taskTestWorker, $taskTestEngine)) {
        if (-not (Test-Path -LiteralPath $taskPayload -PathType Leaf)) { throw "Native test payload missing: $taskPayload. Run scripts/prepare-desktop.ps1 first or set the explicit test payload override." }
    }
    $env:OPENUNPACK_TEST_WORKER = [IO.Path]::GetFullPath($taskTestWorker)
    $env:OPENUNPACK_TEST_ENGINE = [IO.Path]::GetFullPath($taskTestEngine)
    Invoke-OpenUnpackTool dotnet @('run', '--project', 'tests/OpenUnpack.Tests/OpenUnpack.Tests.csproj', '-c', 'Release')
    Push-Location (Join-Path $taskRoot 'apps/desktop-ui')
    try {
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
    # The named-pipe integration suites require the real Worker and official engine prepared above.
    Invoke-OpenUnpackTool cargo @('test', '--workspace', '--locked', '--no-fail-fast', '--', '--test-threads=1')
    if ($WorkerProtocol) {
        & (Join-Path $taskRoot 'src/OpenUnpack.Worker/verify-worker.ps1')
        & (Join-Path $PSScriptRoot 'verify-worker-v2.ps1')
    }
    @{ status = 'passed'; at = [DateTimeOffset]::UtcNow.ToString('o'); scope = 'Synthetic archives, current-user DPAPI, native named pipes, frontend unit tests. No physical HDD/SSD or clean-machine installation acceptance.' } |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskLogs 'summary.json') -Encoding utf8NoBOM
} finally {
    [Environment]::SetEnvironmentVariable('OPENUNPACK_TEST_WORKER', $taskPreviousWorker, 'Process')
    [Environment]::SetEnvironmentVariable('OPENUNPACK_TEST_ENGINE', $taskPreviousEngine, 'Process')
    Pop-Location
}
