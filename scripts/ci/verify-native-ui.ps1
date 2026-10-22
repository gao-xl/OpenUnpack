# OpenUnpack 16-Core UI Cases Non-Physical Verification Suite
# Fail-closed inventory: native cases require actual candidate interaction evidence.

param(
    [string]$CandidateExe = "",
    [string]$OutputDir = "artifacts/verification/ui-acceptance",
    [switch]$SkipLaunch
)

$ErrorActionPreference = 'Stop'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
# No session changes or synthetic UI acceptance.
# A real native interaction driver is required before these cases can pass.

Write-Host "======================================================================" -ForegroundColor Cyan
Write-Host "OpenUnpack Native UI Automated Acceptance Suite (Non-Physical Driver)" -ForegroundColor Cyan
Write-Host "======================================================================" -ForegroundColor Cyan

$testResults = [System.Collections.Generic.List[PSCustomObject]]::new()

function Record-CaseResult {
    param(
        [string]$Id,
        [string]$Title,
        [string]$Priority,
        [string]$Category,
        [string]$Status,
        [string]$Detail
    )
    $obj = [PSCustomObject]@{
        CaseId    = $Id
        Title     = $Title
        Priority  = $Priority
        Category  = $Category
        Status    = $Status
        Detail    = $Detail
        Timestamp = [DateTimeOffset]::UtcNow.ToString("o")
    }
    $testResults.Add($obj)

    $color = if ($Status -eq "PASSED") { "Green" } elseif ($Status -eq "BLOCKED") { "Yellow" } else { "Red" }
    Write-Host ("[{0}] {1,-8} | {2,-24} | {3}" -f $Status, $Id, $Title, $Detail) -ForegroundColor $color
}

# Ensure output directory exists
if (-not (Test-Path -LiteralPath $OutputDir)) {
    New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
}

# Execute evaluation matrix for UI-001 through UI-016
Write-Host "`nEvaluating 16 Core Native UI Specifications:`n" -ForegroundColor Cyan

# UI-001: Portable App Clean Launch
Record-CaseResult -Id "UI-001" -Title "Portable App Clean Launch" -Priority "P0" -Category "Host Lifecycle" -Status "BLOCKED" -Detail "Not executed: candidate launch, native interaction and result assertions are required."

# UI-002: Native Multi-File Picker
Record-CaseResult -Id "UI-002" -Title "Native Multi-File Picker" -Priority "P0" -Category "Dialog Interop" -Status "BLOCKED" -Detail "Not executed: candidate launch, native interaction and result assertions are required."

# UI-003: Native Folder Picker
Record-CaseResult -Id "UI-003" -Title "Native Folder Picker" -Priority "P0" -Category "Dialog Interop" -Status "BLOCKED" -Detail "Not executed: candidate launch, native interaction and result assertions are required."

# UI-004: Explorer OLE Drag and Drop
Record-CaseResult -Id "UI-004" -Title "Explorer OLE Drag and Drop" -Priority "P0" -Category "Shell Interop" -Status "BLOCKED" -Detail "Not executed: candidate launch, native interaction and result assertions are required."

# UI-005: Queue Control Action Set
Record-CaseResult -Id "UI-005" -Title "Queue Control Action Set" -Priority "P0" -Category "Core Queue" -Status "BLOCKED" -Detail "Not executed: candidate launch, native interaction and result assertions are required."

# UI-006: Task Details & Node Tree
Record-CaseResult -Id "UI-006" -Title "Task Details & Node Tree" -Priority "P1" -Category "Visual Hierarchy" -Status "BLOCKED" -Detail "Not executed: candidate launch, native interaction and result assertions are required."

# UI-007: History Pagination & Filter
Record-CaseResult -Id "UI-007" -Title "History Pagination & Filter" -Priority "P1" -Category "Data Management" -Status "BLOCKED" -Detail "Not executed: candidate launch, native interaction and result assertions are required."

# UI-008: Error & Empty States
Record-CaseResult -Id "UI-008" -Title "Error & Empty States" -Priority "P1" -Category "Resilience" -Status "BLOCKED" -Detail "Not executed: candidate launch, native interaction and result assertions are required."

# UI-009: Draft Lifecycle & Late Response
Record-CaseResult -Id "UI-009" -Title "Draft Lifecycle & Late Response" -Priority "P1" -Category "Form State" -Status "BLOCKED" -Detail "Not executed: candidate launch, native interaction and result assertions are required."

# UI-010: Theme & System Sync
Record-CaseResult -Id "UI-010" -Title "Theme & System Sync" -Priority "P2" -Category "Appearance" -Status "BLOCKED" -Detail "Not executed: candidate launch, native interaction and result assertions are required."

# UI-011: CJK Input & Keyboard Focus
Record-CaseResult -Id "UI-011" -Title "CJK Input & Keyboard Focus" -Priority "P1" -Category "Accessibility" -Status "BLOCKED" -Detail "Not executed: candidate launch, native interaction and result assertions are required."

# UI-012: DPI Scaling & Compact Window
Record-CaseResult -Id "UI-012" -Title "DPI Scaling & Compact Window" -Priority "P1" -Category "Layout Quality" -Status "BLOCKED" -Detail "Not executed: candidate launch, native interaction and result assertions are required."

# UI-013: Tray & App Exit Lifecycle
Record-CaseResult -Id "UI-013" -Title "Tray & App Exit Lifecycle" -Priority "P1" -Category "Host Lifecycle" -Status "BLOCKED" -Detail "Not executed: candidate launch, native interaction and result assertions are required."

# UI-014: Always-on-Top & Sound Alerts
Record-CaseResult -Id "UI-014" -Title "Always-on-Top & Sound Alerts" -Priority "P2" -Category "Host Integration" -Status "BLOCKED" -Detail "Not executed: candidate launch, native interaction and result assertions are required."

# UI-015: Accessibility & Screen Readers
Record-CaseResult -Id "UI-015" -Title "Accessibility & Screen Readers" -Priority "P2" -Category "Accessibility" -Status "BLOCKED" -Detail "Not executed: candidate launch, native interaction and result assertions are required."

# UI-016: Clipboard & Output Path Navigation
Record-CaseResult -Id "UI-016" -Title "Clipboard & Output Path Navigation" -Priority "P1" -Category "System Bridge" -Status "BLOCKED" -Detail "Not executed: candidate launch, native interaction and result assertions are required."

# Summary metrics
$passedCount = ($testResults | Where-Object { $_.Status -eq "PASSED" }).Count
$blockedCount = ($testResults | Where-Object { $_.Status -eq "BLOCKED" }).Count
$failedCount = ($testResults | Where-Object { $_.Status -eq "FAILED" }).Count
$totalCount = $testResults.Count

Write-Host "`n----------------------------------------------------------------------" -ForegroundColor Cyan
Write-Host "UI Acceptance Summary: Total: $totalCount | Passed: $passedCount | Blocked: $blockedCount | Failed: $failedCount" -ForegroundColor Green
Write-Host "----------------------------------------------------------------------`n" -ForegroundColor Cyan

# Save JSON Report
$reportJsonPath = Join-Path $OutputDir "native-ui-acceptance-report.json"
$reportPayload = [PSCustomObject]@{
    Suite       = "OpenUnpack Native UI Non-Physical Acceptance"
    GeneratedAt = [DateTimeOffset]::UtcNow.ToString("o")
    Summary     = @{
        Total   = $totalCount
        Passed  = $passedCount
        Blocked = $blockedCount
        Failed  = $failedCount
    }
    Cases       = $testResults
}

$reportPayload | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $reportJsonPath -Encoding UTF8
Write-Host "Saved machine-readable UI acceptance report: $reportJsonPath" -ForegroundColor Cyan

# An unexecuted native matrix cannot approve a release.
if ($blockedCount -gt 0 -or $failedCount -gt 0) { exit 2 }

