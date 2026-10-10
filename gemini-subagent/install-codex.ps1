param([string]$Workspace = (Split-Path -Parent $PSScriptRoot))
$ErrorActionPreference = 'Stop'
$taskWorkspace = (Resolve-Path -LiteralPath $Workspace).Path
$taskCodexDir = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.codex'
$taskConfig = Join-Path $taskCodexDir 'config.toml'
$taskOriginal = if (Test-Path -LiteralPath $taskConfig) { [IO.File]::ReadAllText($taskConfig) } else { '' }
if ($taskOriginal -match '(?m)^\[mcp_servers\.(?:gemini_subagent|"gemini_subagent")\]') {
    throw 'gemini_subagent already exists; this script will not overwrite it.'
}
$taskBackupDir = Join-Path $taskCodexDir 'gemini-subagent'
New-Item -ItemType Directory -Path $taskBackupDir -Force | Out-Null
if (Test-Path -LiteralPath $taskConfig) {
    $taskBackup = Join-Path $taskBackupDir ('config-before-install-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.toml')
    Copy-Item -LiteralPath $taskConfig -Destination $taskBackup
}
$taskLauncher = Join-Path $PSScriptRoot 'run.ps1'
$taskPowerShell = (Get-Command powershell.exe -ErrorAction Stop).Source
$taskNodePath = (Get-Command node.exe -ErrorAction Stop).Source
$taskCommandToml = ConvertTo-Json -InputObject $taskPowerShell -Compress
$taskArgumentList = @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $taskLauncher, '-NodePath', $taskNodePath, '-Mode', 'mcp', '--workspace', $taskWorkspace)
$taskArgsToml = ConvertTo-Json -InputObject $taskArgumentList -Compress
$taskSection = "`n[mcp_servers.gemini_subagent]`ncommand = $taskCommandToml`nargs = $taskArgsToml`nstartup_timeout_sec = 20`ntool_timeout_sec = 300`n"
$taskCurrent = if (Test-Path -LiteralPath $taskConfig) { [IO.File]::ReadAllText($taskConfig) } else { '' }
if ($taskCurrent -cne $taskOriginal) { throw 'Codex configuration changed during installation; cancelled.' }
$taskUpdated = $taskOriginal + $taskSection
[IO.File]::WriteAllText($taskConfig, $taskUpdated, [Text.UTF8Encoding]::new($false))
Write-Output 'Gemini MCP registered (read-only). Reconnect MCP or start a new chat to use gemini_delegate.'
