$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'runtime-policy.ps1')
$taskPolicy = Get-OpenUnpackRuntimePolicy
$taskRuntimeChecks = 0
function Assert-RuntimeRejected([scriptblock]$Action, [string]$Label) {
    $rejected = $false
    try { $null = & $Action } catch { $rejected = $true }
    if (-not $rejected) { throw "Runtime policy accepted $Label." }
    $script:taskRuntimeChecks++
}
function New-TestRelease([string]$Version, [bool]$Security) {
    return @{ 'release-version' = $Version; 'release-date' = '2026-10-13'; security = $Security; runtime = @{ version = $Version } }
}
function New-TestMetadata {
    return @{ 'channel-version' = '10.0'; 'latest-runtime' = '10.0.13'; 'support-phase' = 'active';
        releases = @((New-TestRelease '10.0.13' $true), (New-TestRelease '10.0.12' $false)) }
}
function Test-Metadata($Metadata, [string]$Runtime = '10.0.13') {
    Get-OpenUnpackRuntimeSecurityStatus -RuntimeVersion $Runtime -MetadataJson ($Metadata | ConvertTo-Json -Depth 12) -Policy $taskPolicy
}

Assert-OpenUnpackReviewedRuntime -RuntimeVersion $taskPolicy.defaultRuntimeVersion -Policy $taskPolicy
$taskRuntimeChecks++
foreach ($version in @('10.0.12', '10.0.14', '10.0.13-preview.1', '11.0.0', '10.0.013')) {
    Assert-RuntimeRejected { Assert-OpenUnpackReviewedRuntime -RuntimeVersion $version -Policy $taskPolicy } "unreviewed version $version"
}
$clean = Test-Metadata (New-TestMetadata)
if ($clean.status -ne 'passed' -or -not $clean.checked) { throw 'Current reviewed runtime did not pass.' }
$taskRuntimeChecks++
$old = Test-Metadata (New-TestMetadata) '10.0.12'
if ($old.status -ne 'failed' -or $old.laterSecurityReleases.Count -ne 1) { throw 'Old runtime was cleared by a clean package report.' }
$taskRuntimeChecks++
$later = New-TestMetadata
$later['latest-runtime'] = '10.0.14'
$later.releases = @((New-TestRelease '10.0.14' $false)) + $later.releases
if ((Test-Metadata $later).status -ne 'passed') { throw 'Non-security release was mislabeled as a missing security fix.' }
$taskRuntimeChecks++
$later.releases[0].security = $true
if ((Test-Metadata $later).status -ne 'failed') { throw 'Later security release did not block release.' }
$taskRuntimeChecks++
$preview = New-TestMetadata
$preview.releases = @((New-TestRelease '10.0.14-preview.1' $false)) + $preview.releases
if ((Test-Metadata $preview).status -ne 'passed') { throw 'Preview prevented validation of stable releases.' }
$taskRuntimeChecks++
foreach ($text in @('query unavailable', '{', '{}', 'null', '[]')) {
    Assert-RuntimeRejected { Get-OpenUnpackRuntimeSecurityStatus -RuntimeVersion '10.0.13' -MetadataJson $text } 'missing or malformed metadata'
}
foreach ($mutate in @(
    { param($m) $m['channel-version'] = '11.0' },
    { param($m) $m['support-phase'] = 'eol' },
    { param($m) $m['latest-runtime'] = '10.0.14' },
    { param($m) $m.releases = @() },
    { param($m) $m.releases = @($m.releases[0]) },
    { param($m) $m.releases[0].Remove('security') },
    { param($m) $m.releases[0].security = 'true' },
    { param($m) $m.releases[0]['release-date'] = '2026-02-30' },
    { param($m) $m.releases[0].runtime.version = 'unknown' },
    { param($m) $m.releases += $m.releases[0] }
)) {
    $malformed = New-TestMetadata
    $null = & $mutate $malformed
    # A missing installed version is checked with 10.0.12; the others use the reviewed version.
    $selected = if ($malformed.releases.Count -eq 1) { '10.0.12' } else { '10.0.13' }
    Assert-RuntimeRejected { Test-Metadata $malformed $selected } 'incomplete or contradictory official release data'
}

function New-TestIdentity {
    return @{
        config = @{ runtimeOptions = @{ tfm = 'net10.0'; includedFrameworks = @(@{ name = 'Microsoft.NETCore.App'; version = '10.0.13' }) } };
        deps = @{ runtimeTarget = @{ name = '.NETCoreApp,Version=v10.0/win-x64' }; targets = @{ '.NETCoreApp,Version=v10.0/win-x64' = @{ 'runtimepack.Microsoft.NETCore.App.Runtime.win-x64/10.0.13' = @{} } } };
        files = @{
            'System.Private.CoreLib.dll' = @{ ProductVersion = '10.0.13-servicing.26499.1+synthetic'; FileVersion = '10.0.1326.49901' };
            'coreclr.dll' = @{ ProductVersion = '10,0,1326,49901 @Commit: synthetic'; FileVersion = '10,0,1326,49901 @Commit: synthetic' };
            'hostfxr.dll' = @{ ProductVersion = '10.0.13 @Commit: synthetic'; FileVersion = '10,0,1326,49901 @Commit: synthetic' }
        }
    }
}
function Test-Identity($Identity) {
    Assert-OpenUnpackPublishedRuntimeIdentity -RuntimeConfigJson ($Identity.config | ConvertTo-Json -Depth 12) -DepsJson ($Identity.deps | ConvertTo-Json -Depth 12) -FileVersions $Identity.files -Policy $taskPolicy
}
if ((Test-Identity (New-TestIdentity)) -ne '10.0.13') { throw 'Consistent published identity failed.' }
$taskRuntimeChecks++
foreach ($mutate in @(
    { param($i) $i.config.runtimeOptions.includedFrameworks[0].version = '10.0.12' },
    { param($i) $i.config.runtimeOptions.includedFrameworks = @() },
    { param($i) $i.config.runtimeOptions.includedFrameworks += $i.config.runtimeOptions.includedFrameworks[0] },
    { param($i) $i.config.runtimeOptions.framework = @{ name = 'Microsoft.NETCore.App'; version = '10.0.13' } },
    { param($i) $i.deps.runtimeTarget.name = '.NETCoreApp,Version=v10.0/win-arm64' },
    { param($i) $i.deps.targets['.NETCoreApp,Version=v10.0/win-x64'] = @{ 'runtimepack.Microsoft.NETCore.App.Runtime.win-x64/10.0.12' = @{} } },
    { param($i) $i.files['System.Private.CoreLib.dll'].ProductVersion = '10.0.12-servicing.26422.108' },
    { param($i) $i.files['hostfxr.dll'].ProductVersion = '10.0.12 @Commit: old' },
    { param($i) $i.files['coreclr.dll'].FileVersion = '10,0,1226,42308 @Commit: old' },
    { param($i) $i.files['coreclr.dll'].FileVersion = '' }
)) {
    $mixed = New-TestIdentity
    $null = & $mutate $mixed
    Assert-RuntimeRejected { Test-Identity $mixed } 'mixed, missing or misleading published runtime identity'
}
Write-Output "PASS $taskRuntimeChecks runtime policy cases (reviewed versions, servicing metadata, self-contained identity and mixed binary rejection)."
