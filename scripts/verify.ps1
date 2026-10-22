param([switch]$SkipInstall, [switch]$WorkerProtocol, [switch]$SkipPrepare, [switch]$SkipSecurityAudit)
. (Join-Path $PSScriptRoot 'common.ps1')
Assert-OpenUnpackWindows
Import-OpenUnpackMsvc
$taskRoot = Get-OpenUnpackRoot
$taskLogs = Join-Path $taskRoot 'artifacts/verification'
New-OpenUnpackDirectory $taskLogs
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
    Invoke-OpenUnpackTool dotnet @('run', '--project', 'tests/OpenUnpack.Tests/OpenUnpack.Tests.csproj', '-c', 'Release')
    Push-Location (Join-Path $taskRoot 'apps/desktop-ui')
    try {
        Invoke-OpenUnpackTool npm @('test')
    } finally {
        Pop-Location
    }
    Push-Location (Join-Path $taskRoot 'apps/mcp-connector')
    try {
        if (-not $SkipInstall) { Invoke-OpenUnpackTool npm @('ci') }
        Invoke-OpenUnpackTool npm @('test')
    } finally {
        Pop-Location
    }
    # The named-pipe integration suites require the real Worker and official engine prepared above.
    Invoke-OpenUnpackTool cargo @('test', '--workspace', '--locked', '--', '--test-threads=1')
    if ($WorkerProtocol) {
        & (Join-Path $taskRoot 'src/OpenUnpack.Worker/verify-worker.ps1')
        & (Join-Path $PSScriptRoot 'verify-worker-v2.ps1')
    }
    @{ status = 'passed'; at = [DateTimeOffset]::UtcNow.ToString('o'); scope = 'Synthetic archives, current-user DPAPI, native named pipes, frontend unit tests. No physical HDD/SSD or clean-machine installation acceptance.' } |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskLogs 'summary.json') -Encoding utf8NoBOM
} finally { Pop-Location }
