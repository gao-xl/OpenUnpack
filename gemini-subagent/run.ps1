[CmdletBinding(PositionalBinding = $false)]
param(
    [ValidateSet('cli', 'mcp', 'live')][string]$Mode = 'cli',
    [string]$NodePath = 'node',
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$AgentArgs
)
$ErrorActionPreference = 'Stop'
$taskCredentialPath = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.codex\gemini-subagent\gateway-key.dpapi'
$taskPreviousKey = $env:GEMINI_API_KEY
$taskExitCode = 1
try {
    if ($NodePath -eq 'node') {
        if ($env:CODEX_MCP_NODE_PATH -and (Test-Path -LiteralPath $env:CODEX_MCP_NODE_PATH)) {
            $NodePath = $env:CODEX_MCP_NODE_PATH
        } elseif (Test-Path -LiteralPath 'C:\Program Files\nodejs\node.exe') {
            $NodePath = 'C:\Program Files\nodejs\node.exe'
        }
    }
    if (-not $env:GEMINI_API_KEY) {
        Import-Module -Name (Join-Path $PSHOME 'Modules\Microsoft.PowerShell.Security\Microsoft.PowerShell.Security.psd1') -ErrorAction Stop
        if (-not (Test-Path -LiteralPath $taskCredentialPath)) { throw 'Missing API key. Run set-credential.ps1 first.' }
        $taskEncryptedKey = (Get-Content -Raw -LiteralPath $taskCredentialPath).Trim()
        $taskSecureKey = ConvertTo-SecureString -String $taskEncryptedKey
        $taskPointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($taskSecureKey)
        try { $env:GEMINI_API_KEY = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($taskPointer) }
        finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($taskPointer) }
    }
    $taskEntry = switch ($Mode) {
        'cli' { Join-Path $PSScriptRoot 'src\cli.mjs' }
        'mcp' { Join-Path $PSScriptRoot 'src\mcp.mjs' }
        'live' { Join-Path $PSScriptRoot 'scripts\live-smoke.mjs' }
    }
    & $NodePath $taskEntry @AgentArgs
    $taskExitCode = $LASTEXITCODE
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
} finally {
    $env:GEMINI_API_KEY = $taskPreviousKey
    if ($taskSecureKey) { $taskSecureKey.Dispose() }
}
exit $taskExitCode
