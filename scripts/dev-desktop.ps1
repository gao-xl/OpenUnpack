param([switch]$SkipPrepare)
. (Join-Path $PSScriptRoot 'common.ps1')
Assert-OpenUnpackWindows
Import-OpenUnpackMsvc
$taskRoot = Get-OpenUnpackRoot
if (-not $SkipPrepare) { & (Join-Path $PSScriptRoot 'prepare-desktop.ps1') }
$taskCli = Join-Path $taskRoot 'apps/desktop-ui/node_modules/.bin/tauri.cmd'
if (-not (Test-Path -LiteralPath $taskCli)) { throw '缺少 Tauri CLI，请先运行 prepare-desktop.ps1。' }
Push-Location (Join-Path $taskRoot 'apps/desktop-host')
try { Invoke-OpenUnpackTool $taskCli @('dev') }
finally { Pop-Location }
