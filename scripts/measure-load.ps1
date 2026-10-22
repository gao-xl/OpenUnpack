param(
    [Parameter(Mandatory)][string]$Worker,
    [ValidateSet('ssd','hdd','cpu','controls','snapshots','all')][string]$Profile = 'ssd',
    [ValidateRange(1,5)][int[]]$Levels = @(1,2,3,5),
    [ValidateRange(16,128)][int]$MiB = 64,
    [string]$FixtureParent = [IO.Path]::GetTempPath(),
    [string]$HddParent,
    [string]$ReportRoot,
    [string]$TestExecutable,
    [switch]$DebugBuild
)
. (Join-Path $PSScriptRoot 'common.ps1')
Assert-OpenUnpackWindows
$taskRoot = Get-OpenUnpackRoot
$taskWorker = (Resolve-Path -LiteralPath $Worker).ProviderPath
$taskParent = (Resolve-Path -LiteralPath $FixtureParent).ProviderPath
if (-not (Test-Path -LiteralPath $taskWorker -PathType Leaf)) { throw 'Select an existing frozen Worker executable.' }
if (-not (Test-Path -LiteralPath $taskParent -PathType Container)) { throw 'FixtureParent must already exist; it is never deleted.' }
if ($Profile -in @('hdd','all') -and [string]::IsNullOrWhiteSpace($HddParent)) { throw 'Select an existing HDD parent explicitly for hdd/all.' }
if ($HddParent) { $HddParent = (Resolve-Path -LiteralPath $HddParent).ProviderPath }
if (-not $ReportRoot) { $ReportRoot = Join-Path $taskRoot '_workspace/load-optimization-2026-10-15' }
$taskReport = [IO.Path]::GetFullPath($ReportRoot)
[IO.Directory]::CreateDirectory($taskReport) | Out-Null
$taskRun = Join-Path $taskReport ('run-' + [Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($taskRun) | Out-Null
$taskNames = @('OPENUNPACK_TEST_WORKER','OPENUNPACK_LOAD_LEVELS','OPENUNPACK_LOAD_MIB','OPENUNPACK_LOAD_ROOT','OPENUNPACK_LOAD_HDD_ROOT','OPENUNPACK_LOAD_REPORT_ROOT')
$taskPrevious = @{}
foreach ($taskName in $taskNames) { $taskPrevious[$taskName] = [Environment]::GetEnvironmentVariable($taskName, 'Process') }
Push-Location $taskRoot
try {
    $env:OPENUNPACK_TEST_WORKER = $taskWorker
    $env:OPENUNPACK_LOAD_LEVELS = $Levels -join ','
    $env:OPENUNPACK_LOAD_MIB = [string]$MiB
    $env:OPENUNPACK_LOAD_ROOT = $taskParent
    $env:OPENUNPACK_LOAD_HDD_ROOT = $HddParent
    $env:OPENUNPACK_LOAD_REPORT_ROOT = $taskRun
    if (-not $TestExecutable) {
        Import-OpenUnpackMsvc
        # Complete compilation before launching the measured process. The Rust
        # fixture separately excludes archive creation and digest checks.
        $taskBuildArguments = @('test','-p','openunpack-core','--test','load_profile','--no-run','--locked','--offline','--message-format=json')
        if (-not $DebugBuild) { $taskBuildArguments += '--release' }
        $taskBuild = @(& cargo @taskBuildArguments 2> (Join-Path $taskRun 'build-stderr.log'))
        $taskBuildExit = $LASTEXITCODE
        $taskBuild | Set-Content -LiteralPath (Join-Path $taskRun 'build.jsonl') -Encoding utf8NoBOM
        if ($taskBuildExit -ne 0) { throw "Load fixture build failed; see $taskRun" }
        $taskArtifacts = @($taskBuild | ForEach-Object {
            try { $taskMessage = $_ | ConvertFrom-Json -ErrorAction Stop } catch { return }
            if ($taskMessage.reason -eq 'compiler-artifact' -and $taskMessage.target.name -eq 'load_profile' -and $taskMessage.executable) { $taskMessage.executable }
        })
        if ($taskArtifacts.Count -ne 1) { throw 'Expected exactly one compiled load_profile test executable.' }
        $TestExecutable = $taskArtifacts[0]
    }
    $taskExecutable = (Resolve-Path -LiteralPath $TestExecutable).ProviderPath
    @{
        startedAt = [DateTimeOffset]::UtcNow.ToString('o')
        profile = $Profile; levels = $Levels; tasksPerLevel = 10; payloadMiBPerTask = $MiB
        fixtureParent = $taskParent; hddParent = $HddParent
        worker = $taskWorker; workerExeSha256 = (Get-FileHash -LiteralPath $taskWorker -Algorithm SHA256).Hash
        benchmark = $taskExecutable; benchmarkSha256 = (Get-FileHash -LiteralPath $taskExecutable -Algorithm SHA256).Hash
        requestedBuildConfiguration = $(if ($DebugBuild) { 'debug' } else { 'release' })
        buildConfigurationEvidence = 'Runtime hostDebugAssertions is recorded in each result; supplied executables are not rebuilt.'
        logicalProcessors = [Environment]::ProcessorCount
        sampling = 'Native counters inside the measured process, 100 ms; direct-child PID plus creation identity.'
        constraints = 'Run alone without builds. No system cache flush or power changes. Native GUI is outside this measurement.'
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $taskRun 'invocation.json') -Encoding utf8NoBOM
    $taskTests = switch ($Profile) {
        'ssd' { @('synthetic_load_matrix') }
        'hdd' { @('synthetic_hdd_cache_matrix') }
        'cpu' { @('synthetic_cpu_format_case') }
        'controls' { @('synthetic_pause_and_cancel') }
        'snapshots' { @('synthetic_snapshot_history') }
        'all' { @('synthetic_load_matrix','synthetic_hdd_cache_matrix','synthetic_cpu_format_case','synthetic_pause_and_cancel','synthetic_snapshot_history') }
    }
    # libtest returns success when an --exact filter matches zero tests.
    # Verify names against this exact executable before accepting any run.
    $taskListedTests = @(& $taskExecutable --list 2>&1)
    if ($LASTEXITCODE -ne 0) { throw 'Could not list tests in the selected load executable.' }
    foreach ($taskTest in $taskTests) {
        if (@($taskListedTests | Where-Object { [string]$_ -ceq ($taskTest + ': test') }).Count -ne 1) {
            throw "Selected executable does not contain exactly one $taskTest test; rebuild the fixture before measuring."
        }
    }
    foreach ($taskTest in $taskTests) {
        & $taskExecutable --exact $taskTest --ignored --nocapture --test-threads=1 2>&1 |
            Tee-Object -LiteralPath (Join-Path $taskRun ($taskTest + '.log'))
        if ($LASTEXITCODE -ne 0) { throw "Load measurement failed ($taskTest); synthetic fixture is retained for inspection. See $taskRun" }
    }
    Write-Host "Load measurement evidence: $taskRun"
} finally {
    foreach ($taskName in $taskNames) { [Environment]::SetEnvironmentVariable($taskName, $taskPrevious[$taskName], 'Process') }
    Pop-Location
}
