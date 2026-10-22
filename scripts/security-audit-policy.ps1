# Scanner output is a trust boundary: malformed or incomplete reports never pass.
function Get-OpenUnpackCargoAuditGraph {
    param([Parameter(Mandatory)][string]$MetadataJson)
    try { $metadata = ConvertFrom-Json -InputObject $MetadataJson -ErrorAction Stop }
    catch { throw 'Cargo Windows metadata did not return valid JSON.' }
    if (-not $metadata -or -not $metadata.PSObject.Properties['version'] -or $metadata.version -ne 1 -or
        -not $metadata.PSObject.Properties['packages'] -or $metadata.packages -isnot [Array] -or $metadata.packages.Count -eq 0 -or
        -not $metadata.PSObject.Properties['workspace_members'] -or $metadata.workspace_members -isnot [Array] -or $metadata.workspace_members.Count -eq 0 -or
        -not $metadata.PSObject.Properties['resolve'] -or $metadata.resolve -isnot [pscustomobject] -or
        -not $metadata.resolve.PSObject.Properties['nodes'] -or $metadata.resolve.nodes -isnot [Array] -or $metadata.resolve.nodes.Count -eq 0) {
        throw 'Cargo Windows metadata is unsupported or incomplete; no advisory can be excluded.'
    }
    $packages = [Collections.Generic.Dictionary[string,object]]::new([StringComparer]::Ordinal)
    $nodes = [Collections.Generic.Dictionary[string,object]]::new([StringComparer]::Ordinal)
    foreach ($package in $metadata.packages) {
        if ($package -isnot [pscustomobject] -or -not $package.PSObject.Properties['id'] -or
            -not $package.PSObject.Properties['name'] -or -not $package.PSObject.Properties['version'] -or
            $package.id -isnot [string] -or [string]::IsNullOrWhiteSpace($package.id) -or
            $package.name -isnot [string] -or [string]::IsNullOrWhiteSpace($package.name) -or
            $package.version -isnot [string] -or [string]::IsNullOrWhiteSpace($package.version) -or
            $packages.ContainsKey($package.id)) { throw 'Invalid or duplicate Cargo package identity.' }
        $packages.Add($package.id, $package)
    }
    foreach ($node in $metadata.resolve.nodes) {
        if ($node -isnot [pscustomobject] -or -not $node.PSObject.Properties['id'] -or
            $node.id -isnot [string] -or -not $packages.ContainsKey($node.id) -or $nodes.ContainsKey($node.id) -or
            -not $node.PSObject.Properties['deps'] -or $node.deps -isnot [Array]) {
            throw 'Invalid, missing or duplicate Cargo resolve node.'
        }
        $nodes.Add($node.id, $node)
    }
    if ($nodes.Count -ne $packages.Count) { throw 'Cargo graph omits package nodes; reachability is not established.' }
    # Include all resolved normal, build and dev edges. Never infer absence from
    # a missing node, incomplete edge, empty workspace, or a package name alone.
    foreach ($node in $nodes.Values) {
        foreach ($dependency in $node.deps) {
            if ($dependency -isnot [pscustomobject] -or -not $dependency.PSObject.Properties['pkg'] -or
                $dependency.pkg -isnot [string] -or -not $nodes.ContainsKey($dependency.pkg)) {
                throw 'Cargo Windows graph contains an unresolved dependency edge.'
            }
        }
    }
    $reachable = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $pending = [Collections.Generic.Stack[string]]::new()
    foreach ($member in $metadata.workspace_members) {
        if ($member -isnot [string] -or -not $nodes.ContainsKey($member)) { throw 'Cargo workspace root is missing from the graph.' }
        $pending.Push($member)
    }
    while ($pending.Count -gt 0) {
        $id = $pending.Pop()
        if (-not $reachable.Add($id)) { continue }
        foreach ($dependency in $nodes[$id].deps) { $pending.Push($dependency.pkg) }
    }
    return @{ packages = $packages; nodes = $nodes; reachable = $reachable;
        roots = $metadata.workspace_members;
        sha256 = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($MetadataJson))).ToLowerInvariant() }
}

function Get-OpenUnpackWindowsAuditExceptions {
    param([Parameter(Mandatory)][string]$MetadataJson, [string]$AllMetadataJson = $MetadataJson)
    $windows = Get-OpenUnpackCargoAuditGraph -MetadataJson $MetadataJson
    $all = Get-OpenUnpackCargoAuditGraph -MetadataJson $AllMetadataJson
    # Cargo can omit non-target packages entirely from filtered metadata. Establish
    # their identities in the complete graph, then prove absence in the Windows graph.
    if (@(Compare-Object @($all.roots) @($windows.roots)).Count -ne 0) {
        throw 'Cargo graph workspace roots do not match.'
    }
    foreach ($package in $windows.packages.Values) {
        if (-not $all.packages.ContainsKey($package.id) -or
            $all.packages[$package.id].name -cne $package.name -or
            $all.packages[$package.id].version -cne $package.version) {
            throw 'Cargo target graph is not a subset of the complete package inventory.'
        }
        $allEdges = @($all.nodes[$package.id].deps | ForEach-Object { $_.pkg })
        foreach ($edge in $windows.nodes[$package.id].deps) {
            if ($edge.pkg -cnotin $allEdges) { throw 'Cargo target graph contains an inconsistent edge.' }
        }
    }
    # Reviewed advisories only. Unknown advisories and versions still reach the
    # strict scanner. Even one reachable instance keeps the advisory enabled.
    $reviewed = @(
        @{ id = 'RUSTSEC-2024-0370'; package = 'proc-macro-error'; version = '1.0.4' },
        @{ id = 'RUSTSEC-2024-0429'; package = 'glib'; version = '0.18.5' },
        @{ id = 'RUSTSEC-2026-0371'; package = 'zbus_names'; version = '4.3.4' },
        @{ id = 'RUSTSEC-2026-0370'; package = 'zvariant'; version = '5.15.0' },
        @{ id = 'RUSTSEC-2026-0372'; package = 'zvariant_derive'; version = '5.15.0' }
    )
    $ignored = @()
    foreach ($known in $reviewed) {
        $affected = @($all.packages.Values | Where-Object { $_.name -ceq $known.package })
        if ($affected.Count -gt 0 -and
            @($affected | Where-Object { $_.version -cne $known.version -or $windows.reachable.Contains($_.id) }).Count -eq 0) {
            $ignored += @{ id = $known.id; package = $known.package; version = $known.version;
                reason = 'all reviewed package instances unreachable in x86_64-pc-windows-msvc workspace graph' }
        }
    }
    return @{ target = 'x86_64-pc-windows-msvc'; ignored_advisories = $ignored;
        metadata_sha256 = $windows.sha256; all_metadata_sha256 = $all.sha256;
        reachable_package_count = $windows.reachable.Count; graph_node_count = $windows.nodes.Count;
        all_graph_node_count = $all.nodes.Count }
}

function Get-OpenUnpackDotnetVulnerabilities {
    param([Parameter(Mandatory)][string]$JsonText)
    try { $report = ConvertFrom-Json -InputObject $JsonText -ErrorAction Stop }
    catch { throw 'NuGet advisory query did not return valid JSON.' }
    if (-not $report -or $report.version -ne 1 -or
        -not $report.PSObject.Properties['projects'] -or
        $report.projects -isnot [Array] -or $report.projects.Count -eq 0) {
        throw 'NuGet advisory query returned an unsupported or incomplete report.'
    }
    $found = [Collections.Generic.List[object]]::new()
    $pending = [Collections.Generic.Queue[object]]::new()
    $pending.Enqueue($report)
    while ($pending.Count -gt 0) {
        $node = $pending.Dequeue()
        if ($node -is [Array]) {
            foreach ($child in $node) { if ($null -ne $child) { $pending.Enqueue($child) } }
            continue
        }
        if ($node -isnot [pscustomobject]) { continue }
        foreach ($property in $node.PSObject.Properties) {
            if ($property.Name -in @('errors', 'error', 'warnings', 'warning') -and $property.Value) {
                throw 'NuGet advisory query reported an error or warning; coverage is not confirmed.'
            }
            if ($property.Name -eq 'vulnerabilities') {
                if ($property.Value -isnot [Array]) { throw 'Invalid NuGet vulnerability list.' }
                foreach ($advisory in $property.Value) { $found.Add($advisory) }
            }
            if ($property.Name -eq 'level' -and $property.Value -in @('error', 'warning', 'Error', 'Warning')) {
                throw 'NuGet advisory query reported an error or warning.'
            }
            if ($null -ne $property.Value) { $pending.Enqueue($property.Value) }
        }
    }
    foreach ($project in $report.projects) {
        if ($project -isnot [pscustomobject] -or -not $project.PSObject.Properties['path'] -or
            [string]::IsNullOrWhiteSpace($project.path)) { throw 'Invalid NuGet project result.' }
        # The official CLI omits frameworks for projects without vulnerable packages.
        if ($project.PSObject.Properties['frameworks'] -and $project.frameworks -isnot [Array]) {
            throw 'Invalid NuGet framework result.'
        }
        if (-not $project.PSObject.Properties['frameworks']) { continue }
        foreach ($framework in $project.frameworks) {
            if ($framework -isnot [pscustomobject] -or -not $framework.PSObject.Properties['framework'] -or
                [string]::IsNullOrWhiteSpace($framework.framework)) { throw 'Incomplete NuGet framework result.' }
            foreach ($collectionName in @('topLevelPackages', 'transitivePackages')) {
                if (-not $framework.PSObject.Properties[$collectionName]) { continue }
                $packages = $framework.$collectionName
                if ($packages -isnot [Array]) { throw 'Invalid NuGet package collection.' }
                foreach ($package in $packages) {
                    if ($package -isnot [pscustomobject] -or -not $package.PSObject.Properties['id'] -or
                        [string]::IsNullOrWhiteSpace($package.id) -or -not $package.PSObject.Properties['resolvedVersion'] -or
                        [string]::IsNullOrWhiteSpace($package.resolvedVersion)) { throw 'Incomplete NuGet package result.' }
                    if (-not $package.PSObject.Properties['vulnerabilities']) { continue }
                    if ($package.vulnerabilities -isnot [Array]) { throw 'Invalid NuGet advisory collection.' }
                    foreach ($advisory in $package.vulnerabilities) {
                        if ($advisory -isnot [pscustomobject] -or -not $advisory.PSObject.Properties['severity'] -or
                            $advisory.severity -notin @('Low', 'Moderate', 'High', 'Critical') -or
                            -not $advisory.PSObject.Properties['advisoryurl'] -or
                            [string]::IsNullOrWhiteSpace($advisory.advisoryurl)) { throw 'Incomplete NuGet advisory result.' }
                    }
                }
            }
        }
    }
    return $found.ToArray()
}
