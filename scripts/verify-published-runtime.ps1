param([string]$ReleaseDirectory, [string]$WorkerDirectory)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'runtime-policy.ps1')

if ($WorkerDirectory -and (Test-Path -LiteralPath (Join-Path $WorkerDirectory 'OpenUnpack.Worker.runtimeconfig.json') -PathType Leaf)) {
    Get-OpenUnpackPublishedRuntime -WorkerDirectory $WorkerDirectory | ConvertTo-Json -Depth 8
    return
}

$targetRoot = if ($ReleaseDirectory) {
    $ReleaseDirectory
} elseif ($WorkerDirectory) {
    $WorkerDirectory
} else {
    Join-Path $PSScriptRoot '../artifacts/OpenUnpack-portable'
}
$targetRoot = [IO.Path]::GetFullPath($targetRoot)
$engineDir = if (Test-Path -LiteralPath (Join-Path $targetRoot 'tools/7zip/provenance.json') -PathType Leaf) {
    Join-Path $targetRoot 'tools/7zip'
} else {
    $targetRoot
}
Get-OpenUnpackNativeEngineIdentity -EngineDirectory $engineDir -PortableRoot $targetRoot | ConvertTo-Json -Depth 8
