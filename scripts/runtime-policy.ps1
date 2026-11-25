function ConvertFrom-OpenUnpackJsonHashtable {
    param([Parameter(Mandatory)][string]$Json)
    if ($PSVersionTable.PSVersion.Major -ge 6) {
        return ConvertFrom-Json -InputObject $Json -AsHashtable -ErrorAction Stop
    }
    function Convert-Node {
        param($node)
        if ($node -is [Array]) {
            $list = [Collections.Generic.List[object]]::new()
            foreach ($item in $node) {
                $list.Add((Convert-Node $item))
            }
            return ,$list.ToArray()
        } elseif ($node -is [System.Management.Automation.PSCustomObject]) {
            $hash = [Collections.Hashtable]::new([StringComparer]::Ordinal)
            foreach ($prop in $node.PSObject.Properties) {
                $hash[$prop.Name] = Convert-Node $prop.Value
            }
            return $hash
        } else {
            return $node
        }
    }
    $obj = ConvertFrom-Json -InputObject $Json -ErrorAction Stop
    return (Convert-Node $obj)
}

# Shared reviewed versions and notice hashes are data, not independently maintained allowlists.
function Get-OpenUnpackRuntimePolicy {
    $policy = ConvertFrom-OpenUnpackJsonHashtable (Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'runtime-policy.json'))
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
    $config = ConvertFrom-OpenUnpackJsonHashtable $RuntimeConfigJson
    $deps = ConvertFrom-OpenUnpackJsonHashtable $DepsJson
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
    try { $metadata = ConvertFrom-OpenUnpackJsonHashtable $MetadataJson }
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
        source = $Policy.releaseMetadataUrl; metadataSha256 = ([BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash([Text.Encoding]::UTF8.GetBytes($MetadataJson))).Replace('-', '').ToLowerInvariant());
        checkedAtUtc = [DateTimeOffset]::UtcNow.ToString('o'); scope = 'Runtime servicing baseline; release-level security flags are not application exploitability findings.' }
}

function Get-OpenUnpackNativeEngineIdentity {
    param([Parameter(Mandatory)][string]$EngineDirectory, [string]$PortableRoot = '')
    $engineDir = [IO.Path]::GetFullPath($EngineDirectory)
    $provenancePath = Join-Path $engineDir 'provenance.json'
    if (-not (Test-Path -LiteralPath $provenancePath -PathType Leaf)) {
        throw "7-Zip provenance manifest missing: $provenancePath"
    }
    $provenance = Get-Content -Raw -LiteralPath $provenancePath | ConvertFrom-Json -ErrorAction Stop
    $checks = @(
        @{ Name = '7z.exe'; Expected = $provenance.exeSha256 },
        @{ Name = '7z.dll'; Expected = $provenance.dllSha256 },
        @{ Name = 'License.txt'; Expected = $provenance.licenseSha256 }
    )
    $files = [ordered]@{}
    foreach ($item in $checks) {
        $filePath = Join-Path $engineDir $item.Name
        if (-not (Test-Path -LiteralPath $filePath -PathType Leaf)) {
            throw "Pure-Rust 7-Zip payload file missing: $filePath"
        }
        $actual = (Get-FileHash -LiteralPath $filePath -Algorithm SHA256).Hash.ToLowerInvariant()
        if (-not $item.Expected -or $actual -ne ([string]$item.Expected).ToLowerInvariant()) {
            throw "7-Zip SHA-256 mismatch for $($item.Name): expected $($item.Expected), got $actual"
        }
        $files[$item.Name] = @{ sha256 = $actual }
    }
    if ($PortableRoot) {
        $root = [IO.Path]::GetFullPath($PortableRoot)
        foreach ($retired in @('OpenUnpack.Worker.exe', 'OpenUnpack.Worker.runtimeconfig.json', 'coreclr.dll')) {
            if (Test-Path -LiteralPath (Join-Path $root $retired)) {
                throw "Retired .NET runtime file unexpectedly present in pure-Rust payload: $retired"
            }
        }
    }
    return @{
        status = 'passed'
        mode = 'pure-rust-microkernel'
        sevenZipVersion = $provenance.version
        engineDirectory = $engineDir
        files = $files
        checkedAtUtc = [DateTimeOffset]::UtcNow.ToString('o')
    }
}
