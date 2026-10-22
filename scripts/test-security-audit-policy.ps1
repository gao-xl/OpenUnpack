$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'security-audit-policy.ps1')
$valid = '{"version":1,"projects":[{"path":"synthetic.csproj"}]}'
if (@(Get-OpenUnpackDotnetVulnerabilities -JsonText $valid).Count -ne 0) { throw 'Clean official output failed.' }
$vulnerable = '{"version":1,"projects":[{"path":"synthetic.csproj","frameworks":[{"framework":"net10.0","transitivePackages":[{"id":"synthetic","resolvedVersion":"1.0","vulnerabilities":[{"severity":"High","advisoryurl":"https://example.invalid/advisory"}]}]}]}]}'
if (@(Get-OpenUnpackDotnetVulnerabilities -JsonText $vulnerable).Count -ne 1) { throw 'Advisory disappeared.' }
foreach ($invalid in @('网络查询失败', '{', '{}', '{"version":2,"projects":[]}', '{"version":1,"projects":[]}', '{"version":1,"projects":[{}]}', '{"version":1,"projects":[{"path":"synthetic.csproj","warnings":["feed unavailable"]}]}', '{"version":1,"projects":[{"path":"synthetic.csproj","vulnerabilities":"unexpected"}]}')) {
    $rejected = $false
    try { $null = Get-OpenUnpackDotnetVulnerabilities -JsonText $invalid }
    catch { $rejected = $true }
    if (-not $rejected) { throw 'Invalid or incomplete scanner output passed.' }
}
Write-Output 'PASS 10 security-audit report parsing cases (clean, advisory, malformed, incomplete, unavailable feed).'
foreach ($invalid in @('{"version":1,"projects":[{"path":"p","frameworks":[{}]}]}', '{"version":1,"projects":[{"path":"p","frameworks":[null]}]}', '{"version":1,"projects":[{"path":"p","frameworks":[{"framework":"net10.0","topLevelPackages":"not-an-array"}]}]}', '{"version":1,"projects":[{"path":"p","frameworks":[{"framework":"net10.0","topLevelPackages":[{}]}]}]}', '{"version":1,"projects":[{"path":"p","frameworks":[{"framework":"net10.0","topLevelPackages":[{"id":"p","resolvedVersion":"1.0","vulnerabilities":[{}]}]}]}]}')) {
    $rejected = $false
    try { $null = Get-OpenUnpackDotnetVulnerabilities -JsonText $invalid }
    catch { $rejected = $true }
    if (-not $rejected) { throw 'Incomplete nested scanner schema passed.' }
}
Write-Output 'PASS 5 nested schema rejection cases.'

function New-TestCargoMetadata {
    param([switch]$Reachable, [switch]$Transitive, [string]$Version = '4.3.4', [switch]$SecondVersion)
    $dependencyId = "registry#zbus_names@$Version"
    $rootNode = @{ id = 'workspace#app'; deps = @() }
    $bridgeNode = @{ id = 'registry#bridge'; deps = @(@{ pkg = $dependencyId }) }
    if ($Reachable) { $rootNode.deps = @(@{ pkg = $(if ($Transitive) { 'registry#bridge' } else { $dependencyId }) }) }
    $packages = @(@{ id = 'workspace#app'; name = 'app'; version = '0.1.0' },
        @{ id = 'registry#bridge'; name = 'bridge'; version = '0.1.0' },
        @{ id = $dependencyId; name = 'zbus_names'; version = $Version })
    $nodes = @($rootNode, $bridgeNode, @{ id = $dependencyId; deps = @() })
    if ($SecondVersion) {
        $packages += @{ id = 'registry#zbus_names@99.0.0'; name = 'zbus_names'; version = '99.0.0' }
        $nodes += @{ id = 'registry#zbus_names@99.0.0'; deps = @() }
    }
    return @{ version = 1; workspace_members = @('workspace#app'); packages = $packages; resolve = @{ nodes = $nodes } }
}
function Assert-TestCargoExcluded {
    param($Metadata, [bool]$Expected)
    $result = Get-OpenUnpackWindowsAuditExceptions -MetadataJson ($Metadata | ConvertTo-Json -Depth 12)
    if (@($result.ignored_advisories | Where-Object id -eq 'RUSTSEC-2026-0371').Count -ne [int]$Expected) { throw 'Windows reachability exclusion is incorrect.' }
    if ($result.metadata_sha256 -notmatch '^[0-9a-f]{64}$') { throw 'Graph evidence hash is missing.' }
}
Assert-TestCargoExcluded (New-TestCargoMetadata) $true
Assert-TestCargoExcluded (New-TestCargoMetadata -Reachable) $false
Assert-TestCargoExcluded (New-TestCargoMetadata -Reachable -Transitive) $false
Assert-TestCargoExcluded (New-TestCargoMetadata -Version '99.0.0') $false
Assert-TestCargoExcluded (New-TestCargoMetadata -SecondVersion) $false
$cycle = New-TestCargoMetadata -Reachable -Transitive
$cycle.resolve.nodes[2].deps = @(@{ pkg = 'registry#bridge' })
Assert-TestCargoExcluded $cycle $false
$anotherRoot = New-TestCargoMetadata
$anotherRoot.workspace_members += 'registry#bridge'
Assert-TestCargoExcluded $anotherRoot $false
$invalidGraphs = @('invalid', '{}', '{"version":1,"packages":[],"workspace_members":[],"resolve":{"nodes":[]}}')
$unsupportedVersion = New-TestCargoMetadata
$unsupportedVersion.version = 2
$invalidGraphs += $unsupportedVersion | ConvertTo-Json -Depth 12
$missingNode = New-TestCargoMetadata
$missingNode.resolve.nodes = @($missingNode.resolve.nodes | Select-Object -First 2)
$invalidGraphs += $missingNode | ConvertTo-Json -Depth 12
$missingUnreferencedNode = New-TestCargoMetadata
$missingUnreferencedNode.resolve.nodes = @($missingUnreferencedNode.resolve.nodes | Where-Object id -ne 'registry#bridge')
$invalidGraphs += $missingUnreferencedNode | ConvertTo-Json -Depth 12
$badEdge = New-TestCargoMetadata
$badEdge.resolve.nodes[0].deps = @(@{ pkg = 'missing' })
$invalidGraphs += $badEdge | ConvertTo-Json -Depth 12
$duplicate = New-TestCargoMetadata
$duplicate.resolve.nodes += $duplicate.resolve.nodes[0]
$invalidGraphs += $duplicate | ConvertTo-Json -Depth 12
$missingRoot = New-TestCargoMetadata
$missingRoot.workspace_members = @('missing')
$invalidGraphs += $missingRoot | ConvertTo-Json -Depth 12
foreach ($invalidGraph in $invalidGraphs) {
    $rejected = $false
    try { $null = Get-OpenUnpackWindowsAuditExceptions -MetadataJson $invalidGraph }
    catch { $rejected = $true }
    if (-not $rejected) { throw 'Incomplete/ambiguous Cargo graph enabled an advisory exclusion.' }
}
Write-Output "PASS 7 Cargo reachability cases and $($invalidGraphs.Count) invalid graph rejection cases."
$fullGraph = New-TestCargoMetadata -Reachable -Transitive
$targetGraph = New-TestCargoMetadata
$targetGraph.packages = @($targetGraph.packages | Where-Object name -eq 'app')
$targetGraph.resolve.nodes = @($targetGraph.resolve.nodes | Where-Object id -eq 'workspace#app')
$targetJson = $targetGraph | ConvertTo-Json -Depth 12
$completeJson = $fullGraph | ConvertTo-Json -Depth 12
$targetResult = Get-OpenUnpackWindowsAuditExceptions -MetadataJson $targetJson -AllMetadataJson $completeJson
if (@($targetResult.ignored_advisories | Where-Object id -eq 'RUSTSEC-2026-0371').Count -ne 1 -or
    $targetResult.all_metadata_sha256 -notmatch '^[0-9a-f]{64}$') {
    throw 'Target-omitted package identity was not established from the complete graph.'
}
$reachableResult = Get-OpenUnpackWindowsAuditExceptions -MetadataJson $completeJson -AllMetadataJson $completeJson
if (@($reachableResult.ignored_advisories).Count -ne 0) { throw 'Reachable package was excluded.' }
foreach ($mismatch in @('identity', 'roots', 'edge', 'malformed')) {
    $badFull = New-TestCargoMetadata -Reachable -Transitive
    switch ($mismatch) {
        'identity' { $badFull.packages[0].version = '99.0.0' }
        'roots' { $badFull.workspace_members += 'registry#bridge' }
        'edge' { $badFull.resolve.nodes[0].deps = @() }
    }
    $badJson = if ($mismatch -eq 'malformed') { '{}' } else { $badFull | ConvertTo-Json -Depth 12 }
    $rejected = $false
    try { $null = Get-OpenUnpackWindowsAuditExceptions -MetadataJson $completeJson -AllMetadataJson $badJson }
    catch { $rejected = $true }
    if (-not $rejected) { throw "Mismatched complete graph passed: $mismatch" }
}
Write-Output 'PASS 6 complete/target Cargo graph cross-check cases.'
& (Join-Path $PSScriptRoot 'test-runtime-policy.ps1')
