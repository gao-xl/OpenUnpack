param(
    [ValidateRange(1, 65535)][int]$Port = 47980,
    [string]$Archive,
    [string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
if (-not $env:OPENUNPACK_API_TOKEN) { throw 'Set OPENUNPACK_API_TOKEN in this process before calling.' }
if ($Archive -and -not $OutputDirectory) { throw 'Archive requires OutputDirectory.' }

function Invoke-OpenUnpackCommand {
    param([string]$Name, [hashtable]$Arguments)
    $requestBody = @{ command = $Name }
    if ($null -ne $Arguments) { $requestBody.args = $Arguments }
    $parameters = @{
        Uri = "http://127.0.0.1:$Port/api/v1/commands"
        Method = 'Post'
        Headers = @{ Authorization = "Bearer $env:OPENUNPACK_API_TOKEN" }
        ContentType = 'application/json; charset=utf-8'
        Body = [Text.Encoding]::UTF8.GetBytes(($requestBody | ConvertTo-Json -Depth 30 -Compress))
        TimeoutSec = 120
    }
    if ($PSVersionTable.PSVersion.Major -ge 7) { $parameters.NoProxy = $true }
    (Invoke-RestMethod @parameters).data
}

if ($Archive) {
    $archivePath = (Resolve-Path -LiteralPath $Archive).Path
    $outputPath = (Resolve-Path -LiteralPath $OutputDirectory).Path
    $scan = Invoke-OpenUnpackCommand 'scan_sources' @{ paths = @($archivePath) }
    $settings = Invoke-OpenUnpackCommand 'read_settings'
    $settings.outputRoot = $outputPath
    $settings.sourceCleanup = 'keep'
    Invoke-OpenUnpackCommand 'add_jobs' @{ sources = @($scan.sources); options = $settings } | Out-Null
    Invoke-OpenUnpackCommand 'start_jobs' | Out-Null
}
Invoke-OpenUnpackCommand 'queue_snapshot' | ConvertTo-Json -Depth 30
