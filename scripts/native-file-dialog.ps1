param([int]$AppPid,[string]$SelectedPath,[switch]$Cancel)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class FeatureDialogNative {
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr window,uint message,IntPtr wParam,string text);
 [DllImport("user32.dll", EntryPoint="SendMessageW")] public static extern IntPtr SendMessage(IntPtr window,uint message,IntPtr wParam,IntPtr lParam);
 [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr window);
 [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr window);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window,out uint pid);
 [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr window);
 [DllImport("user32.dll")] public static extern bool IsChild(IntPtr parent,IntPtr child);
}
'@
$featureRoot=[Windows.Automation.AutomationElement]::RootElement
$featureProcessCondition=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ProcessIdProperty,$AppPid)
$featureEditCondition=[Windows.Automation.AndCondition]::new([Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::AutomationIdProperty,'1148'),[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Edit))
$featureDeadline=[DateTimeOffset]::UtcNow.AddSeconds(15)
do {
 foreach($featureWindow in $featureRoot.FindAll([Windows.Automation.TreeScope]::Children,$featureProcessCondition)) {
  if($featureWindow.Current.ClassName -ne '#32770'){continue}
  $featureEdit=$featureWindow.FindFirst([Windows.Automation.TreeScope]::Descendants,$featureEditCondition)
  if($null -eq $featureEdit){continue}
  if($Cancel){
   $featureCancelCondition=[Windows.Automation.AndCondition]::new([Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::AutomationIdProperty,'2'),[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Button),[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ClassNameProperty,'Button'))
   $featureCancelButtons=$featureWindow.FindAll([Windows.Automation.TreeScope]::Descendants,$featureCancelCondition)
   if($featureCancelButtons.Count -ne 1){throw 'Expected one native Cancel button.'}
   $featureCancelHandle=[IntPtr]$featureCancelButtons[0].Current.NativeWindowHandle
   [uint32]$featureCancelPid=0
   $null=[FeatureDialogNative]::GetWindowThreadProcessId($featureCancelHandle,[ref]$featureCancelPid)
   if($featureCancelHandle -eq [IntPtr]::Zero -or $featureCancelPid -ne $AppPid -or [FeatureDialogNative]::GetDlgCtrlID($featureCancelHandle) -ne 2 -or -not [FeatureDialogNative]::IsChild([IntPtr]$featureWindow.Current.NativeWindowHandle,$featureCancelHandle) -or -not [FeatureDialogNative]::IsWindowEnabled($featureCancelHandle)){throw 'Cancel button ownership, native ID or enabled state mismatch.'}
   $null=[FeatureDialogNative]::SendMessage($featureCancelHandle,0x00F5,[IntPtr]::Zero,[IntPtr]::Zero)
   Write-Output 'Confirmed actual native Cancel button; caller must verify no path returned.'
   exit 0
  }
  $featureHandle=[IntPtr]$featureEdit.Current.NativeWindowHandle
  [uint32]$featureOwnerPid=0
  $null=[FeatureDialogNative]::GetWindowThreadProcessId($featureHandle,[ref]$featureOwnerPid)
  if($featureHandle -eq [IntPtr]::Zero -or $featureOwnerPid -ne $AppPid){throw 'Filename control ownership mismatch.'}
  Write-Output ('Filename control class: ' + $featureEdit.Current.ClassName + ', native control ID: ' + [FeatureDialogNative]::GetDlgCtrlID($featureHandle))
  try{$featureEdit.SetFocus()}catch{}
  $null=[FeatureDialogNative]::SendMessage($featureHandle,0x000C,[IntPtr]::Zero,'')
  foreach($featureCharacter in $SelectedPath.ToCharArray()) { $null=[FeatureDialogNative]::SendMessage($featureHandle,0x0102,[IntPtr][int]$featureCharacter,[IntPtr]1) }
  $featureParent=[FeatureDialogNative]::GetParent($featureHandle)
  $featureControlId=[FeatureDialogNative]::GetDlgCtrlID($featureHandle)
  $null=[FeatureDialogNative]::SendMessage($featureParent,0x0111,[IntPtr]((0x0300 -shl 16) -bor ($featureControlId -band 0xFFFF)),$featureHandle)
  Start-Sleep -Milliseconds 300
  $featureActual=$featureEdit.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).Current.Value
  if($featureActual -ne $SelectedPath){throw 'Filename editor did not retain exact synthetic path.'}
  $featureButtonCondition=[Windows.Automation.AndCondition]::new([Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::AutomationIdProperty,'1'),[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Button),[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ClassNameProperty,'Button'))
  $featureButtons=$featureWindow.FindAll([Windows.Automation.TreeScope]::Descendants,$featureButtonCondition)
  if($featureButtons.Count -ne 1){throw 'Expected one native Open button.'}
  $featureButton=$featureButtons[0]
  $featureButtonHandle=[IntPtr]$featureButton.Current.NativeWindowHandle
  [uint32]$featureButtonPid=0
  $null=[FeatureDialogNative]::GetWindowThreadProcessId($featureButtonHandle,[ref]$featureButtonPid)
  if($featureButtonHandle -eq [IntPtr]::Zero -or $featureButtonPid -ne $AppPid -or [FeatureDialogNative]::GetDlgCtrlID($featureButtonHandle) -ne 1 -or -not [FeatureDialogNative]::IsChild([IntPtr]$featureWindow.Current.NativeWindowHandle,$featureButtonHandle)){throw 'Open button ownership or native ID mismatch.'}
  Write-Output ('Confirmed button: ' + $featureButton.Current.Name + '; type: ' + $featureButton.Current.ControlType.ProgrammaticName + '; native ID: ' + [FeatureDialogNative]::GetDlgCtrlID($featureButtonHandle))
  Write-Output ('Open enabled: UIA=' + $featureButton.Current.IsEnabled + '; native=' + [FeatureDialogNative]::IsWindowEnabled($featureButtonHandle))
  if(-not [FeatureDialogNative]::IsWindowEnabled($featureButtonHandle)){throw 'Native Open button is disabled.'}
  $null=[FeatureDialogNative]::SendMessage($featureButtonHandle,0x00F5,[IntPtr]::Zero,[IntPtr]::Zero)
  Write-Output 'Native filename edit and confirmation submitted; caller must verify exact Host result.'
  exit 0
 }
 Start-Sleep -Milliseconds 150
}while([DateTimeOffset]::UtcNow -lt $featureDeadline)
throw 'Native filename control unavailable.'
