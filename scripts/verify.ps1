param([switch]$SkipInstall, [switch]$WorkerProtocol, [switch]$SkipPrepare)
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
    & (Join-Path $PSScriptRoot 'collect-release-materials.ps1')
    Invoke-OpenUnpackTool dotnet @('build', 'src/OpenUnpack.Worker/OpenUnpack.Worker.csproj', '-c', 'Release', '--no-restore')
    Invoke-OpenUnpackTool dotnet @('run', '--project', 'tests/OpenUnpack.Tests/OpenUnpack.Tests.csproj', '-c', 'Release')
    Invoke-OpenUnpackTool npm @('--prefix', 'apps/desktop-ui', 'test')
    # The named-pipe integration suites require the real Worker and official engine prepared above.
    Invoke-OpenUnpackTool cargo @('test', '--workspace', '--locked', '--', '--test-threads=1')
    if ($WorkerProtocol) { & (Join-Path $taskRoot 'src/OpenUnpack.Worker/verify-worker.ps1') }
    @{ status = 'passed'; at = [DateTimeOffset]::UtcNow.ToString('o'); scope = 'Synthetic archives, current-user DPAPI, native named pipes, frontend unit tests. No physical HDD/SSD or clean-machine installation acceptance.' } |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskLogs 'summary.json') -Encoding utf8NoBOM
} finally { Pop-Location }
