$ErrorActionPreference = 'Stop'
$taskCredentialDir = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.codex\gemini-subagent'
$taskSecureKey = Read-Host 'Gateway API key (hidden input)' -AsSecureString
try {
    New-Item -ItemType Directory -Path $taskCredentialDir -Force | Out-Null
    $taskSecureKey | ConvertFrom-SecureString | Set-Content -LiteralPath (Join-Path $taskCredentialDir 'gateway-key.dpapi') -Encoding utf8
    Write-Output 'API key saved with Windows DPAPI encryption.'
} finally { $taskSecureKey.Dispose() }
