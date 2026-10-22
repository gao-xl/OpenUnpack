[CmdletBinding()]
param([string]$WorkerExecutable, [string]$EnginePath)
$ErrorActionPreference = 'Stop'
$taskRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if (-not $WorkerExecutable) { $WorkerExecutable=Join-Path $taskRoot 'src/OpenUnpack.Worker/bin/Release/net10.0-windows/OpenUnpack.Worker.exe' }
if (-not $EnginePath) { $EnginePath=Join-Path $taskRoot 'tools/7zip/7z.dll' }
$taskFolder = Join-Path $taskRoot ('artifacts/issue-fixes-2026-10-10/v2-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($taskFolder) | Out-Null
$taskInput = Join-Path $taskFolder 'public-fixture.txt'
[IO.File]::WriteAllText($taskInput, 'synthetic-only native protocol fixture')
$taskArchive = Join-Path $taskFolder 'encrypted.7z'
& (Join-Path $taskRoot 'tools/7zip/7z.exe') a $taskArchive $taskInput '-pSYNTHETIC-ONLY-V2' '-mhe=on' '-y' '-bso0' '-bsp0' '-bse0'
if ($LASTEXITCODE -ne 0) { throw 'Fixture creation failed.' }
$taskPrefix = 'verify-v2-' + [guid]::NewGuid().ToString('N')
$taskInfo = [Diagnostics.ProcessStartInfo]::new($WorkerExecutable)
$taskInfo.UseShellExecute = $false; $taskInfo.CreateNoWindow = $true
$taskInfo.RedirectStandardOutput = $true; $taskInfo.RedirectStandardError = $true
foreach ($taskArgument in @('--pipe-prefix',$taskPrefix,'--parent-pid',"$PID",'--engine',$EnginePath)) { $taskInfo.ArgumentList.Add($taskArgument) }
$taskProcess = [Diagnostics.Process]::Start($taskInfo)
$taskControl = [IO.Pipes.NamedPipeClientStream]::new('.',$taskPrefix+'-control',[IO.Pipes.PipeDirection]::InOut,[IO.Pipes.PipeOptions]::Asynchronous)
$taskEvents = [IO.Pipes.NamedPipeClientStream]::new('.',$taskPrefix+'-events',[IO.Pipes.PipeDirection]::InOut,[IO.Pipes.PipeOptions]::Asynchronous)
$taskChecks = [Collections.Generic.List[string]]::new()
function Assert-V2([bool]$Condition,[string]$Name) {
    if (-not $Condition) { throw "Failed: $Name" }; $script:taskChecks.Add($Name)
}
function Read-V2 {
    $taskRead=$script:taskReader.ReadLineAsync()
    if (-not $taskRead.Wait([TimeSpan]::FromSeconds(30))) { throw 'V2 event timeout.' }
    if ($null -eq $taskRead.Result) { throw 'V2 disconnected.' }
    $taskRead.Result | ConvertFrom-Json
}
function Send-V2([string]$Operation,[string]$OperationId='active-operation',[ulong]$Generation=7) {
    $script:taskWriter.WriteLine((@{protocolVersion=2;requestId='active-request';operation=$Operation;operationId=$OperationId;generation=$Generation}|ConvertTo-Json -Compress))
    $script:taskWriter.Flush()
}
try {
    $taskControl.Connect(15000); $taskEvents.Connect(15000)
    $script:taskWriter=[IO.StreamWriter]::new($taskControl,[Text.UTF8Encoding]::new($false),4096,$true)
    $script:taskReader=[IO.StreamReader]::new($taskEvents,[Text.UTF8Encoding]::new($false),$false,4096,$true)
    $taskReady=Read-V2
    Assert-V2 ($taskReady.protocolVersion -eq 1 -and $taskReady.kind -eq 'ready' -and $taskReady.hello.major -eq 2) 'compatible ready advertises Hello v2'
    Assert-V2 (-not $taskReady.hello.capabilities.extractWithCandidates -and -not $taskReady.hello.capabilities.solidOptimization -and $taskReady.hello.capabilities.maxBatchCandidates -eq 128) 'capabilities reflect implemented operations'
    $taskCandidates=@(0..127|ForEach-Object { @{candidateId="wrong-$_";source='vault';revision=1;label='synthetic';secret="SYNTHETIC-WRONG-$_"} })
    $taskRequest=@{protocolVersion=2;requestId='active-request';operation='probe';operationId='active-operation';generation=7;
        archivePath=$taskArchive;allowedVolumes=@($taskArchive);maxEntries=100;maxTotalBytes=1000000;maxWrittenBytes=1000000;candidates=$taskCandidates;candidatePasswords=@()}
    $script:taskWriter.WriteLine(($taskRequest|ConvertTo-Json -Depth 10 -Compress)); $script:taskWriter.Flush()
    Send-V2 'cancel' 'active-operation' 6
    Send-V2 'cancel' 'old-operation' 7
    Send-V2 'exit' 'active-operation' 6
    Send-V2 'ping' 'old-operation' 7
    Send-V2 'ping'
    $taskWrongGeneration=0; $taskWrongOperation=0; $taskPing=$false
    while ($taskWrongGeneration -lt 2 -or $taskWrongOperation -lt 2 -or -not $taskPing) {
        $taskEvent=Read-V2
        Assert-V2 ($taskEvent.protocolVersion -eq 2 -and $taskEvent.requestId -eq 'active-request' -and $null -eq $taskEvent.matchedPassword) 'v2 response identity and no returned secret'
        if ($taskEvent.kind -eq 'error' -and $taskEvent.errorCode -eq 'ProtocolError') {
            if ($taskEvent.generation -eq 6) { $taskWrongGeneration++ }
            elseif ($taskEvent.operationId -eq 'old-operation') { $taskWrongOperation++ }
        } elseif ($taskEvent.kind -eq 'heartbeat' -and $taskEvent.operationId -eq 'active-operation' -and $taskEvent.generation -eq 7) { $taskPing=$true }
        elseif ($taskEvent.kind -in @('completed','cancelled','error')) { throw 'Active operation terminated before valid cancellation.' }
    }
    Assert-V2 (-not $taskProcess.HasExited) 'wrong controls leave active Worker alive'
    Send-V2 'cancel'
    do { $taskTerminal=Read-V2 } while ($taskTerminal.kind -notin @('completed','error','cancelled'))
    Assert-V2 ($taskTerminal.kind -eq 'cancelled' -and $taskTerminal.generation -eq 7 -and $taskTerminal.operationId -eq 'active-operation') 'matching cancel terminates only active operation'
    $taskProbe=@{protocolVersion=2;requestId='active-request';operation='probe';operationId='active-operation';generation=8;
        archivePath=$taskArchive;allowedVolumes=@($taskArchive);maxEntries=100;maxTotalBytes=1000000;maxWrittenBytes=1000000;candidates=@(@{candidateId='saved-fixture';source='vault';revision=1;label='synthetic';secret='SYNTHETIC-ONLY-V2'});candidatePasswords=@()}
    $script:taskWriter.WriteLine(($taskProbe|ConvertTo-Json -Depth 10 -Compress)); $script:taskWriter.Flush()
    do { $taskMatched=Read-V2 } while ($taskMatched.kind -notin @('completed','error','cancelled'))
    Assert-V2 ($taskMatched.protocolVersion -eq 2 -and $taskMatched.kind -eq 'completed' -and $taskMatched.generation -eq 8 -and $taskMatched.matchedCandidateId -eq 'saved-fixture' -and $null -eq $taskMatched.matchedPassword) 'v2 probe returns candidate identity without secret'
    $taskDestination=Join-Path $taskFolder 'extracted'
    $taskExtract=@{protocolVersion=2;requestId='active-request';operation='extract';operationId='active-operation';generation=9;
        archivePath=$taskArchive;allowedVolumes=@($taskArchive);destinationDirectory=$taskDestination;password='SYNTHETIC-ONLY-V2';maxEntries=100;maxTotalBytes=1000000;maxWrittenBytes=1000000;minFreeBytes=0;exclusions=@()}
    $script:taskWriter.WriteLine(($taskExtract|ConvertTo-Json -Depth 10 -Compress)); $script:taskWriter.Flush()
    do { $taskExtracted=Read-V2 } while ($taskExtracted.kind -notin @('completed','error','cancelled'))
    Assert-V2 ($taskExtracted.kind -eq 'completed' -and $taskExtracted.generation -eq 9 -and $taskExtracted.passwordUsed) 'v2 extraction completes with correlated identity'
    $taskDelivered=Join-Path $taskDestination 'public-fixture.txt'
    Assert-V2 ([IO.File]::ReadAllText($taskDelivered) -eq [IO.File]::ReadAllText($taskInput)) 'v2 extracted bytes equal independent source'
    Send-V2 'exit' 'active-operation' 9
    Assert-V2 ($taskProcess.WaitForExit(10000)) 'matching exit confirms real Worker termination'
    Assert-V2 ($taskProcess.StandardOutput.ReadToEnd().Length -eq 0 -and $taskProcess.StandardError.ReadToEnd().Length -eq 0) 'stdout and stderr contain no request secrets'
    $taskChecks | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskFolder 'results.json') -Encoding utf8NoBOM
    Write-Output "PASS native v2 controls: $($taskChecks.Count) assertions. Evidence: $taskFolder"
} finally {
    if ($script:taskWriter) { $script:taskWriter.Dispose() }; if ($script:taskReader) { $script:taskReader.Dispose() }
    $taskControl.Dispose(); $taskEvents.Dispose()
    if (-not $taskProcess.HasExited) { $taskProcess.Kill($true); $taskProcess.WaitForExit() }
    $taskProcess.Dispose()
}
