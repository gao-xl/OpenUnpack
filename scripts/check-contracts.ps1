# OpenUnpack 契约与跨语言类型一致性检查
param([switch]$RunCargoTest)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Write-Host "==> Checking contracts against cross-language targets..."

# 1. 验证 protocol 库定义
$protocolDesktop = Join-Path $repoRoot 'crates/protocol/src/desktop.rs'
$protocolWorker = Join-Path $repoRoot 'crates/protocol/src/worker.rs'
$uiTypes = Join-Path $repoRoot 'apps/desktop-ui/src/types.ts'
$csharpProtocol = Join-Path $repoRoot 'src/OpenUnpack.Worker/Protocol.cs'

if (-not (Test-Path $protocolDesktop) -or -not (Test-Path $protocolWorker)) {
    throw "Missing crates/protocol DTO files."
}
if (-not (Test-Path $uiTypes)) {
    throw "Missing apps/desktop-ui/src/types.ts"
}
if (-not (Test-Path $csharpProtocol)) {
    throw "Missing src/OpenUnpack.Worker/Protocol.cs"
}

# 2. 检查关键契约结构是否在 UI types.ts 中声明
$uiContent = [IO.File]::ReadAllText($uiTypes)
$requiredUiInterfaces = @('PasswordInput', 'PasswordView', 'ImportPreview', 'ImportRow', 'SourceGroup', 'Settings', 'NodeView', 'JobView', 'AppSnapshot')
foreach ($iface in $requiredUiInterfaces) {
    if ($uiContent -notmatch "interface\s+$iface\b") {
        throw "UI types.ts missing interface: $iface"
    }
}

# 3. 检查 Worker 契约关键类是否在 C# Protocol.cs 中声明
$csContent = [IO.File]::ReadAllText($csharpProtocol)
$requiredCsRecords = @('FileRecord', 'CopyFile', 'WorkerRequest', 'WorkerEvent')
foreach ($rec in $requiredCsRecords) {
    if ($csContent -notmatch "record\s+$rec\b") {
        throw "C# Protocol.cs missing record: $rec"
    }
}

Write-Host "==> Contract structural check passed."

# 4. 可选运行 cargo test -p openunpack-protocol
if ($RunCargoTest) {
    Write-Host "==> Running protocol golden sample tests..."
    & cargo test -p openunpack-protocol --test golden_samples
    if ($LASTEXITCODE -ne 0) {
        throw "Protocol golden sample tests failed."
    }
}

Write-Host "==> Contracts are verified and in sync."
