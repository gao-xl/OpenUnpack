# OpenUnpack Non-Physical Native UI Automation Driver
# Replaces fragile OS-level mouse coordinates (GetCursorPos/SetCursorPos) with
# Windows UI Automation (UIA) Control Patterns and Win32 direct window messages.

Add-Type -AssemblyName UIAutomationClient -ErrorAction SilentlyContinue
Add-Type -AssemblyName UIAutomationTypes -ErrorAction SilentlyContinue

$win32Interop = @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class NativeUiInterop {
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessage(IntPtr hWnd, uint Msg, IntPtr wParam, string lParam);

    [DllImport("user32.dll", EntryPoint = "SendMessageW")]
    public static extern IntPtr SendMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern IntPtr GetParent(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern int GetDlgCtrlID(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);

    [DllImport("user32.dll")]
    public static extern bool IsWindowEnabled(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)]
    public static extern int GetClassName(IntPtr hWnd, StringBuilder lpClassName, int nMaxCount);

    // Common Windows Messages
    public const uint WM_COMMAND    = 0x0111;
    public const uint WM_SETTEXT    = 0x000C;
    public const uint WM_GETTEXT    = 0x000D;
    public const uint WM_CHAR       = 0x0102;
    public const uint BM_CLICK      = 0x00F5;
    public const uint WM_CLOSE      = 0x0010;
    public const uint WM_DROPFILES  = 0x0233;
}
'@

try {
    Add-Type -TypeDefinition $win32Interop -ErrorAction SilentlyContinue
} catch {}

function Find-ApplicationWindow {
    param(
        [int]$ProcessId,
        [int]$TimeoutSeconds = 15
    )
    $root = [System.Windows.Automation.AutomationElement]::RootElement
    $procCond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ProcessIdProperty,
        $ProcessId
    )
    $deadline = [DateTimeOffset]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        $windows = $root.FindAll([System.Windows.Automation.TreeScope]::Children, $procCond)
        foreach ($w in $windows) {
            $hwnd = [IntPtr]$w.Current.NativeWindowHandle
            if ($hwnd -ne [IntPtr]::Zero -and [NativeUiInterop]::IsWindowVisible($hwnd)) {
                return $w
            }
        }
        Start-Sleep -Milliseconds 200
    } while ([DateTimeOffset]::UtcNow -lt $deadline)
    return $null
}

function Invoke-UIElementSafely {
    param(
        [System.Windows.Automation.AutomationElement]$Element,
        [string]$Description = "UI Action"
    )
    if ($null -eq $Element) {
        throw "Element for '$Description' cannot be null."
    }

    # 1. Try UIA InvokePattern (Preferred: Works across DPI, locked desktop, and occlusion)
    $invokePattern = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$invokePattern)) {
        $invokePattern.Invoke()
        Write-Verbose "[$Description] Successfully triggered via UIA InvokePattern."
        return $true
    }

    # 2. Try TogglePattern (For checkboxes and switches)
    $togglePattern = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$togglePattern)) {
        $togglePattern.Toggle()
        Write-Verbose "[$Description] Successfully triggered via UIA TogglePattern."
        return $true
    }

    # 3. Try SelectionItemPattern (For list items, tabs, and radio buttons)
    $selectionPattern = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$selectionPattern)) {
        $selectionPattern.Select()
        Write-Verbose "[$Description] Successfully triggered via UIA SelectionItemPattern."
        return $true
    }

    # 4. Fallback to Window Message BM_CLICK
    $hwnd = [IntPtr]$Element.Current.NativeWindowHandle
    if ($hwnd -ne [IntPtr]::Zero) {
        $null = [NativeUiInterop]::SendMessage($hwnd, [NativeUiInterop]::BM_CLICK, [IntPtr]::Zero, [IntPtr]::Zero)
        Write-Verbose "[$Description] Successfully sent Win32 BM_CLICK to hwnd 0x$($hwnd.ToString('X'))."
        return $true
    }

    throw "Element '$Description' does not support non-physical programmatic invocation."
}

function Set-UIElementTextSafely {
    param(
        [System.Windows.Automation.AutomationElement]$Element,
        [string]$Text,
        [string]$Description = "Text Edit"
    )
    if ($null -eq $Element) {
        throw "Element for '$Description' cannot be null."
    }

    # 1. Try UIA ValuePattern
    $valuePattern = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$valuePattern)) {
        $valuePattern.SetValue($Text)
        Write-Verbose "[$Description] Set text via UIA ValuePattern: $Text"
        return $true
    }

    # 2. Fallback to Win32 WM_SETTEXT
    $hwnd = [IntPtr]$Element.Current.NativeWindowHandle
    if ($hwnd -ne [IntPtr]::Zero) {
        $null = [NativeUiInterop]::SendMessage($hwnd, [NativeUiInterop]::WM_SETTEXT, [IntPtr]::Zero, $Text)
        Write-Verbose "[$Description] Set text via Win32 WM_SETTEXT: $Text"
        return $true
    }

    throw "Element '$Description' does not support text setting."
}

function Submit-NativeFileDialog {
    param(
        [int]$AppProcessId,
        [string]$FilePath,
        [switch]$Cancel,
        [int]$TimeoutSeconds = 15
    )
    $root = [System.Windows.Automation.AutomationElement]::RootElement
    $procCond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ProcessIdProperty,
        $AppProcessId
    )
    $editCond = New-Object System.Windows.Automation.AndCondition(
        (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, '1148')),
        (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Edit))
    )

    $deadline = [DateTimeOffset]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        foreach ($dialog in $root.FindAll([System.Windows.Automation.TreeScope]::Children, $procCond)) {
            if ($dialog.Current.ClassName -ne '#32770') { continue }
            
            # Handle Cancel action
            if ($Cancel) {
                $cancelCond = New-Object System.Windows.Automation.AndCondition(
                    (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, '2')),
                    (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button))
                )
                $btnCancel = $dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cancelCond)
                if ($btnCancel) {
                    Invoke-UIElementSafely -Element $btnCancel -Description "FileDialog Cancel Button"
                    return @{ Success = $true; Action = "Cancelled" }
                }
            }

            # Handle Open/Select action
            $edit = $dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $editCond)
            if ($null -ne $edit) {
                Set-UIElementTextSafely -Element $edit -Text $FilePath -Description "FileDialog Path Edit"
                Start-Sleep -Milliseconds 200

                $openCond = New-Object System.Windows.Automation.AndCondition(
                    (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, '1')),
                    (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button))
                )
                $btnOpen = $dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $openCond)
                if ($btnOpen) {
                    Invoke-UIElementSafely -Element $btnOpen -Description "FileDialog Open Button"
                    return @{ Success = $true; Action = "Selected"; Path = $FilePath }
                }
            }
        }
        Start-Sleep -Milliseconds 200
    } while ([DateTimeOffset]::UtcNow -lt $deadline)

    return @{ Success = $false; Error = "Native file dialog (#32770) not found within timeout." }
}

Write-Host "[NativeUI-Driver] Initialized non-physical UIA and Win32 message driver successfully." -ForegroundColor Green
