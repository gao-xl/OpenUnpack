# Scoped structural checks always run; -RunCargoTest adds real Rust -> C# -> Rust checks.
param([switch]$RunCargoTest)
. (Join-Path $PSScriptRoot 'common.ps1')
$taskRoot = Get-OpenUnpackRoot
Push-Location $taskRoot
try {
    Invoke-OpenUnpackTool node @('scripts/check-contracts.mjs', '--self-test')
    Invoke-OpenUnpackTool node @('scripts/check-architecture.mjs', '--self-test')
    & (Join-Path $PSScriptRoot 'test-security-audit-policy.ps1')
    if ($RunCargoTest) {
        Import-OpenUnpackMsvc
        $taskCorpus = Join-Path $taskRoot 'artifacts/issue-fixes-2026-10-10/contracts'
        New-OpenUnpackDirectory $taskCorpus
        Invoke-OpenUnpackTool cargo @('run', '-p', 'openunpack-protocol', '--example', 'contract_samples', '--locked', '--offline', '--', 'export', $taskCorpus)
        Invoke-OpenUnpackTool dotnet @('run', '--project', 'tests/OpenUnpack.ProtocolChecks/OpenUnpack.ProtocolChecks.csproj', '-c', 'Release', '--', $taskCorpus)
        Invoke-OpenUnpackTool cargo @('run', '-p', 'openunpack-protocol', '--example', 'contract_samples', '--locked', '--offline', '--', 'verify', $taskCorpus)
        Invoke-OpenUnpackTool cargo @('test', '-p', 'openunpack-protocol', '--test', 'golden_samples', '--locked', '--offline')
        Write-Host 'Rust -> C# public DTO -> Rust checks passed. Compatibility differences are recorded in the corpus.'
    }
} finally { Pop-Location }
