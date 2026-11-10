#requires -Version 5.1
<#
.SYNOPSIS
  Permanently disables automatic Windows Update on Windows 10 (Home/Pro) without gpedit.

.DESCRIPTION
  Layer 1 (documented policy, Microsoft Learn "Configure Automatic Updates by editing the registry"):
    HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU
      NoAutoUpdate                  = 1  (DWORD)  Automatic Updates disabled
      AUOptions                     = 1  (DWORD)  "Keep my computer up to date" disabled
      NoAutoRebootWithLoggedOnUsers = 1  (DWORD)  no auto-restart while signed in
    HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate
      SetDisableUXWUAccess          = 1  (DWORD)  removes "Check for updates" in Settings (Policy CSP Update/SetDisableUXWUAccess)
    HKLM\SOFTWARE\Policies\Microsoft\WindowsStore
      AutoDownload                  = 2  (DWORD)  Store: turn off automatic download/install of app updates
  Layer 2 (hard kill, skipped with -PolicyOnly):
    No phone home : DoNotConnectToWindowsUpdateInternetLocations=1, DisableWindowsUpdateAccess=1 (both documented to apply
                    only with an intranet update server, so WUServer/WUStatusServer=http://127.0.0.1:8530 + AU\UseWUServer=1),
                    DeliveryOptimization\DODownloadMode=99 (HTTP only, no peering, no DO cloud service).
    Tasks    : every task in \Microsoft\Windows\WaaSMedic\, \UpdateOrchestrator\, \WindowsUpdate\ disabled; protected ones are
               disabled once as SYSTEM through a throw-away scheduled task (no task-file or ACL changes).
    Services : WaaSMedicSvc, uhssvc (if installed), UsoSvc, wuauserv -> Start=4, stopped.
               WaaSMedicSvc: its REGISTRY KEY (HKLM\SYSTEM\CurrentControlSet\Services\WaaSMedicSvc, never the binary) is taken
               over: owner -> Administrators, Administrators FullControl, SYSTEM denied "Set Value" on that key only, so Medic /
               servicing can't write Start back. Original owner + SDDL saved in state.json; Revert-Tweaks.ps1 -Updates restores them.
    Firewall : outbound Block rules Win10Tweaks-NoWU-<service> for wuauserv, UsoSvc, WaaSMedicSvc, uhssvc.

  TRADE-OFFS (read before running):
    * With wuauserv disabled, NO updates install: not cumulative updates, not consumer ESU security
      patches, not Microsoft Store app installs/updates, not Defender definitions via Windows Update.
      Use 'Update-MpSignature -UpdateSource MMPC' for Defender definitions.
    * To patch manually: Revert-Tweaks.ps1 -Updates, update, then re-run this script.

.PARAMETER PolicyOnly
  Only write the policy values; leave services and scheduled tasks alone.
.EXAMPLE
  powershell -ExecutionPolicy Bypass -File .\scripts\Disable-AutomaticUpdates.ps1 -WhatIf
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param([switch]$PolicyOnly)

. (Join-Path $PSScriptRoot 'TweakCommon.ps1')
if (Invoke-SelfElevate -ScriptPath $PSCommandPath -BoundParameters $PSBoundParameters) { return }
Initialize-TweakLog -Name 'Disable-AutomaticUpdates'
$cat = 'Updates'

$WU = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate'
$AU = "$WU\AU"

Write-TweakLog '--- Layer 1: Windows Update policy values ---'
Set-TweakRegistry -Path $AU -Name 'NoAutoUpdate'                  -Type DWord -Value 1 -Category $cat -Note 'Automatic Updates disabled'
Set-TweakRegistry -Path $AU -Name 'AUOptions'                     -Type DWord -Value 1 -Category $cat -Note 'Keep my computer up to date: disabled'
Set-TweakRegistry -Path $AU -Name 'NoAutoRebootWithLoggedOnUsers' -Type DWord -Value 1 -Category $cat -Note 'No auto-restart with signed-in users'
Set-TweakRegistry -Path $WU -Name 'SetDisableUXWUAccess'          -Type DWord -Value 1 -Category $cat -Note 'Hide Check for updates UI'
Set-TweakRegistry -Path 'HKLM:\SOFTWARE\Policies\Microsoft\WindowsStore' -Name 'AutoDownload' -Type DWord -Value 2 -Category $cat -Note 'Store auto app updates off (stops Store re-pushing Copilot)'

if ($PolicyOnly) {
    Write-TweakLog '-PolicyOnly: services and tasks left untouched.' 'INFO'
} else {
    Write-TweakLog '--- Layer 2: no-phone-home policies ---'
    foreach ($e in Get-TweakManifest | Where-Object { $_.Category -eq 'Updates' -and $_.Level -eq 'Hard' }) {
        Set-TweakRegistry -Path $e.Path -Name $e.Name -Type $e.Type -Value $e.Value -Category $cat
    }
    Write-TweakLog '--- Layer 2: scheduled tasks that rescan / repair / reboot ---'
    # Tasks first, so Medic/Orchestrator cannot immediately re-enable the services we disable next.
    Disable-TweakTaskFolder -TaskPath '\Microsoft\Windows\WaaSMedic\'          -Category $cat -Force
    Disable-TweakTaskFolder -TaskPath '\Microsoft\Windows\UpdateOrchestrator\' -Category $cat -Force
    Disable-TweakTaskFolder -TaskPath '\Microsoft\Windows\WindowsUpdate\'      -Category $cat -Force

    Write-TweakLog '--- Layer 2: services (forced) ---'
    foreach ($svc in Get-TweakServiceManifest) { Set-TweakServiceStart -Name $svc.Name -Start 4 -Category $cat -StopNow -Force }

    Write-TweakLog '--- Layer 2: firewall ---'
    Set-TweakFirewallBlock
    Write-TweakLog 'Settings > Update & Security > Windows Update will show errors / be blank while UsoSvc+wuauserv are disabled. That is expected.' 'INFO'
}

Set-TweakUpdatesMode -PolicyOnly ([bool]$PolicyOnly)
Clear-TweakTempAllow
Write-TweakLog 'Reboot so WaaSMedicSvc / policy changes made via the registry take effect.' 'INFO'
Write-TweakSummary -Name 'Disable-AutomaticUpdates'
