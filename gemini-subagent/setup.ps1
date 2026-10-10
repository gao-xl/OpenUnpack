[CmdletBinding(PositionalBinding = $false)]
param([switch]$VerifyLive)
$ErrorActionPreference = 'Stop'
$taskNodePath = (Get-Command node.exe -ErrorAction Stop).Source
$taskCodexPath = (Get-Command codex.exe -ErrorAction Stop).Source
Push-Location $PSScriptRoot
try {
    if (-not (Test-Path -LiteralPath (Join-Path $PSScriptRoot 'node_modules\@modelcontextprotocol\sdk\package.json'))) {
        & npm ci --ignore-scripts
        if ($LASTEXITCODE -ne 0) { throw 'Dependency installation failed.' }
    }
    $taskPreparedJson = & $taskNodePath (Join-Path $PSScriptRoot 'scripts\prepare-plugin.mjs')
    if ($LASTEXITCODE -ne 0) { throw 'Plugin preparation failed.' }
    $taskPrepared = $taskPreparedJson | ConvertFrom-Json
    $taskListJson = & $taskCodexPath plugin list --marketplace $taskPrepared.marketplaceName --json
    if ($LASTEXITCODE -ne 0) { throw 'Failed to read installed plugin state.' }
    $taskPluginState = ($taskListJson | ConvertFrom-Json).installed | Where-Object pluginId -EQ $taskPrepared.pluginId | Select-Object -First 1
    if ($taskPluginState -and $taskPluginState.version -eq $taskPrepared.version) {
        if (-not $taskPluginState.enabled) { throw 'Plugin is installed but disabled. Enable it in Codex plugin settings.' }
        $taskInstalledPath = Join-Path ([Environment]::GetFolderPath('UserProfile')) ('.codex\plugins\cache\' + $taskPrepared.marketplaceName + '\gemini-subagent\' + $taskPrepared.version)
    } else {
        $taskInstallJson = & $taskCodexPath plugin add $taskPrepared.pluginId --json
        if ($LASTEXITCODE -ne 0) { throw 'Codex plugin installation failed; previous plugin cache is preserved.' }
        $taskInstalledPath = ($taskInstallJson | ConvertFrom-Json).installedPath
    }
    $taskCheckArgs = @((Join-Path $PSScriptRoot 'scripts\check-plugin.mjs'), '--root', $taskInstalledPath, '--compare-source', $taskPrepared.pluginSource)
    if ($VerifyLive) { $taskCheckArgs += '--live' }
    & $taskNodePath @taskCheckArgs
    if ($LASTEXITCODE -ne 0) { throw 'Installed plugin verification failed.' }
    Write-Output ('Ready: ' + $taskPrepared.pluginId + ' v' + $taskPrepared.version)
} finally { Pop-Location }
