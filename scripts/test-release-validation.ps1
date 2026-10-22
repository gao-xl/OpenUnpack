$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'common.ps1')
Assert-OpenUnpackWindows

# Load only the pure validation functions; never execute installer or signing entry points.
foreach ($taskScript in @('verify-installer-isolated.ps1', 'verify-release-signatures.ps1')) {
    $taskTokens = $null
    $taskParseErrors = $null
    $taskAst = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot $taskScript), [ref]$taskTokens, [ref]$taskParseErrors)
    if ($taskParseErrors.Count -ne 0) { throw "PowerShell parsing failed: $taskScript" }
    foreach ($taskFunction in $taskAst.FindAll({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] }, $false)) {
        if ($taskFunction.Name -in @('Get-OpenUnpackInstallerPayload', 'Assert-OpenUnpackInstallerPayload', 'Get-OpenUnpackSignaturePolicyErrors')) {
            . ([scriptblock]::Create($taskFunction.Extent.Text))
        }
    }
}
$taskChecks = [Collections.Generic.List[object]]::new()
function Test-ReleaseCase([string]$Name, [scriptblock]$Body) {
    & $Body
    $taskChecks.Add(@{ name = $Name; status = 'passed' })
}
function Assert-Rejected([scriptblock]$Body) {
    $taskRejected = $false
    try { & $Body } catch { $taskRejected = $true }
    if (-not $taskRejected) { throw 'Invalid release validation input was accepted.' }
}
$taskTestRoot = Join-Path (Join-Path (Get-OpenUnpackRoot) 'artifacts') ('release-validation-tests-' + [guid]::NewGuid().ToString('N'))
$null = Assert-OpenUnpackOwnedPath -Path $taskTestRoot -Parent (Join-Path (Get-OpenUnpackRoot) 'artifacts')
$taskExpectedRoot = Join-Path $taskTestRoot 'expected'
$taskActualRoot = Join-Path $taskTestRoot 'installed'
New-OpenUnpackDirectory $taskExpectedRoot
New-OpenUnpackDirectory $taskActualRoot
$taskFixtureFiles = @('openunpack-desktop.exe', 'uninstall.exe', 'LICENSE', 'THIRD_PARTY_NOTICES.md',
    'worker/OpenUnpack.Worker.exe', 'worker/OpenUnpack.Worker.dll', 'worker/OpenUnpack.Engine.dll', 'worker/7z.dll',
    'worker/OpenUnpack.Worker.runtimeconfig.json', 'worker/System.Private.CoreLib.dll', 'worker/hostfxr.dll',
    'worker/hostpolicy.dll', 'worker/coreclr.dll', 'worker/clrjit.dll', 'worker/7zip-License.txt',
    'worker/7zip-provenance.json', 'worker/THIRD_PARTY_NOTICES.md', 'worker/synthetic-runtime-component.dll',
    'release-materials/synthetic/LICENSE.txt')
foreach ($taskRelative in $taskFixtureFiles) {
    foreach ($taskDirectory in @($taskExpectedRoot, $taskActualRoot)) {
        $taskPath = Join-Path $taskDirectory $taskRelative
        New-OpenUnpackDirectory ([IO.Path]::GetDirectoryName($taskPath))
        [IO.File]::WriteAllText($taskPath, 'synthetic ' + $taskRelative)
    }
}
$taskPluginRoot = Join-Path $taskExpectedRoot '$PLUGINSDIR'
New-OpenUnpackDirectory $taskPluginRoot
[IO.File]::WriteAllText((Join-Path $taskPluginRoot 'synthetic-plugin.dll'), 'installer only')
$taskTempRoot = Join-Path $taskExpectedRoot '$TEMP'
New-OpenUnpackDirectory $taskTempRoot
[IO.File]::WriteAllText((Join-Path $taskTempRoot 'synthetic-bootstrap.exe'), 'installer only')
$taskExpected = Get-OpenUnpackInstallerPayload -Directory $taskExpectedRoot -ExtractedSetup
Test-ReleaseCase 'complete payload with NSIS-only exclusions' {
    if ($taskExpected.Count -ne $taskFixtureFiles.Count) { throw 'Expected file omitted or installer helper retained.' }
    Assert-OpenUnpackInstallerPayload $taskExpected (Get-OpenUnpackInstallerPayload $taskActualRoot)
}
Test-ReleaseCase 'unchanged Host cannot hide a stale runtime' {
    $taskActual = Get-OpenUnpackInstallerPayload $taskActualRoot
    $taskActual['worker/synthetic-runtime-component.dll'] = '0' * 64
    Assert-Rejected { Assert-OpenUnpackInstallerPayload $taskExpected $taskActual }
}
Test-ReleaseCase 'missing required runtime rejected' {
    $taskIncomplete = Join-Path $taskTestRoot 'incomplete'
    New-OpenUnpackDirectory $taskIncomplete
    foreach ($taskRelative in $taskFixtureFiles | Where-Object { $_ -ne 'worker/coreclr.dll' }) {
        $taskPath = Join-Path $taskIncomplete $taskRelative
        New-OpenUnpackDirectory ([IO.Path]::GetDirectoryName($taskPath))
        [IO.File]::WriteAllText($taskPath, 'synthetic ' + $taskRelative)
    }
    Assert-Rejected { Get-OpenUnpackInstallerPayload $taskIncomplete }
}
Test-ReleaseCase 'missing legal file rejected' {
    $taskActual = Get-OpenUnpackInstallerPayload $taskActualRoot
    $null = $taskActual.Remove('worker/7zip-License.txt')
    Assert-Rejected { Assert-OpenUnpackInstallerPayload $taskExpected $taskActual }
}
Test-ReleaseCase 'unexpected installed file rejected' {
    $taskActual = Get-OpenUnpackInstallerPayload $taskActualRoot
    $taskActual.Add('unexpected.dll', '0' * 64)
    Assert-Rejected { Assert-OpenUnpackInstallerPayload $taskExpected $taskActual }
}
Test-ReleaseCase 'same-count path substitution rejected' {
    $taskActual = Get-OpenUnpackInstallerPayload $taskActualRoot
    $null = $taskActual.Remove('worker/synthetic-runtime-component.dll')
    $taskActual.Add('worker/substituted.dll', '0' * 64)
    Assert-Rejected { Assert-OpenUnpackInstallerPayload $taskExpected $taskActual }
}
Test-ReleaseCase 'portable versus NSIS Host marker mismatch rejected' {
    $taskActual = Get-OpenUnpackInstallerPayload $taskActualRoot
    $taskActual['openunpack-desktop.exe'] = '0' * 64
    Assert-Rejected { Assert-OpenUnpackInstallerPayload $taskExpected $taskActual }
}
Test-ReleaseCase 'linked payload directory rejected' {
    $taskLinked = Join-Path $taskTestRoot 'linked'
    $null = New-Item -ItemType Junction -Path $taskLinked -Target $taskActualRoot
    Assert-Rejected { Get-OpenUnpackInstallerPayload $taskLinked }
}
$taskPin = 'a' * 40
Test-ReleaseCase 'trusted pinned CodeSigning metadata accepted by policy only' {
    if (@(Get-OpenUnpackSignaturePolicyErrors 'Valid' $taskPin $true $taskPin.ToUpperInvariant()).Count -ne 0) { throw 'Valid synthetic policy metadata failed.' }
}
foreach ($taskInvalid in @(
    @{ name = 'unsigned'; status = 'NotSigned'; thumbprint = ''; eku = $false; pin = $taskPin },
    @{ name = 'untrusted'; status = 'NotTrusted'; thumbprint = $taskPin; eku = $true; pin = $taskPin },
    @{ name = 'tampered'; status = 'HashMismatch'; thumbprint = $taskPin; eku = $true; pin = $taskPin },
    @{ name = 'wrong publisher'; status = 'Valid'; thumbprint = ('b' * 40); eku = $true; pin = $taskPin },
    @{ name = 'missing CodeSigning EKU'; status = 'Valid'; thumbprint = $taskPin; eku = $false; pin = $taskPin },
    @{ name = 'missing publisher pin'; status = 'Valid'; thumbprint = $taskPin; eku = $true; pin = '' },
    @{ name = 'malformed publisher pin'; status = 'Valid'; thumbprint = $taskPin; eku = $true; pin = 'invalid' }
)) {
    Test-ReleaseCase ('signature rejection: ' + $taskInvalid.name) {
        if (@(Get-OpenUnpackSignaturePolicyErrors $taskInvalid.status $taskInvalid.thumbprint $taskInvalid.eku $taskInvalid.pin).Count -eq 0) { throw 'Invalid synthetic signature metadata passed.' }
    }
}
@{ status = 'passed'; scope = 'Synthetic validation-policy fixtures only; no installer execution, signing or clean-machine acceptance.'; checks = $taskChecks.ToArray() } |
    ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $taskTestRoot 'results.json') -Encoding utf8NoBOM
Write-Output ("PASS $($taskChecks.Count) release validation cases; retained synthetic fixtures: $taskTestRoot")
