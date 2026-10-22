param([string]$Version = '0.2.0')
. (Join-Path $PSScriptRoot 'common.ps1')
Assert-OpenUnpackWindows
if ($Version -notmatch '^\d+\.\d+\.\d+([-.][A-Za-z0-9.-]+)?$') { throw '版本格式无效。' }
$taskRoot = Get-OpenUnpackRoot
$taskRelease = Join-Path $taskRoot 'artifacts/release'
New-OpenUnpackDirectory $taskRelease
$taskStage = Join-Path $taskRelease ('OpenUnpack-' + $Version + '-source')
Reset-OpenUnpackBuildDirectory -Path $taskStage -Parent $taskRelease

# An explicit allowlist prevents local credentials, the Gemini gateway project, data,
# downloaded engines and build outputs from entering the application source release.
$taskFiles = @('Cargo.toml', 'Cargo.lock', 'Directory.Build.props', 'global.json', 'OpenUnpack.sln', 'LICENSE', '.gitignore', '.editorconfig', 'README.md', 'CONTRIBUTING.md', 'SECURITY.md', 'CHANGELOG.md', 'THIRD_PARTY_NOTICES.md')
$taskFiles += @('assets/legal/swagger-ui/LICENSE', 'assets/legal/swagger-ui/NOTICE', 'assets/legal/swagger-ui/provenance.json', 'examples/local-api.py', 'examples/local-api.ps1', 'apps/desktop-ui/eslint.config.js')
foreach ($relative in $taskFiles) {
    $source = Join-Path $taskRoot $relative
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw "源码发布缺少文件：$relative" }
    $taskDestination = Join-Path $taskStage $relative
    New-OpenUnpackDirectory ([IO.Path]::GetDirectoryName($taskDestination))
    Copy-Item -LiteralPath $source -Destination $taskDestination
}
$taskAllowedExtensions = @('.rs', '.toml', '.json', '.lock', '.cs', '.csproj', '.xaml', '.sln', '.props', '.targets', '.tsx', '.ts', '.css', '.html', '.md', '.ps1', '.mjs', '.yml', '.yaml', '.png', '.ico', '.icns', '.svg', '.txt', '.nsi', '.nsh')
$taskFixturePrefix = 'tests/fixtures/upstream-archives/'
$taskFixtureProvenance = Join-Path $taskRoot ($taskFixturePrefix + 'provenance.json')
$taskFixtureFiles = @{}
if (Test-Path -LiteralPath $taskFixtureProvenance -PathType Leaf) {
    $taskProvenance = Get-Content -Raw -LiteralPath $taskFixtureProvenance | ConvertFrom-Json
    if ($taskProvenance.repository -ne 'https://github.com/adamhathcock/sharpcompress' -or $taskProvenance.license -ne 'MIT') { throw '测试归档必须有已审查的 SharpCompress MIT 来源。' }
    foreach ($fixture in $taskProvenance.files) {
        if ([IO.Path]::GetFileName($fixture.file) -ne $fixture.file -or $fixture.sha256 -notmatch '^[0-9a-f]{64}$') { throw '测试归档清单包含越界或无效摘要。' }
        $taskFixturePath = Join-Path $taskRoot ($taskFixturePrefix + $fixture.file)
        if (-not (Test-Path -LiteralPath $taskFixturePath -PathType Leaf) -or (Get-FileHash -LiteralPath $taskFixturePath).Hash.ToLowerInvariant() -ne $fixture.sha256) { throw "测试归档与来源摘要不符：$($fixture.file)" }
        $taskFixtureFiles[$taskFixturePrefix + $fixture.file] = $true
    }
}
$taskSensitive = '(?i)(^|/)(node_modules|target(?:_[^/]+)?|bin|obj|dist|\.git|\.gemini-subagent-backups|gemini-subagent|artifacts|OpenUnpack-Archive|_workspace.*)(/|$)|(^|/)(\.env(?:\..*)?|config\.toml|gateway-key\.dpapi)$|\.(tsbuildinfo|vault(?:\.bak)?|sqlite(?:3)?(?:-wal|-shm)?|db|dpapi|pem|pfx|key)$'
foreach ($tree in @('apps', 'crates', 'src', 'tests', 'scripts', 'docs', '.github')) {
    $directory = Join-Path $taskRoot $tree
    if (-not (Test-Path -LiteralPath $directory)) { continue }
    foreach ($file in Get-ChildItem -LiteralPath $directory -File -Recurse -Force) {
        $relative = [IO.Path]::GetRelativePath($taskRoot, $file.FullName).Replace('\', '/')
        $taskSwaggerAsset = $relative -match '^crates/local-api/assets/swagger-ui/(swagger-ui-bundle|swagger-ui-standalone-preset)\.js$'
        if ($relative -match $taskSensitive -or ($file.Extension -notin $taskAllowedExtensions -and -not $taskFixtureFiles.ContainsKey($relative) -and -not $taskSwaggerAsset)) { continue }
        if ($file.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "源码发布拒绝链接：$relative" }
        $destination = Join-Path $taskStage $relative
        New-OpenUnpackDirectory ([IO.Path]::GetDirectoryName($destination))
        Copy-Item -LiteralPath $file.FullName -Destination $destination
    }
}
Invoke-OpenUnpackTool node @((Join-Path $PSScriptRoot 'audit-source.mjs'), $taskStage)
Write-OpenUnpackChecksums $taskStage
$taskZip = Join-Path $taskRelease ('OpenUnpack-' + $Version + '-source.zip')
if (Test-Path -LiteralPath $taskZip) { Remove-Item -LiteralPath (Assert-OpenUnpackOwnedPath -Path $taskZip -Parent $taskRelease) -Force }
Compress-Archive -LiteralPath $taskStage -DestinationPath $taskZip -CompressionLevel Optimal
Write-Output "源码发布包：$taskZip"
