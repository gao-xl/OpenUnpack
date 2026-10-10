param([switch]$SkipInstall)
. (Join-Path $PSScriptRoot 'common.ps1')
Assert-OpenUnpackWindows
Import-OpenUnpackMsvc
$taskRoot = Get-OpenUnpackRoot
if (-not (Test-Path -LiteralPath (Join-Path $taskRoot 'tools/7zip/7z.dll'))) {
    & (Join-Path $PSScriptRoot 'setup-engine.ps1')
}
$workerOut = Join-Path $taskRoot 'artifacts/worker'
Reset-OpenUnpackBuildDirectory -Path $workerOut -Parent (Join-Path $taskRoot 'artifacts')
Invoke-OpenUnpackTool dotnet @('publish', (Join-Path $taskRoot 'src/OpenUnpack.Worker/OpenUnpack.Worker.csproj'), '-c', 'Release', '-r', 'win-x64', '--self-contained', 'true', '-p:RuntimeFrameworkVersion=10.0.9', '-o', $workerOut)
Copy-Item -LiteralPath (Join-Path $taskRoot 'tools/7zip/7z.dll') -Destination $workerOut
Copy-Item -LiteralPath (Join-Path $taskRoot 'tools/7zip/License.txt') -Destination (Join-Path $workerOut '7zip-License.txt')
Copy-Item -LiteralPath (Join-Path $taskRoot 'tools/7zip/provenance.json') -Destination (Join-Path $workerOut '7zip-provenance.json')
Copy-Item -LiteralPath (Join-Path $taskRoot 'THIRD_PARTY_NOTICES.md') -Destination $workerOut
if (-not $SkipInstall) {
    Invoke-OpenUnpackTool npm @('--prefix', (Join-Path $taskRoot 'apps/desktop-ui'), 'ci')
}
Invoke-OpenUnpackTool npm @('--prefix', (Join-Path $taskRoot 'apps/desktop-ui'), 'run', 'build')
Write-Output 'Windows 桌面构建资源已准备。'
