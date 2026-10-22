# Shared reviewed versions and notice hashes are data, not independently maintained allowlists.
function Get-OpenUnpackRuntimePolicy {
    $policy = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'runtime-policy.json') | ConvertFrom-Json -AsHashtable -ErrorAction Stop
    if ($policy.schemaVersion -ne 1 -or $policy.channelVersion -ne '10.0' -or
        $policy.releaseMetadataUrl -ne 'https://builds.dotnet.microsoft.com/dotnet/release-metadata/10.0/releases.json' -or
        $policy.reviewedRuntimes -isnot [Array] -or $policy.reviewedRuntimes.Count -eq 0 -or
        $policy.minimumRuntimeVersion -notmatch '^10\.0\.(0|[1-9]\d*)$') { throw 'Invalid reviewed runtime policy.' }
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($entry in $policy.reviewedRuntimes) {
        if ($entry.version -notmatch '^10\.0\.(0|[1-9]\d*)$' -or
            [version]$entry.version -lt [version]$policy.minimumRuntimeVersion -or
            -not $seen.Add($entry.version) -or $entry.licenseSha256 -notmatch '^[a-f0-9]{64}$' -or
            $entry.thirdPartyNoticesSha256 -notmatch '^[a-f0-9]{64}$') { throw 'Invalid reviewed runtime entry.' }
    }
    if (-not $seen.Contains($policy.defaultRuntimeVersion)) { throw 'Default runtime has not been reviewed.' }
    return $policy
}

function Assert-OpenUnpackReviewedRuntime {
    param([Parameter(Mandatory)][string]$RuntimeVersion, $Policy = (Get-OpenUnpackRuntimePolicy))
    if ($RuntimeVersion -notmatch '^10\.0\.(0|[1-9]\d*)$' -or
        [version]$RuntimeVersion -lt [version]$Policy.minimumRuntimeVersion -or
        @($Policy.reviewedRuntimes | Where-Object version -CEQ $RuntimeVersion).Count -ne 1) {
        throw "Runtime $RuntimeVersion is not an approved security baseline; review the version and notices in scripts/runtime-policy.json together."
    }
}

function Assert-OpenUnpackPublishedRuntimeIdentity {
    param([Parameter(Mandatory)][string]$RuntimeConfigJson, [Parameter(Mandatory)][string]$DepsJson,
        [Parameter(Mandatory)][Collections.IDictionary]$FileVersions, $Policy = (Get-OpenUnpackRuntimePolicy))
    $config = ConvertFrom-Json -InputObject $RuntimeConfigJson -AsHashtable -ErrorAction Stop
    $deps = ConvertFrom-Json -InputObject $DepsJson -AsHashtable -ErrorAction Stop
    $options = $config.runtimeOptions
    if ($options -isnot [Collections.IDictionary] -or $options.tfm -ne 'net10.0' -or
        $options.includedFrameworks -isnot [Array] -or $options.includedFrameworks.Count -ne 1 -or
        $options.includedFrameworks[0].name -ne 'Microsoft.NETCore.App' -or $options['framework'] -or $options['frameworks']) {
        throw 'Worker must contain exactly one self-contained Microsoft.NETCore.App runtime.'
    }
    $runtime = $options.includedFrameworks[0].version
    Assert-OpenUnpackReviewedRuntime -RuntimeVersion $runtime -Policy $Policy
    $target = '.NETCoreApp,Version=v10.0/win-x64'
    $runtimePack = "runtimepack.Microsoft.NETCore.App.Runtime.win-x64/$runtime"
    if ($deps.runtimeTarget.name -ne $target -or $deps.targets -isnot [Collections.IDictionary] -or
        $deps.targets[$target] -isnot [Collections.IDictionary] -or
        @($deps.targets[$target].Keys | Where-Object { $_ -like 'runtimepack.Microsoft.NETCore.App.Runtime.*' }).Count -ne 1 -or
        -not $deps.targets[$target].Contains($runtimePack)) { throw 'Worker deps runtime pack or architecture differs from runtimeconfig.' }
    $escaped = [regex]::Escape($runtime)
    foreach ($file in @('System.Private.CoreLib.dll', 'hostfxr.dll')) {
        if ($FileVersions[$file].ProductVersion -notmatch "^$escaped(?:[-+ ]|$)") { throw "Published $file product version differs from runtimeconfig." }
    }
    # Native coreclr uses an encoded servicing FileVersion, rather than 10.0.<patch>.
    # Compare that build identity to CoreLib; parsing its first three numbers as a patch would be wrong.
    $builds = foreach ($file in @('System.Private.CoreLib.dll', 'coreclr.dll')) {
        $match = [regex]::Match([string]$FileVersions[$file].FileVersion, '^(\d+)[.,](\d+)[.,](\d+)[.,](\d+)(?:\s|$)')
        if (-not $match.Success) { throw "Published $file has no recognizable file build version." }
        ($match.Groups[1..4].Value -join '.')
    }
    if ($builds[0] -cne $builds[1]) { throw 'CoreCLR and CoreLib come from different runtime builds.' }
    return $runtime
}

function Get-OpenUnpackPublishedRuntime {
    param([Parameter(Mandatory)][string]$WorkerDirectory, $Policy = (Get-OpenUnpackRuntimePolicy))
    $directory = [IO.Path]::GetFullPath($WorkerDirectory)
    $files = [ordered]@{}
    $versions = @{}
    foreach ($file in @('OpenUnpack.Worker.runtimeconfig.json', 'OpenUnpack.Worker.deps.json', 'System.Private.CoreLib.dll', 'coreclr.dll', 'hostfxr.dll')) {
        $filePath = Join-Path $directory $file
        if (-not (Test-Path -LiteralPath $filePath -PathType Leaf)) { throw "Published runtime component missing: $file" }
        $files[$file] = @{ sha256 = (Get-FileHash -LiteralPath $filePath -Algorithm SHA256).Hash.ToLowerInvariant() }
        if ($file.EndsWith('.dll')) {
            $info = [Diagnostics.FileVersionInfo]::GetVersionInfo($filePath)
            $versions[$file] = @{ ProductVersion = $info.ProductVersion; FileVersion = $info.FileVersion }
            $files[$file].productVersion = $info.ProductVersion
            $files[$file].fileVersion = $info.FileVersion
        }
    }
    $runtime = Assert-OpenUnpackPublishedRuntimeIdentity -RuntimeConfigJson (Get-Content -Raw -LiteralPath (Join-Path $directory 'OpenUnpack.Worker.runtimeconfig.json')) -DepsJson (Get-Content -Raw -LiteralPath (Join-Path $directory 'OpenUnpack.Worker.deps.json')) -FileVersions $versions -Policy $Policy
    return @{ status = 'passed'; version = $runtime; workerDirectory = $directory; files = $files; checkedAtUtc = [DateTimeOffset]::UtcNow.ToString('o') }
}

function Get-OpenUnpackRuntimeSecurityStatus {
    param([Parameter(Mandatory)][string]$RuntimeVersion, [Parameter(Mandatory)][string]$MetadataJson,
        $Policy = (Get-OpenUnpackRuntimePolicy))
    if ($RuntimeVersion -notmatch '^10\.0\.(0|[1-9]\d*)$') { throw 'Unknown published runtime version.' }
    try { $metadata = ConvertFrom-Json -InputObject $MetadataJson -AsHashtable -ErrorAction Stop }
    catch { throw 'Official runtime metadata is not valid JSON.' }
    if ($metadata -isnot [Collections.IDictionary] -or $metadata['channel-version'] -ne $Policy.channelVersion -or
        $metadata['latest-runtime'] -notmatch '^10\.0\.(0|[1-9]\d*)$' -or
        $metadata['support-phase'] -notin @('active', 'maintenance') -or
        $metadata.releases -isnot [Array] -or $metadata.releases.Count -eq 0) { throw 'Official runtime metadata is incomplete, unsupported or out of support.' }
    $releases = [Collections.Generic.List[object]]::new()
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($release in $metadata.releases) {
        if ($release -isnot [Collections.IDictionary] -or $release.security -isnot [bool] -or
            $release['release-date'] -notmatch '^\d{4}-\d{2}-\d{2}$') { throw 'Invalid official runtime release record.' }
        $date = [DateTime]::MinValue
        if (-not [DateTime]::TryParseExact($release['release-date'], 'yyyy-MM-dd', [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::None, [ref]$date)) { throw 'Invalid official runtime release date.' }
        $version = $release.runtime.version
        if ($version -match '^10\.0\.(0|[1-9]\d*)-(preview|rc)\.') { continue }
        if ($version -notmatch '^10\.0\.(0|[1-9]\d*)$' -or -not $seen.Add($version) -or
            $release['release-version'] -ne $version) { throw 'Unknown or duplicate official runtime release version.' }
        $releases.Add(@{ version = $version; date = $release['release-date']; security = $release.security })
    }
    if (-not $seen.Contains($RuntimeVersion) -or $releases.Count -eq 0 -or
        ($releases | Sort-Object { [version]$_.version } -Descending | Select-Object -First 1).version -ne $metadata['latest-runtime']) {
        throw 'Published runtime or latest-runtime cannot be established from official releases.'
    }
    $updates = @($releases | Where-Object { $_.security -and [version]$_.version -gt [version]$RuntimeVersion } | Sort-Object { [version]$_.version })
    return @{ status = $(if ($updates.Count -eq 0) { 'passed' } else { 'failed' }); checked = $true;
        version = $RuntimeVersion; latestRuntime = $metadata['latest-runtime']; laterSecurityReleases = $updates;
        source = $Policy.releaseMetadataUrl; metadataSha256 = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($MetadataJson))).ToLowerInvariant();
        checkedAtUtc = [DateTimeOffset]::UtcNow.ToString('o'); scope = 'Runtime servicing baseline; release-level security flags are not application exploitability findings.' }
}
