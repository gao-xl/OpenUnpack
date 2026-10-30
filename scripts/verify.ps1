param([switch]$SkipInstall, [switch]$WorkerProtocol, [switch]$SkipPrepare, [switch]$SkipSecurityAudit, [switch]$SkipMcpTest)
. (Join-Path $PSScriptRoot 'common.ps1')
Assert-OpenUnpackWindows
Import-OpenUnpackMsvc
$taskRoot = Get-OpenUnpackRoot
# 7-Zip 26.x loads its codec DLL through an ANSI path while packing fixtures, so
# a checkout under a non-ASCII directory fails with "Codec Load Error: ... 80070005"
# and "Unsupported archive type" while CI (ASCII path) stays green. Warn loudly
# instead of letting the failure look like a product defect.
if ($taskRoot -match '[^\x00-\x7F]') {
    Write-Warning "Non-ASCII checkout path: $taskRoot. The bundled 7-Zip fixture packer may fail to load 7z.dll from this path; prefer an ASCII path or point OPENUNPACK_TEST_ENGINE / OPENUNPACK_7Z_PATH at ASCII copies."
}
$taskLogs = Join-Path $taskRoot 'artifacts/verification'
New-OpenUnpackDirectory $taskLogs
$taskPreviousWorker = [Environment]::GetEnvironmentVariable('OPENUNPACK_TEST_WORKER', 'Process')
$taskPreviousEngine = [Environment]::GetEnvironmentVariable('OPENUNPACK_TEST_ENGINE', 'Process')
$taskPreviousCli = [Environment]::GetEnvironmentVariable('OPENUNPACK_7Z_PATH', 'Process')
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
    # Single-stack native Rust integration suites.
    Invoke-OpenUnpackTool cargo @('test', '--workspace', '--locked', '--no-fail-fast', '--', '--test-threads=1')
    @{ status = 'passed'; at = [DateTimeOffset]::UtcNow.ToString('o'); scope = 'Synthetic archives, current-user DPAPI, native Rust microkernel, frontend unit tests. No physical HDD/SSD or clean-machine installation acceptance.' } |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskLogs 'summary.json') -Encoding utf8NoBOM
} finally {
    [Environment]::SetEnvironmentVariable('OPENUNPACK_TEST_WORKER', $taskPreviousWorker, 'Process')
    [Environment]::SetEnvironmentVariable('OPENUNPACK_TEST_ENGINE', $taskPreviousEngine, 'Process')
    [Environment]::SetEnvironmentVariable('OPENUNPACK_7Z_PATH', $taskPreviousCli, 'Process')
    Pop-Location
}
