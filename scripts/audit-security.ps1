param(
    [switch]$Offline,
    [switch]$DenyWarnings,
    [switch]$InstallCargoAudit,
    [string]$WorkerDirectory
)
. (Join-Path $PSScriptRoot 'common.ps1')
. (Join-Path $PSScriptRoot 'security-audit-policy.ps1')
. (Join-Path $PSScriptRoot 'runtime-policy.ps1')
$taskRoot = Get-OpenUnpackRoot
$taskLogs = Join-Path $taskRoot 'artifacts/verification'
New-OpenUnpackDirectory $taskLogs
Push-Location $taskRoot
$dotnetVulnStatus = 'not_checked'
$taskRuntimeStatus = @{ status = 'not_checked'; checked = $false }
$taskRuntimeIdentity = @{ status = 'not_checked' }

try {
    Write-Host '=== Published .NET runtime identity and servicing baseline ===' -ForegroundColor Cyan
    if (-not $WorkerDirectory) { $WorkerDirectory = Join-Path $taskRoot 'artifacts/worker' }
    $taskRuntimePolicy = Get-OpenUnpackRuntimePolicy
    $taskRuntimeIdentity = @{ status = 'failed'; workerDirectory = $WorkerDirectory; reason = 'Published runtime identity validation did not complete.' }
    $taskRuntimeIdentity = Get-OpenUnpackPublishedRuntime -WorkerDirectory $WorkerDirectory -Policy $taskRuntimePolicy
    $taskRuntimeIdentity | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $taskLogs 'runtime-identity.json') -Encoding utf8NoBOM
    if ($Offline) {
        $taskRuntimeStatus = @{ status = 'skipped'; checked = $false; version = $taskRuntimeIdentity.version;
            reason = 'Offline: local reviewed identity passed; current official servicing metadata was not queried.' }
    } else {
        $taskRuntimeStatus = @{ status = 'failed'; checked = $false; version = $taskRuntimeIdentity.version;
            source = $taskRuntimePolicy.releaseMetadataUrl; reason = 'Official runtime metadata query or validation did not complete.' }
        $taskRuntimeMetadata = (Invoke-WebRequest -Uri $taskRuntimePolicy.releaseMetadataUrl -TimeoutSec 60 -ErrorAction Stop).Content
        if ($taskRuntimeMetadata -is [byte[]]) { $taskRuntimeMetadata = [Text.Encoding]::UTF8.GetString($taskRuntimeMetadata) }
        $taskRuntimeMetadata | Set-Content -LiteralPath (Join-Path $taskLogs 'dotnet-runtime-releases.json') -Encoding utf8NoBOM -NoNewline
        $taskRuntimeStatus = Get-OpenUnpackRuntimeSecurityStatus -RuntimeVersion $taskRuntimeIdentity.version -MetadataJson $taskRuntimeMetadata -Policy $taskRuntimePolicy
        if ($taskRuntimeStatus.status -ne 'passed') { throw 'Published runtime is behind a later official security release; review and republish before release.' }
    }

    Write-Host '=== .NET Roslyn SAST & NuGet package audit (separate from the runtime) ===' -ForegroundColor Cyan
    
    # 1. Roslyn SAST Security Analyzer Audit:
    # Builds with EnableNETAnalyzers=true, AnalysisModeSecurity=All, and TreatWarningsAsErrors=true.
    Invoke-OpenUnpackTool dotnet @(
        'build', 'src/OpenUnpack.Worker/OpenUnpack.Worker.csproj',
        '-c', 'Release',
        '--no-restore',
        '-p:UseSharedCompilation=false',
        '-p:EnableNETAnalyzers=true',
        '-p:TreatWarningsAsErrors=true',
        '-p:AnalysisLevelSecurity=recommended',
        '-p:AnalysisModeSecurity=All'
    )
    Write-Host "-> DotNetCliSAST Security Analyzer pass: 0 violations (CA21xx/CA30xx/CA53xx clean)." -ForegroundColor Green

    # NuGet reports describe packages, never the self-contained runtime's servicing status.
    $dotnetVulnStatus = 'skipped'
    $taskProjectReports = @()
    if (-not $Offline) {
        $dotnetVulnStatus = 'failed'
        Write-Host '-> Scanning all .NET projects for known package advisories...' -ForegroundColor DarkGray
        # ProtocolChecks is not in the solution, so discover projects beneath both source roots.
        $taskProjects = @(Get-ChildItem -LiteralPath (Join-Path $taskRoot 'src'), (Join-Path $taskRoot 'tests') -Recurse -Filter '*.csproj' -File | Sort-Object FullName)
        if ($taskProjects.Count -eq 0) { throw 'No .NET projects were discovered for the package audit.' }
        foreach ($taskProject in $taskProjects) {
            $taskPackageRaw = @(& (Get-Command dotnet).Source list $taskProject.FullName package --vulnerable --include-transitive --format json 2>&1)
            $taskPackageExit = $LASTEXITCODE
            $taskPackageText = $taskPackageRaw -join "`n"
            if ($taskPackageExit -ne 0) { throw "NuGet advisory query failed for $($taskProject.Name) (exit $taskPackageExit)." }
            $taskAdvisories = @(Get-OpenUnpackDotnetVulnerabilities -JsonText $taskPackageText)
            $taskProjectReports += @{ project = [IO.Path]::GetRelativePath($taskRoot, $taskProject.FullName); status = $(if ($taskAdvisories.Count) { 'failed' } else { 'passed' }); vulnerabilities = $taskAdvisories.Count }
            if ($taskAdvisories.Count -gt 0) { throw "Vulnerable NuGet packages detected in $($taskProject.Name)." }
        }
        $dotnetVulnStatus = 'passed'
        Write-Host '-> NuGet package advisory queries passed for all discovered projects.' -ForegroundColor Green
    } else {
        Write-Host '-> NuGet advisory queries skipped (-Offline specified).' -ForegroundColor DarkGray
    }
    Write-Host "`n=== [2/2] Rust Dependency & Known Vulnerabilities Audit (cargo-audit) ===" -ForegroundColor Cyan
    
    $cargoAuditCmd = Get-Command cargo-audit -CommandType Application -ErrorAction SilentlyContinue
    if (-not $cargoAuditCmd) {
        $auditSubcmd = @(& (Get-Command cargo).Source audit --version 2>&1)
        if ($LASTEXITCODE -eq 0) {
            $cargoAuditCmd = 'cargo'
        }
    }

    if (-not $cargoAuditCmd -and $InstallCargoAudit) {
        Write-Host "Installing cargo-audit..." -ForegroundColor Yellow
        Invoke-OpenUnpackTool cargo @('install', 'cargo-audit', '--locked')
        $cargoAuditCmd = 'cargo'
    }

    if (-not $cargoAuditCmd) {
        throw "cargo-audit is not installed. Please run 'cargo install cargo-audit --locked' or specify -InstallCargoAudit."
    }

    $auditArgs = [System.Collections.Generic.List[string]]::new()
    $auditArgs.Add('audit')
    if ($Offline) {
        $auditArgs.Add('-n')
    }
    if ($DenyWarnings) {
        $auditArgs.Add('--deny')
        $auditArgs.Add('warnings')
    }
    # Suppression is allowed only after checking the actual Windows dependency graph.
    $taskMetadataRaw = @(& cargo metadata --format-version 1 --locked --offline --filter-platform x86_64-pc-windows-msvc 2> (Join-Path $taskLogs 'cargo-windows-metadata.stderr.log'))
    if ($LASTEXITCODE -ne 0) { throw 'Could not establish the Windows dependency graph.' }
    $taskMetadataJson = $taskMetadataRaw -join "`n"
    $taskAllMetadataArgs = @('metadata', '--format-version', '1', '--locked')
    if ($Offline) { $taskAllMetadataArgs += '--offline' }
    $taskAllMetadataRaw = @(& cargo @taskAllMetadataArgs 2> (Join-Path $taskLogs 'cargo-all-metadata.stderr.log'))
    if ($LASTEXITCODE -ne 0) { throw 'Could not establish the complete Cargo package inventory.' }
    $taskAllMetadataJson = $taskAllMetadataRaw -join "`n"
    $taskAuditGraph = Get-OpenUnpackWindowsAuditExceptions -MetadataJson $taskMetadataJson -AllMetadataJson $taskAllMetadataJson
    $taskIgnoredAdvisories = @($taskAuditGraph.ignored_advisories)
    $taskMetadataJson | Set-Content -LiteralPath (Join-Path $taskLogs 'cargo-windows-metadata.json') -Encoding utf8NoBOM -NoNewline
    $taskAllMetadataJson | Set-Content -LiteralPath (Join-Path $taskLogs 'cargo-all-metadata.json') -Encoding utf8NoBOM -NoNewline
    $taskAuditGraph | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $taskLogs 'cargo-windows-audit-scope.json') -Encoding utf8NoBOM
    foreach ($taskKnown in $taskIgnoredAdvisories) {
        $auditArgs.Add('--ignore'); $auditArgs.Add($taskKnown.id)
    }

    Invoke-OpenUnpackTool cargo $auditArgs.ToArray()
    Write-Host "-> cargo-audit pass: 0 vulnerabilities found." -ForegroundColor Green

    $taskNpmStatus = 'skipped'
    if (-not $Offline) {
        $taskNpmRaw = @(& npm --prefix (Join-Path $taskRoot 'apps/desktop-ui') audit --json 2>&1)
        $taskNpmExit = $LASTEXITCODE
        $taskNpmReport = ($taskNpmRaw -join "`n") | ConvertFrom-Json -ErrorAction Stop
        if ($taskNpmExit -ne 0 -or -not $taskNpmReport.PSObject.Properties['metadata'] -or
            -not $taskNpmReport.metadata.PSObject.Properties['vulnerabilities'] -or
            $taskNpmReport.metadata.vulnerabilities.total -ne 0) {
            throw 'npm advisory query failed, returned an incomplete report, or found vulnerabilities.'
        }
        $taskNpmStatus = 'passed'
    }

    $overallStatus = if ($dotnetVulnStatus -eq 'skipped' -or $taskRuntimeStatus.status -eq 'skipped' -or $taskNpmStatus -eq 'skipped') { 'passed_with_skipped' } else { 'passed' }
    $auditSummary = @{
        status = $overallStatus
        at = [DateTimeOffset]::UtcNow.ToString('o')
        dotnet_sast = @{
            analyzers = 'Roslyn Security Analyzers (CA21xx, CA30xx, CA53xx)'
            status = 'passed'
            violations = 0
        }
        dotnet_dependencies = @{
            status = $dotnetVulnStatus
            checked = (-not $Offline)
            projects = $(if ($Offline) { @() } else { $taskProjectReports })
        }
        dotnet_runtime_identity = $taskRuntimeIdentity
        dotnet_runtime_servicing = $taskRuntimeStatus
        cargo_audit = @{
            status = 'passed'
            vulnerabilities = 0
            advisories_checked = $true
            offline = [bool]$Offline
            ignored_advisories = $taskIgnoredAdvisories
            target_graph = $taskAuditGraph
        }
        npm_dependencies = @{ status = $taskNpmStatus; checked = (-not $Offline) }
    }
    $auditSummary | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $taskLogs 'security-audit.json') -Encoding utf8NoBOM

    Write-Host "`n=== Security Audit Complete: Status=$overallStatus. ===" -ForegroundColor Green
} catch {
    @{ status = 'failed'; at = [DateTimeOffset]::UtcNow.ToString('o'); failure_reason = $_.Exception.Message;
        dotnet_dependencies = @{ status = $dotnetVulnStatus }; dotnet_runtime_identity = $taskRuntimeIdentity; dotnet_runtime_servicing = $taskRuntimeStatus } |
        ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $taskLogs 'security-audit.json') -Encoding utf8NoBOM
    throw
} finally {
    Pop-Location
}
