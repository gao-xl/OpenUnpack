# OpenUnpack CI Session Keep-Alive and Interactive Desktop Probe
# Resolves Win32 GetCursorPos 0x80070005 (Access Denied) in unattended CI environments

param(
    [switch]$RedirectRdpToConsole,
    [switch]$DisableLockScreen,
    [switch]$Quiet
)

$ErrorActionPreference = 'Stop'

function Write-SessionInfo([string]$Message) {
    if (-not $Quiet) {
        Write-Host "[CI-Session] $Message" -ForegroundColor Cyan
    }
}

function Write-SessionSuccess([string]$Message) {
    if (-not $Quiet) {
        Write-Host "[CI-Session] $Message" -ForegroundColor Green
    }
}

function Write-SessionWarn([string]$Message) {
    Write-Host "[CI-Session] WARN: $Message" -ForegroundColor Yellow
}

Write-SessionInfo "Probing current Windows host session environment..."

# 1. Inspect Session ID and SessionName
$currentProcess = Get-Process -Id $PID
$sessionId = $currentProcess.SessionId
$sessionName = [System.Environment]::GetEnvironmentVariable("SESSIONNAME")
if ([string]::IsNullOrWhiteSpace($sessionName)) {
    $sessionName = "Console/Local"
}

Write-SessionInfo "Current Process PID: $PID, Session ID: $sessionId, SessionName: $sessionName"

if ($sessionId -eq 0) {
    Write-SessionWarn "Running in Windows Session 0 isolation environment."
    Write-SessionWarn "Physical cursor and GUI subsystems are not available in Session 0."
}

# 2. RDP Session Redirection to physical console if requested
if ($RedirectRdpToConsole -and ($sessionName -like "RDP-Tcp*" -or $sessionName -like "rdp*")) {
    Write-SessionInfo "Active RDP session detected. Attempting tscon redirection to console..."
    try {
        $tsconPath = Join-Path $env:SystemRoot "System32\tscon.exe"
        if (Test-Path -LiteralPath $tsconPath) {
            & $tsconPath $sessionId /dest:console
            Write-SessionSuccess "Redirected session $sessionId to console."
        }
    } catch {
        Write-SessionWarn "tscon redirection failed: $_"
    }
}

# 3. Disable screensaver and lock screen timeout
if ($DisableLockScreen) {
    try {
        Set-ItemProperty -Path 'HKCU:\Control Panel\Desktop' -Name 'ScreenSaveActive' -Value '0' -ErrorAction SilentlyContinue
        Set-ItemProperty -Path 'HKCU:\Control Panel\Desktop' -Name 'ScreenSaverIsSecure' -Value '0' -ErrorAction SilentlyContinue
        Write-SessionSuccess "Disabled screensaver and secure lock screen in current user profile."
    } catch {
        Write-SessionWarn "Failed to update screensaver registry: $_"
    }
}

# 4. Probe Win32 Cursor Access (detect if 0x80070005 occurs)
$csharpSource = @'
using System;
using System.Runtime.InteropServices;

public static class SessionCursorProbe {
    [StructLayout(LayoutKind.Sequential)]
    public struct POINT { public int X; public int Y; }

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool GetCursorPos(out POINT lpPoint);

    [DllImport("user32.dll")]
    public static extern IntPtr GetDesktopWindow();

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();
}
'@

try {
    Add-Type -TypeDefinition $csharpSource -ErrorAction SilentlyContinue
} catch {}

$cursorPoint = New-Object SessionCursorProbe+POINT
$cursorOk = [SessionCursorProbe]::GetCursorPos([ref]$cursorPoint)
$lastError = [System.Runtime.InteropServices.Marshal]::GetLastWin32Error()

$probeResult = [PSCustomObject]@{
    Timestamp          = [DateTimeOffset]::UtcNow.ToString("o")
    SessionId          = $sessionId
    SessionName        = $sessionName
    IsSession0         = ($sessionId -eq 0)
    CanAccessCursor    = $cursorOk
    CursorX            = if ($cursorOk) { $cursorPoint.X } else { $null }
    CursorY            = if ($cursorOk) { $cursorPoint.Y } else { $null }
    LastWin32Error     = $lastError
    DesktopWindow      = [SessionCursorProbe]::GetDesktopWindow().ToInt64()
    ForegroundWindow   = [SessionCursorProbe]::GetForegroundWindow().ToInt64()
}

if ($cursorOk) {
    Write-SessionSuccess "Probe SUCCESS: Win32 desktop is unlocked. GetCursorPos is accessible at ($($cursorPoint.X), $($cursorPoint.Y))."
} else {
    Write-SessionWarn "Probe FAILED: GetCursorPos returned error: $lastError (5 = Access Denied / 0x80070005)."
    Write-SessionWarn "Desktop is locked (Winlogon Desktop), RDP is disconnected, or session is in Session 0."
}

return $probeResult
