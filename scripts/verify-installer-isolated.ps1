param([Parameter(Mandatory)][string]$SetupExe, [Parameter(Mandatory)][string]$ProductName,
      [Parameter(Mandatory)][string]$EvidenceDirectory, [string]$ExpectedPayloadDirectory,
      [switch]$PreserveSyntheticData)
. (Join-Path $PSScriptRoot 'common.ps1')
Assert-OpenUnpackWindows

function Get-OpenUnpackInstallerPayload {
    param([Parameter(Mandatory)][string]$Directory, [switch]$ExtractedSetup)
    $taskPayloadRoot = [IO.Path]::GetFullPath($Directory)
    if (-not (Test-Path -LiteralPath $taskPayloadRoot -PathType Container)) { throw '安装载荷目录不存在。' }
    $null = Assert-OpenUnpackOwnedPath -Path $taskPayloadRoot -Parent ([IO.Path]::GetDirectoryName($taskPayloadRoot))
    $taskManifest = [Collections.Generic.Dictionary[string,string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($taskFile in Get-ChildItem -LiteralPath $taskPayloadRoot -File -Recurse -Force) {
        $taskRelative = [IO.Path]::GetRelativePath($taskPayloadRoot, $taskFile.FullName).Replace('\', '/')
        # These directories contain NSIS plugins and the WebView bootstrapper, not installed application files.
        if ($ExtractedSetup -and $taskRelative.Split('/')[0] -in @('$PLUGINSDIR', '$TEMP')) { continue }
        if (-not $taskManifest.TryAdd($taskRelative, (Get-FileHash -LiteralPath $taskFile.FullName -Algorithm SHA256).Hash.ToLowerInvariant())) {
            throw '安装载荷存在大小写冲突。'
        }
    }
    foreach ($taskRequired in @('openunpack-desktop.exe', 'uninstall.exe', 'LICENSE', 'THIRD_PARTY_NOTICES.md',
        'worker/OpenUnpack.Worker.exe', 'worker/OpenUnpack.Worker.dll', 'worker/OpenUnpack.Engine.dll', 'worker/7z.dll',
        'worker/OpenUnpack.Worker.runtimeconfig.json', 'worker/System.Private.CoreLib.dll',
        'worker/hostfxr.dll', 'worker/hostpolicy.dll', 'worker/coreclr.dll', 'worker/clrjit.dll',
        'worker/7zip-License.txt', 'worker/7zip-provenance.json', 'worker/THIRD_PARTY_NOTICES.md')) {
        if (-not $taskManifest.ContainsKey($taskRequired)) { throw "安装载荷不完整：$taskRequired" }
    }
    return ,$taskManifest
}

function Assert-OpenUnpackInstallerPayload {
    param([Parameter(Mandatory)]$Expected, [Parameter(Mandatory)]$Actual)
    if ($Expected.Count -eq 0 -or $Expected.Count -ne $Actual.Count) { throw '安装载荷文件数量不一致。' }
    foreach ($taskEntry in $Expected.GetEnumerator()) {
        if (-not $Actual.ContainsKey($taskEntry.Key) -or $Actual[$taskEntry.Key] -cne $taskEntry.Value) {
            throw "安装载荷与同一 NSIS 候选不符：$($taskEntry.Key)"
        }
    }
}

# Only a dedicated audit product may be installed. Never touch the real app's keys.
if ($ProductName -notmatch '^OpenUnpackAudit-[a-f0-9]{32}$') { throw '安装验收需要独立的审计产品名。' }
$taskRoot = Get-OpenUnpackRoot
$taskEvidence = Assert-OpenUnpackOwnedPath -Path $EvidenceDirectory -Parent (Join-Path $taskRoot 'artifacts')
if ((Test-Path -LiteralPath $taskEvidence) -and @(Get-ChildItem -LiteralPath $taskEvidence -Force).Count -ne 0) {
    throw '安装证据目录非空，拒绝覆盖历史结果。'
}
New-OpenUnpackDirectory $taskEvidence
$taskSetup = (Get-Item -LiteralPath $SetupExe).FullName
$taskSetupInfo = [Diagnostics.FileVersionInfo]::GetVersionInfo($taskSetup)
if ($taskSetupInfo.ProductName -ne $ProductName -or $taskSetupInfo.CompanyName -ne 'openunpack') {
    throw '安装包产品名或发行者与隔离配置不符，拒绝启动。'
}
$taskSetupSha256 = (Get-FileHash -LiteralPath $taskSetup -Algorithm SHA256).Hash.ToLowerInvariant()
$taskSevenZip = Join-Path $taskRoot 'tools/7zip/7z.exe'
if (-not (Test-Path -LiteralPath $taskSevenZip -PathType Leaf)) { throw '需要项目内 7-Zip 解包并绑定同一 NSIS 安装载荷。' }
$taskExtracted = Join-Path $taskEvidence ('same-nsis-' + [guid]::NewGuid().ToString('N'))
$null = Assert-OpenUnpackOwnedPath -Path $taskExtracted -Parent $taskEvidence
New-OpenUnpackDirectory $taskExtracted
& $taskSevenZip x $taskSetup ('-o' + $taskExtracted) -y -bso0 -bsp0 -bse0
if ($LASTEXITCODE -ne 0) { throw 'NSIS 解包失败，拒绝启动安装。' }
if ((Get-FileHash -LiteralPath $taskSetup -Algorithm SHA256).Hash.ToLowerInvariant() -cne $taskSetupSha256) { throw '安装包在解包过程中改变。' }
$taskExpectedPayload = Get-OpenUnpackInstallerPayload -Directory $taskExtracted -ExtractedSetup
if ($ExpectedPayloadDirectory) {
    $taskSuppliedPayload = Get-OpenUnpackInstallerPayload -Directory $ExpectedPayloadDirectory -ExtractedSetup
    Assert-OpenUnpackInstallerPayload -Expected $taskExpectedPayload -Actual $taskSuppliedPayload
}
@{ setupSha256 = $taskSetupSha256; extractionDirectory = $taskExtracted; files = $taskExpectedPayload } |
    ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $taskEvidence 'expected-installed-payload.json') -Encoding utf8NoBOM
$taskData = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA $ProductName))
$taskProgramParent = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Programs'))
$taskProgram = [IO.Path]::GetFullPath((Join-Path $taskProgramParent $ProductName))
$taskProductKey = "HKCU:\Software\openunpack\$ProductName"
$taskUninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$ProductName"
foreach ($taskPath in @($taskData, $taskProgram, $taskProductKey, $taskUninstallKey)) {
    if (Test-Path -LiteralPath $taskPath) { throw "隔离验收目标已存在，拒绝修改：$taskPath" }
}
$taskRows = [Collections.Generic.List[object]]::new()
$taskSetupRuns = [Collections.Generic.List[object]]::new()
function Test-Step([string]$Name, [scriptblock]$Body) {
    try { & $Body; $taskRows.Add(@{ name = $Name; status = 'passed' }) }
    catch { $taskRows.Add(@{ name = $Name; status = 'failed'; message = $_.Exception.Message }); throw }
    finally { $taskRows | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $taskEvidence 'installer-results.json') -Encoding utf8NoBOM }
    Write-Output "PASS $Name"
}
function Invoke-Setup([string[]]$Arguments, [bool]$ExpectedSuccess = $true) {
    if ((Get-FileHash -LiteralPath $taskSetup -Algorithm SHA256).Hash.ToLowerInvariant() -cne $taskSetupSha256) { throw '安装包已改变，拒绝运行。' }
    $taskProcess = Start-Process -FilePath $taskSetup -ArgumentList $Arguments -WindowStyle Hidden -PassThru
    if (-not $taskProcess.WaitForExit(60000)) { throw "安装进程未退出，保留现场 PID $($taskProcess.Id)" }
    $taskSetupRuns.Add(@{ arguments = $Arguments; exitCode = $taskProcess.ExitCode; expectedSuccess = $ExpectedSuccess })
    $taskSetupRuns | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $taskEvidence 'setup-executions.json') -Encoding utf8NoBOM
    if ($ExpectedSuccess -and $taskProcess.ExitCode -ne 0) { throw "安装失败：$($taskProcess.ExitCode)" }
    if (-not $ExpectedSuccess -and $taskProcess.ExitCode -ne 2) { throw "未收到数据目录阻断退出码：$($taskProcess.ExitCode)" }
}
New-OpenUnpackDirectory $taskData
Set-Content -LiteralPath (Join-Path $taskData '.openunpack-audit-owner') -Value $ProductName -Encoding utf8NoBOM
# Synthetic bytes, not a personal database or vault.
Set-Content -LiteralPath (Join-Path $taskData 'jobs.sqlite') -Value 'synthetic-old-jobs' -Encoding utf8NoBOM
Set-Content -LiteralPath (Join-Path $taskData 'passwords.vault') -Value 'synthetic-old-vault' -Encoding utf8NoBOM
Set-Content -LiteralPath (Join-Path $taskData 'jobs.sqlite-wal') -Value 'synthetic-old-wal' -Encoding utf8NoBOM
Set-Content -LiteralPath (Join-Path $taskData 'passwords.vault.bak') -Value 'synthetic-old-backup' -Encoding utf8NoBOM
$taskBefore = @(Get-ChildItem -LiteralPath $taskData -File | ForEach-Object { @{ name = $_.Name; sha256 = (Get-FileHash -LiteralPath $_.FullName).Hash } })
$taskBefore | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskEvidence 'data-before.json') -Encoding utf8NoBOM
function Assert-DataPreserved {
    foreach ($taskFile in $taskBefore) {
        if ((Get-FileHash -LiteralPath (Join-Path $taskData $taskFile.name)).Hash -ne $taskFile.sha256) { throw '旧数据摘要改变。' }
    }
}
$taskCompleted = $false
try {
    Test-Step 'reject explicit persistent-data installation directory' {
        foreach ($taskAlias in @($taskData, "$taskData\", "$taskData\.")) {
            Invoke-Setup -Arguments @('/S', '/NS', "/D=$taskAlias") -ExpectedSuccess $false
        }
        if (Test-Path -LiteralPath (Join-Path $taskData 'openunpack-desktop.exe')) { throw '程序写入数据目录。' }
        Assert-DataPreserved
    }
    # Simulate the old recorded directory without installing or uninstalling a personal app.
    New-Item -Path $taskProductKey -Force | Out-Null
    Set-Item -LiteralPath $taskProductKey -Value $taskData
    Test-Step 'legacy recorded directory skipped and Programs default selected' {
        Invoke-Setup -Arguments @('/S', '/NS')
        foreach ($taskFile in @('openunpack-desktop.exe', 'uninstall.exe', 'worker/OpenUnpack.Worker.exe', 'worker/7z.dll')) {
            if (-not (Test-Path -LiteralPath (Join-Path $taskProgram $taskFile) -PathType Leaf)) { throw "安装资源缺失：$taskFile" }
        }
        if ((Get-Item -LiteralPath $taskProductKey).GetValue('') -ne $taskProgram) { throw '安装目录注册值不正确。' }
        $taskInstalledPayload = Get-OpenUnpackInstallerPayload -Directory $taskProgram
        Assert-OpenUnpackInstallerPayload -Expected $taskExpectedPayload -Actual $taskInstalledPayload
        @{ files = $taskInstalledPayload; count = $taskInstalledPayload.Count; setupSha256 = $taskSetupSha256 } |
            ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $taskEvidence 'installed-payload.json') -Encoding utf8NoBOM
        Assert-DataPreserved
    }
    Test-Step 'repeat installer preserves data and registered program directory' {
        Invoke-Setup -Arguments @('/S', '/NS')
        if (-not (Test-Path -LiteralPath (Join-Path $taskProgram 'openunpack-desktop.exe'))) { throw '重复安装后程序缺失。' }
        Assert-OpenUnpackInstallerPayload -Expected $taskExpectedPayload -Actual (Get-OpenUnpackInstallerPayload -Directory $taskProgram)
        Assert-DataPreserved
    }
    Test-Step 'uninstall removes payload and retains synthetic legacy data' {
        $taskUninstall = Start-Process -FilePath (Join-Path $taskProgram 'uninstall.exe') -ArgumentList '/S' -WindowStyle Hidden -PassThru
        if (-not $taskUninstall.WaitForExit(60000) -or $taskUninstall.ExitCode -ne 0) { throw '卸载未成功退出。' }
        $taskDeadline = [DateTime]::UtcNow.AddSeconds(30)
        while (((Test-Path -LiteralPath (Join-Path $taskProgram 'openunpack-desktop.exe')) -or (Test-Path -LiteralPath $taskUninstallKey)) -and [DateTime]::UtcNow -lt $taskDeadline) { Start-Sleep -Milliseconds 100 }
        if (Test-Path -LiteralPath (Join-Path $taskProgram 'openunpack-desktop.exe')) { throw '卸载后程序残留。' }
        if (Test-Path -LiteralPath $taskUninstallKey) { throw '卸载注册值残留。' }
        Assert-DataPreserved
    }
    @{ productName = $ProductName; programDirectory = $taskProgram; dataDirectory = $taskData; setupSha256 = $taskSetupSha256; payloadFiles = $taskExpectedPayload.Count; preserveSyntheticData = [bool]$PreserveSyntheticData; scope = 'Same-machine isolated product, legacy directory simulation, repeat installation and uninstall, complete same-NSIS payload hashes; not clean-machine or cross-version data migration acceptance.' } |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskEvidence 'installer-scope.json') -Encoding utf8NoBOM
    $taskCompleted = $true
} finally {
    # Preserve failed installation evidence. Cleanup only this fresh synthetic data
    # and registry key, after validating exact full paths and the owner marker.
    if (Test-Path -LiteralPath $taskProgram) { Write-Output "安装目录保留：$taskProgram" }
    if ($PreserveSyntheticData -or -not $taskCompleted -or
        (Test-Path -LiteralPath (Join-Path $taskData 'openunpack-desktop.exe')) -or (Test-Path -LiteralPath $taskUninstallKey)) {
        Write-Output "失败现场保留：$taskData"
    } else {
        try {
            $taskCheckedData = Assert-OpenUnpackOwnedPath -Path $taskData -Parent $env:LOCALAPPDATA
            if ((Get-Content -Raw -LiteralPath (Join-Path $taskCheckedData '.openunpack-audit-owner')).Trim() -ne $ProductName) { throw '隔离数据所有权标记改变，拒绝清理。' }
            Remove-Item -LiteralPath $taskCheckedData -Recurse -Force
            if (Test-Path -LiteralPath $taskProductKey) { Remove-Item -LiteralPath $taskProductKey -Recurse -Force }
        } catch {
            @{ status = 'retained'; message = $_.Exception.Message; dataDirectory = $taskData } |
                ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskEvidence 'cleanup-result.json') -Encoding utf8NoBOM
            Write-Warning '合成现场清理未完成；验收结果保持独立，详情见 cleanup-result.json。'
        }
    }
}
