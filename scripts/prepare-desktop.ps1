param(
    [switch]$SkipInstall,
    [string]$RuntimeFrameworkVersion,
    [switch]$SkipFrontendBuild
)
. (Join-Path $PSScriptRoot 'common.ps1')
. (Join-Path $PSScriptRoot 'runtime-policy.ps1')
Assert-OpenUnpackWindows
Import-OpenUnpackMsvc
$taskRoot = Get-OpenUnpackRoot
$taskRuntimePolicy = Get-OpenUnpackRuntimePolicy
if (-not $RuntimeFrameworkVersion) { $RuntimeFrameworkVersion = $taskRuntimePolicy.defaultRuntimeVersion }
Assert-OpenUnpackReviewedRuntime -RuntimeVersion $RuntimeFrameworkVersion -Policy $taskRuntimePolicy
if (-not (Test-Path -LiteralPath (Join-Path $taskRoot 'tools/7zip/7z.dll'))) {
    & (Join-Path $PSScriptRoot 'setup-engine.ps1')
}
$workerOut = Join-Path $taskRoot 'artifacts/worker'
Reset-OpenUnpackBuildDirectory -Path $workerOut -Parent (Join-Path $taskRoot 'artifacts')
Invoke-OpenUnpackTool dotnet @('publish', (Join-Path $taskRoot 'src/OpenUnpack.Worker/OpenUnpack.Worker.csproj'), '-c', 'Release', '-r', 'win-x64', '--self-contained', 'true', "-p:RuntimeFrameworkVersion=$RuntimeFrameworkVersion", '-o', $workerOut)
$taskPublishedRuntime = Get-OpenUnpackPublishedRuntime -WorkerDirectory $workerOut -Policy $taskRuntimePolicy
if ($taskPublishedRuntime.version -cne $RuntimeFrameworkVersion) { throw 'Published Worker runtime does not match the requested reviewed version.' }
Copy-Item -LiteralPath (Join-Path $taskRoot 'tools/7zip/7z.dll') -Destination $workerOut
Copy-Item -LiteralPath (Join-Path $taskRoot 'tools/7zip/License.txt') -Destination (Join-Path $workerOut '7zip-License.txt')
Copy-Item -LiteralPath (Join-Path $taskRoot 'tools/7zip/provenance.json') -Destination (Join-Path $workerOut '7zip-provenance.json')
Copy-Item -LiteralPath (Join-Path $taskRoot 'THIRD_PARTY_NOTICES.md') -Destination $workerOut
if (-not $SkipInstall) {
    Invoke-OpenUnpackTool npm @('--prefix', (Join-Path $taskRoot 'apps/desktop-ui'), 'ci')
}
if (-not $SkipFrontendBuild) {
    Invoke-OpenUnpackTool npm @('--prefix', (Join-Path $taskRoot 'apps/desktop-ui'), 'run', 'build')
}
Write-Output 'Windows 桌面构建资源已准备。'
