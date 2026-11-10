#requires -Version 5.1
<#
.SYNOPSIS
  Removes Microsoft Copilot from Windows 10 and blocks the usual ways it comes back.

.DESCRIPTION
  Policy (WindowsAI Policy CSP, deprecated by Microsoft but still honoured for the legacy Copilot pane):
    HKCU\Software\Policies\Microsoft\Windows\WindowsCopilot  TurnOffWindowsCopilot = 1 (DWORD)  [user-scope policy]
    HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsCopilot  TurnOffWindowsCopilot = 1 (DWORD)  [belt-and-braces]
  Taskbar button (Win10 22H2 Copilot preview, KB5032278+):
    HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced  ShowCopilotButton = 0 (DWORD)
  Edge sidebar (where Copilot lives in Edge; documented Edge policy):
    HKLM\SOFTWARE\Policies\Microsoft\Edge  HubsSidebarEnabled = 0 (DWORD)
  Apps (Microsoft Learn "Remove or prevent installation of the Microsoft Copilot app"):
    Stops Copilot processes, then removes Appx package Microsoft.Copilot for all users + its provisioned copy.
    Also removes Microsoft.MicrosoftOfficeHub ("Microsoft 365 Copilot" launcher; Office apps are NOT touched)
    unless -KeepMicrosoft365App.
  Keep-gone (Microsoft Learn "Keep removed apps from returning during an update"):
    HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Appx\AppxAllUserStore\Deprovisioned\<PackageFamilyName>  (empty key)
  Store auto-updates are turned off by Disable-AutomaticUpdates.ps1 (WindowsStore\AutoDownload=2).

  NOT done: AppLocker rule (Microsoft's recommended block). It needs AppIDSvc + an AppLocker policy; doable on Pro
  via secpol.msc, but out of scope for an idempotent registry kit - see README.
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param([switch]$KeepMicrosoft365App)

. (Join-Path $PSScriptRoot 'TweakCommon.ps1')
if (Invoke-SelfElevate -ScriptPath $PSCommandPath -BoundParameters $PSBoundParameters) { return }
Initialize-TweakLog -Name 'Remove-Copilot'
$cat = 'Copilot'

Write-TweakLog '--- Policies / registry ---'
Set-TweakRegistry -Path 'HKCU:\Software\Policies\Microsoft\Windows\WindowsCopilot' -Name 'TurnOffWindowsCopilot' -Type DWord -Value 1 -Category $cat
Set-TweakRegistry -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsCopilot' -Name 'TurnOffWindowsCopilot' -Type DWord -Value 1 -Category $cat
Set-TweakRegistry -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced' -Name 'ShowCopilotButton' -Type DWord -Value 0 -Category $cat -Note 'taskbar button'
Set-TweakRegistry -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Edge' -Name 'HubsSidebarEnabled' -Type DWord -Value 0 -Category $cat -Note 'Edge sidebar / Copilot pane'

$packages = @(@{ Name = 'Microsoft.Copilot'; Family = 'Microsoft.Copilot_8wekyb3d8bbwe' })
if (-not $KeepMicrosoft365App) { $packages += @{ Name = 'Microsoft.MicrosoftOfficeHub'; Family = 'Microsoft.MicrosoftOfficeHub_8wekyb3d8bbwe' } }

Write-TweakLog '--- Processes ---'
foreach ($procName in @('Copilot', 'Microsoft.Copilot', 'M365Copilot')) {
    foreach ($p in @(Get-Process -Name $procName -ErrorAction SilentlyContinue)) {
        if ($PSCmdlet.ShouldProcess("Process $($p.Name) ($($p.Id))", 'Stop')) {
            try { Stop-Process -Id $p.Id -Force -ErrorAction Stop; Write-TweakLog "Process $($p.Name) ($($p.Id)) : Running -> Stopped" 'SET' }
            catch { Write-TweakLog "Process $($p.Name): $($_.Exception.Message)" 'WARN' }
        } else { Write-TweakLog "Process $($p.Name) ($($p.Id)) : Running -> Stopped" 'WHATIF' }
    }
}

Write-TweakLog '--- Appx packages ---'
foreach ($pkg in $packages) {
    $installed = @(Get-AppxPackage -AllUsers -Name $pkg.Name -ErrorAction SilentlyContinue)
    if ($installed.Count -eq 0) { Write-TweakLog "Appx $($pkg.Name) not installed for any user" 'SKIP' }
    foreach ($i in $installed) {
        if ($PSCmdlet.ShouldProcess("Appx $($i.PackageFullName)", 'Remove for all users')) {
            try {
                Remove-AppxPackage -Package $i.PackageFullName -AllUsers -ErrorAction Stop
                Write-TweakLog "Appx $($i.PackageFullName) : installed -> removed (all users)" 'SET'
            } catch {
                try { Remove-AppxPackage -Package $i.PackageFullName -ErrorAction Stop; Write-TweakLog "Appx $($i.PackageFullName) : installed -> removed (current user only)" 'SET' }
                catch { Write-TweakLog "Appx $($i.PackageFullName) : remove failed ($($_.Exception.Message))" 'ERROR' }
            }
        } else { Write-TweakLog "Appx $($i.PackageFullName) : installed -> removed" 'WHATIF' }
    }
    $prov = @(Get-AppxProvisionedPackage -Online -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -eq $pkg.Name })
    foreach ($pp in $prov) {
        if ($PSCmdlet.ShouldProcess("Provisioned $($pp.PackageName)", 'Remove')) {
            try { Remove-AppxProvisionedPackage -Online -PackageName $pp.PackageName -ErrorAction Stop | Out-Null; Write-TweakLog "Provisioned $($pp.PackageName) : present -> removed" 'SET' }
            catch { Write-TweakLog "Provisioned $($pp.PackageName) : remove failed ($($_.Exception.Message))" 'ERROR' }
        } else { Write-TweakLog "Provisioned $($pp.PackageName) : present -> removed" 'WHATIF' }
    }
    New-TweakRegistryKey -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Appx\AppxAllUserStore\Deprovisioned\$($pkg.Family)" -Category $cat
}

Write-TweakLog 'Restart Explorer or sign out for the taskbar button change to show.' 'INFO'
Write-TweakSummary -Name 'Remove-Copilot'
