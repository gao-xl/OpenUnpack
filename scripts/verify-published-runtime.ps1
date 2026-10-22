param([Parameter(Mandatory)][string]$WorkerDirectory)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'runtime-policy.ps1')
Get-OpenUnpackPublishedRuntime -WorkerDirectory $WorkerDirectory | ConvertTo-Json -Depth 8
