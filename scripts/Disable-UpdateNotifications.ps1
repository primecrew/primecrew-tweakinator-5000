#requires -Version 5.1
<#
.SYNOPSIS
  Silences Windows Update toasts, restart nags and "finish setting up your device" prompts.

.DESCRIPTION
  Documented policy ("Display options for update notifications", WindowsUpdate.admx, Win10 1809+):
    HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate
      SetUpdateNotificationLevel = 1 (DWORD)  policy enabled
      UpdateNotificationLevel    = 2 (DWORD)  0=default 1=all off except restart warnings 2=all off incl. restart warnings
  Settings-app toggles (not policy; written the same way the Settings UI writes them):
    HKLM\SOFTWARE\Microsoft\WindowsUpdate\UX\Settings  RestartNotificationsAllowed2 = 0  ("Show a notification when your PC requires a restart")
    HKCU\Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement  ScoobeSystemSettingEnabled = 0  ("Suggest ways I can finish setting up my device")
  Task:
    \Microsoft\Windows\UpdateOrchestrator\USO_UxBroker (launches MusNotification toasts) -> Disabled (may be refused; logged as WARN)

  Microsoft's own warning applies: with notifications off AND updates off, you will not know about critical patches.

.PARAMETER Level
  UpdateNotificationLevel value. 2 (default) hides restart warnings too; 1 keeps restart warnings.
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param([ValidateSet(1, 2)][int]$Level = 2)

. (Join-Path $PSScriptRoot 'TweakCommon.ps1')
if (Invoke-SelfElevate -ScriptPath $PSCommandPath -BoundParameters $PSBoundParameters) { return }
Initialize-TweakLog -Name 'Disable-UpdateNotifications'
$cat = 'Notifications'
$WU = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate'

Set-TweakRegistry -Path $WU -Name 'SetUpdateNotificationLevel' -Type DWord -Value 1      -Category $cat -Note 'Display options for update notifications: enabled'
Set-TweakRegistry -Path $WU -Name 'UpdateNotificationLevel'    -Type DWord -Value $Level -Category $cat -Note '2 = all WU notifications off incl. restart warnings'
Set-TweakRegistry -Path 'HKLM:\SOFTWARE\Microsoft\WindowsUpdate\UX\Settings' -Name 'RestartNotificationsAllowed2' -Type DWord -Value 0 -Category $cat -Note 'Settings toggle, not a policy'
Set-TweakRegistry -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement' -Name 'ScoobeSystemSettingEnabled' -Type DWord -Value 0 -Category $cat -Note 'Finish-setting-up nag (per user)'
Disable-TweakTask -TaskPath '\Microsoft\Windows\UpdateOrchestrator\' -TaskName 'USO_UxBroker' -Category $cat

Write-TweakSummary -Name 'Disable-UpdateNotifications'
