[CmdletBinding()]
param([string]$WorkerExecutable, [string]$EnginePath)
$ErrorActionPreference = 'Stop'
$taskProjectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$taskWorker = if ($WorkerExecutable) { $WorkerExecutable } else { Join-Path $PSScriptRoot 'bin\Release\net10.0-windows\OpenUnpack.Worker.exe' }
$taskEngine = if ($EnginePath) { $EnginePath } else { Join-Path $taskProjectRoot 'tools\7zip\7z.dll' }
$taskFixture = Join-Path $taskProjectRoot ('artifacts\worker-verification-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($taskFixture) | Out-Null
$taskResults = [Collections.Generic.List[object]]::new()
$taskPrefix = 'verify-' + [guid]::NewGuid().ToString('N')
$taskProcessInfo = [Diagnostics.ProcessStartInfo]::new($taskWorker)
$taskProcessInfo.UseShellExecute = $false
$taskProcessInfo.CreateNoWindow = $true
$taskProcessInfo.RedirectStandardOutput = $true
$taskProcessInfo.RedirectStandardError = $true
foreach ($taskArgument in @('--pipe-prefix', $taskPrefix, '--parent-pid', "$PID", '--engine', $taskEngine)) { $taskProcessInfo.ArgumentList.Add($taskArgument) }
$taskProcess = [Diagnostics.Process]::Start($taskProcessInfo)
$taskControl = [IO.Pipes.NamedPipeClientStream]::new('.', $taskPrefix + '-control', [IO.Pipes.PipeDirection]::InOut, [IO.Pipes.PipeOptions]::Asynchronous)
$taskEvents = [IO.Pipes.NamedPipeClientStream]::new('.', $taskPrefix + '-events', [IO.Pipes.PipeDirection]::InOut, [IO.Pipes.PipeOptions]::Asynchronous)

function Receive-WorkerEvent {
    $taskRead = $script:taskReader.ReadLineAsync()
    if (-not $taskRead.Wait([TimeSpan]::FromSeconds(30))) { throw 'Worker event timed out.' }
    $taskLine = $taskRead.Result
    if ($null -eq $taskLine) { throw 'Worker disconnected.' }
    return ($taskLine | ConvertFrom-Json)
}

function Invoke-WorkerJob([hashtable]$Request) {
    $Request.protocolVersion = 1
    if (-not $Request.ContainsKey('requestId')) { $Request.requestId = [guid]::NewGuid().ToString('N') }
    $script:taskWriter.WriteLine(($Request | ConvertTo-Json -Depth 20 -Compress))
    $script:taskWriter.Flush()
    while ($true) {
        $taskEvent = Receive-WorkerEvent
        if ($taskEvent.requestId -eq $Request.requestId -and $taskEvent.kind -in @('completed', 'error', 'cancelled')) { return $taskEvent }
    }
}

function New-ZipFixture([string]$Name, [hashtable]$Entries) {
    $taskPath = Join-Path $script:taskFixture $Name
    $taskZip = [IO.Compression.ZipFile]::Open($taskPath, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($taskName in $Entries.Keys) {
            $taskEntry = $taskZip.CreateEntry($taskName)
            if (-not $taskName.EndsWith('/')) {
                $taskOutput = $taskEntry.Open()
                try { $taskBytes = [Text.Encoding]::UTF8.GetBytes($Entries[$taskName]); $taskOutput.Write($taskBytes) }
                finally { $taskOutput.Dispose() }
            }
        }
    } finally { $taskZip.Dispose() }
    return $taskPath
}

function New-ExtractRequest([string]$Archive, [string]$Name) {
    return @{ operation = 'extract'; archivePath = $Archive; destinationDirectory = (Join-Path $script:taskFixture $Name)
        allowedVolumes = @($Archive); maxEntries = 100; maxTotalBytes = 10000000; maxWrittenBytes = 10000000; minFreeBytes = 0; exclusions = @() }
}

function Assert-Result([bool]$Condition, [string]$Name) {
    if (-not $Condition) { throw ('Failed: ' + $Name) }
    $script:taskResults.Add(@{ check = $Name; passed = $true })
}

function Test-SafeDiagnostic([string]$Line, [string]$ExpectedIdentity) {
    if ([Text.Encoding]::UTF8.GetByteCount($Line) -gt 512) { return $false }
    $document = $null
    try {
        $document = [Text.Json.JsonDocument]::Parse($Line)
        # Enumerating raw JSON fields also rejects duplicate keys.
        if ($document.RootElement.ValueKind -ne [Text.Json.JsonValueKind]::Object -or
            @($document.RootElement.EnumerateObject()).Count -ne 8) { return $false }
        $record = $Line | ConvertFrom-Json -AsHashtable -ErrorAction Stop
        $fields = @('kind', 'version', 'operation', 'phase', 'errorCode', 'exceptionType', 'hResult', 'nativeCode')
        if ($record.Count -ne 8 -or @($record.Keys | Where-Object { $_ -cnotin $fields }).Count -ne 0) { return $false }
        foreach ($field in @('kind', 'operation', 'phase', 'errorCode', 'exceptionType')) {
            if ($record[$field] -isnot [string]) { return $false }
        }
        $identity = '{0}:{1}:{2}' -f $record.operation, $record.phase, $record.errorCode
        return $identity -ceq $ExpectedIdentity -and $record.kind -ceq 'openunpackDiagnostic' -and
            ($record.version -is [long] -or $record.version -is [int]) -and $record.version -eq 1 -and
            $record.exceptionType -ceq 'ArchiveException' -and
            ($record.hResult -is [long] -or $record.hResult -is [int]) -and
            $record.hResult -ge [int]::MinValue -and $record.hResult -le [int]::MaxValue -and
            $null -eq $record.nativeCode
    } catch { return $false }
    finally { if ($document) { $document.Dispose() } }
}

try {
    $taskControl.Connect(15000); $taskEvents.Connect(15000)
    $script:taskWriter = [IO.StreamWriter]::new($taskControl, [Text.UTF8Encoding]::new($false), 4096, $true)
    $script:taskReader = [IO.StreamReader]::new($taskEvents, [Text.UTF8Encoding]::new($false), $false, 4096, $true)
    $taskReady = Receive-WorkerEvent
    Assert-Result ($taskReady.kind -eq 'ready' -and $taskReady.protocolVersion -eq 1) 'authenticated pipes ready'

    $taskArchive = New-ZipFixture 'plain.zip' @{ 'folder/hello.txt' = 'native worker text'; 'empty/' = ''; 'omit.url' = 'omit' }
    $taskRequest = New-ExtractRequest $taskArchive 'extract'
    $taskRequest.exclusions = @('*.url')
    $taskExtract = Invoke-WorkerJob $taskRequest
    Assert-Result ($taskExtract.kind -eq 'completed') 'native ZIP extract completed'
    $taskTextRecord = @($taskExtract.files | Where-Object relativePath -eq 'folder\hello.txt')[0]
    $taskEmptyRecord = @($taskExtract.files | Where-Object relativePath -eq 'empty')[0]
    $taskExpectedHash = [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes('native worker text')))
    Assert-Result ($taskTextRecord.sha256 -eq $taskExpectedHash) 'stream SHA256 matches known content'
    Assert-Result ($taskEmptyRecord.isDirectory) 'empty directory preserved in manifest'
    Assert-Result (-not (Test-Path -LiteralPath (Join-Path $taskFixture 'extract\omit.url'))) 'output exclusion applied'
    Assert-Result (-not $taskExtract.passwordUsed) 'unencrypted archive does not report password success'

    $taskCopyFiles = @($taskExtract.files | ForEach-Object { @{ sourcePath = (Join-Path $taskRequest.destinationDirectory $_.relativePath)
        relativePath = $_.relativePath; size = $_.size; sha256 = $_.sha256; isDirectory = $_.isDirectory } })
    $taskTarget = Join-Path $taskFixture 'target'
    $taskCopy = Invoke-WorkerJob @{ operation = 'copy'; destinationDirectory = $taskTarget; files = $taskCopyFiles; maxWrittenBytes = 10000000 }
    Assert-Result ($taskCopy.kind -eq 'completed' -and $taskCopy.verificationScope -eq 'sourceStreamSha256') 'copy with expected digest completed'
    $taskReadbackFiles = @($taskCopy.files | ForEach-Object { @{ sourcePath = (Join-Path $taskTarget $_.relativePath)
        relativePath = $_.relativePath; size = $_.size; sha256 = $_.sha256; isDirectory = $_.isDirectory } })
    $taskReadback = Invoke-WorkerJob @{ operation = 'readback'; destinationDirectory = $taskTarget; files = $taskReadbackFiles }
    Assert-Result ($taskReadback.kind -eq 'completed' -and $taskReadback.verificationScope -eq 'targetReadback') 'target readback completed'
    [IO.File]::WriteAllText((Join-Path $taskTarget 'folder\hello.txt'), 'different same len')
    $taskMismatch = Invoke-WorkerJob @{ operation = 'readback'; destinationDirectory = $taskTarget; files = $taskReadbackFiles }
    Assert-Result ($taskMismatch.kind -eq 'error' -and $taskMismatch.errorCode -eq 'Corrupt') 'target modification rejected'

    $taskUnsafeArchive = New-ZipFixture 'unsafe.zip' @{ '../escaped.txt' = 'unsafe' }
    $taskUnsafe = Invoke-WorkerJob (New-ExtractRequest $taskUnsafeArchive 'unsafe-output')
    Assert-Result ($taskUnsafe.kind -eq 'error' -and $taskUnsafe.errorCode -eq 'UnsafePath') 'archive path traversal rejected'
    Assert-Result (-not (Test-Path -LiteralPath (Join-Path $taskFixture 'escaped.txt'))) 'path traversal writes nothing outside root'
    $taskLimitedRequest = New-ExtractRequest $taskArchive 'limited'
    $taskLimitedRequest.maxTotalBytes = 1
    $taskLimited = Invoke-WorkerJob $taskLimitedRequest
    Assert-Result ($taskLimited.kind -eq 'error' -and $taskLimited.errorCode -eq 'ResourceLimit') 'expanded byte budget rejected'

    $taskBadCopyFiles = @(@{ sourcePath = (Join-Path $taskFixture 'extract\folder\hello.txt'); relativePath = 'bad.txt'; size = 18; sha256 = ('0' * 64); isDirectory = $false })
    $taskBadCopy = Invoke-WorkerJob @{ operation = 'copy'; destinationDirectory = (Join-Path $taskFixture 'bad-copy'); files = $taskBadCopyFiles; maxWrittenBytes = 100 }
    Assert-Result ($taskBadCopy.kind -eq 'error' -and $taskBadCopy.errorCode -eq 'Corrupt') 'copy digest mismatch rejected'

    $taskEncryptedInput = Join-Path $taskFixture 'secret-input'
    [IO.Directory]::CreateDirectory($taskEncryptedInput) | Out-Null
    [IO.File]::WriteAllText((Join-Path $taskEncryptedInput 'private.txt'), 'fixture private content')
    $taskEncrypted = Join-Path $taskFixture 'encrypted.7z'
    $task7z = Join-Path $taskProjectRoot 'tools\7zip\7z.exe'
    & $task7z a $taskEncrypted (Join-Path $taskEncryptedInput 'private.txt') '-pfixture-password' '-mhe=on' '-y' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Cannot build encrypted test fixture.' }
    $taskNeeds = Invoke-WorkerJob (New-ExtractRequest $taskEncrypted 'needs')
    Assert-Result ($taskNeeds.kind -eq 'error' -and $taskNeeds.errorCode -eq 'NeedsPassword') 'encrypted header requests password'
    $taskWrongRequest = New-ExtractRequest $taskEncrypted 'wrong'
    $taskWrongRequest.password = 'fixture-wrong'
    $taskWrong = Invoke-WorkerJob $taskWrongRequest
    Assert-Result ($taskWrong.kind -eq 'error' -and $taskWrong.errorCode -eq 'PasswordMismatchOrCorrupt') 'wrong encrypted header password rejected'
    $taskCorrectRequest = New-ExtractRequest $taskEncrypted 'correct'
    $taskCorrectRequest.password = 'fixture-password'
    $taskCorrect = Invoke-WorkerJob $taskCorrectRequest
    Assert-Result ($taskCorrect.kind -eq 'completed' -and $taskCorrect.passwordUsed) 'correct encrypted header password and content verified'

    $taskSplit = Join-Path $taskFixture 'split.7z'
    & $task7z a $taskSplit (Join-Path $taskEncryptedInput 'private.txt') '-v50b' '-y' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Cannot build split test fixture.' }
    $taskSplitVolumes = @(Get-ChildItem -LiteralPath $taskFixture -Filter 'split.7z.*' | Sort-Object Name | ForEach-Object FullName)
    $taskSplitRequest = New-ExtractRequest $taskSplitVolumes[0] 'split-output'
    $taskSplitRequest.allowedVolumes = $taskSplitVolumes
    $taskSplitResult = Invoke-WorkerJob $taskSplitRequest
    Assert-Result ($taskSplitResult.kind -eq 'completed') 'approved numeric volumes extracted'
    $taskMissingRequest = New-ExtractRequest $taskSplitVolumes[0] 'missing-split'
    $taskMissingRequest.allowedVolumes = @($taskSplitVolumes[0], $taskSplitVolumes[-1])
    if ($taskSplitVolumes.Length -gt 2) {
        $taskMissingResult = Invoke-WorkerJob $taskMissingRequest
        Assert-Result ($taskMissingResult.kind -eq 'error' -and $taskMissingResult.errorCode -eq 'MissingVolume') 'missing middle approved volume rejected'
    }

    $taskLinkArchive = New-ZipFixture 'link.zip' @{ 'link' = 'folder/hello.txt' }
    $taskLinkZip = [IO.Compression.ZipFile]::Open($taskLinkArchive, [IO.Compression.ZipArchiveMode]::Update)
    try { $taskLinkZip.GetEntry('link').ExternalAttributes = [BitConverter]::ToInt32([BitConverter]::GetBytes([Convert]::ToUInt32('A1FF0000', 16)), 0) }
    finally { $taskLinkZip.Dispose() }
    # ZIP host OS must be Unix for the upper attributes to describe a symlink.
    $taskLinkBytes = [IO.File]::ReadAllBytes($taskLinkArchive)
    for ($taskOffset = 0; $taskOffset -lt $taskLinkBytes.Length - 46; $taskOffset++) {
        if ([BitConverter]::ToUInt32($taskLinkBytes, $taskOffset) -eq 0x02014b50) { $taskLinkBytes[$taskOffset + 5] = 3 }
    }
    [IO.File]::WriteAllBytes($taskLinkArchive, $taskLinkBytes)
    $taskLinkResult = Invoke-WorkerJob (New-ExtractRequest $taskLinkArchive 'link-output')
    Assert-Result ($taskLinkResult.kind -eq 'error' -and $taskLinkResult.errorCode -eq 'UnsafePath') 'archive symbolic link rejected'

    $taskLargePath = Join-Path $taskFixture 'large-fixture.bin'
    $taskLargeFile = [IO.File]::Open($taskLargePath, [IO.FileMode]::CreateNew)
    try { $taskLargeFile.SetLength(512MB) } finally { $taskLargeFile.Dispose() }
    $taskCancelId = [guid]::NewGuid().ToString('N')
    $taskCancelRoot = Join-Path $taskFixture 'cancel-copy'
    $taskCancelRequest = @{ protocolVersion = 1; requestId = $taskCancelId; operation = 'copy'; destinationDirectory = $taskCancelRoot
        files = @(@{ sourcePath = $taskLargePath; relativePath = 'large.bin'; size = 512MB; sha256 = ''; isDirectory = $false }); maxWrittenBytes = 1GB }
    $script:taskWriter.WriteLine(($taskCancelRequest | ConvertTo-Json -Depth 10 -Compress)); $script:taskWriter.Flush()
    $taskRequestedCancel = $false; $taskPingSeen = $false
    while ($true) {
        $taskCancelEvent = Receive-WorkerEvent
        if ($taskCancelEvent.requestId -eq 'cancel-ping' -and $taskCancelEvent.kind -eq 'heartbeat') { $taskPingSeen = $true }
        if ($taskCancelEvent.requestId -ne $taskCancelId) { continue }
        if (-not $taskRequestedCancel -and $taskCancelEvent.kind -eq 'progress') {
            $script:taskWriter.WriteLine('{"protocolVersion":1,"requestId":"cancel-ping","operation":"ping"}')
            $script:taskWriter.WriteLine((@{ protocolVersion = 1; requestId = $taskCancelId; operation = 'cancel' } | ConvertTo-Json -Compress))
            $script:taskWriter.Flush(); $taskRequestedCancel = $true
        }
        if ($taskCancelEvent.kind -in @('completed', 'error', 'cancelled')) { break }
    }
    Assert-Result ($taskRequestedCancel -and $taskCancelEvent.kind -eq 'cancelled') 'active transfer cancellation on independent control pipe'
    Assert-Result $taskPingSeen 'ping responds during active transfer'
    [IO.File]::Delete($taskLargePath)
    [IO.File]::Delete((Join-Path $taskCancelRoot 'large.bin'))
    Assert-Result (Test-Path -LiteralPath $taskArchive) 'original archive preserved'

    $script:taskWriter.WriteLine('{"protocolVersion":1,"requestId":"shutdown","operation":"exit"}')
    $script:taskWriter.Flush()
    if (-not $taskProcess.WaitForExit(10000)) { throw 'Worker exit deadline failed.' }
    $taskStdout = $taskProcess.StandardOutput.ReadToEnd()
    $taskStderr = $taskProcess.StandardError.ReadToEnd()
    Assert-Result ($taskStdout.Length -eq 0) 'stdout contains no requests or secrets'
    # SafeDiagnostics intentionally reports these negative fixtures on stderr.
    # Accept only their exact bounded schema and expected enum values: arbitrary
    # text, extra fields, paths, request identifiers and passwords still fail.
    $taskExpectedDiagnostics = @(
        'readback:transfer:Corrupt', 'extract:decode:UnsafePath',
        'extract:decode:ResourceLimit', 'copy:transfer:Corrupt',
        'extract:decode:NeedsPassword', 'extract:decode:PasswordMismatchOrCorrupt',
        'extract:decode:MissingVolume', 'extract:decode:UnsafePath'
    )
    $taskDiagnosticLines = @($taskStderr -split '\r?\n' | Where-Object { $_ })
    Assert-Result ($taskDiagnosticLines.Count -eq $taskExpectedDiagnostics.Count) 'stderr reports exactly eight bounded negative-fixture diagnostics'
    for ($taskDiagnosticIndex = 0; $taskDiagnosticIndex -lt $taskDiagnosticLines.Count; $taskDiagnosticIndex++) {
        $taskLine = $taskDiagnosticLines[$taskDiagnosticIndex]
        Assert-Result (Test-SafeDiagnostic $taskLine $taskExpectedDiagnostics[$taskDiagnosticIndex]) "stderr diagnostic $taskDiagnosticIndex contains only bounded expected safe fields and values"
    }
    foreach ($taskMutation in @(
        @{ field = 'password'; value = 'SYNTHETIC-LEAK' },
        @{ field = 'operation'; value = 'C:\private\SYNTHETIC-LEAK' },
        @{ field = 'kind'; value = @('openunpackDiagnostic', 'SYNTHETIC-LEAK') },
        @{ field = 'version'; value = 2 }
    )) {
        $taskMutant = $taskDiagnosticLines[0] | ConvertFrom-Json -AsHashtable
        $taskMutant[$taskMutation.field] = $taskMutation.value
        Assert-Result (-not (Test-SafeDiagnostic ($taskMutant | ConvertTo-Json -Depth 5 -Compress) $taskExpectedDiagnostics[0])) "stderr validator rejects mutated $($taskMutation.field)"
    }
    Assert-Result (-not (Test-SafeDiagnostic 'unstructured SYNTHETIC-LEAK' $taskExpectedDiagnostics[0])) 'stderr validator rejects unstructured text'
    Assert-Result (-not (Test-SafeDiagnostic ('x' * 513) $taskExpectedDiagnostics[0])) 'stderr validator rejects oversized output'
    $taskDuplicateKey = $taskDiagnosticLines[0].Replace('{', '{"kind":"SYNTHETIC-LEAK",')
    Assert-Result (-not (Test-SafeDiagnostic $taskDuplicateKey $taskExpectedDiagnostics[0])) 'stderr validator rejects duplicate keys'
    $taskResults | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $taskFixture 'results.json') -Encoding utf8
    Write-Output ($taskResults | ConvertTo-Json -Depth 4)
} finally {
    if ($script:taskWriter) { $script:taskWriter.Dispose() }
    if ($script:taskReader) { $script:taskReader.Dispose() }
    $taskControl.Dispose(); $taskEvents.Dispose()
    if (-not $taskProcess.HasExited) { $taskProcess.Kill($true); $taskProcess.WaitForExit() }
    $taskProcess.Dispose()
}
