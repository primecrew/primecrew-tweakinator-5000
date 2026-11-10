#requires -Version 5.1
<#
.SYNOPSIS
  Pins the PC to Windows 10 22H2 (the final Windows 10 feature update) and blocks the Windows 11 offer.

.DESCRIPTION
  Primary, documented (Policy CSP Update/ProductVersion + Update/TargetReleaseVersion,
  GPO "Select the target Feature Update version"):
    HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate
      TargetReleaseVersion     = 1            (DWORD)  policy enabled
      ProductVersion           = "Windows 10" (REG_SZ)
      TargetReleaseVersionInfo = "22H2"       (REG_SZ)
  Legacy belt-and-braces (from the Win7/8.1 "OS upgrade" controls; harmless if ignored):
      HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\DisableOSUpgrade            = 1 (DWORD)
      HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\OSUpgrade\AllowOSUpgrade = 0 (DWORD)
      HKLM\SOFTWARE\Policies\Microsoft\WindowsStore\DisableOSUpgrade                    = 1 (DWORD)
  With TargetReleaseVersion set, Windows Update stops showing the "Windows 11 is ready" / upgrade banner.
  None of this stops a manual upgrade via Installation Assistant / ISO.
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param()

. (Join-Path $PSScriptRoot 'TweakCommon.ps1')
if (Invoke-SelfElevate -ScriptPath $PSCommandPath -BoundParameters $PSBoundParameters) { return }
Initialize-TweakLog -Name 'Block-Windows11Upgrade'
$cat = 'BlockWin11'

$os = Get-OsSummary
Write-TweakLog "OS: $($os.ProductName) $($os.EditionID) $($os.DisplayVersion) build $($os.Build)"
if ($os.Build -notlike '19045.*') { Write-TweakLog 'This is not build 19045 (22H2). Pinning to 22H2 will make WU offer 22H2 if updates are ever re-enabled.' 'WARN' }

$WU = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate'
Set-TweakRegistry -Path $WU -Name 'TargetReleaseVersion'     -Type DWord  -Value 1            -Category $cat
Set-TweakRegistry -Path $WU -Name 'ProductVersion'           -Type String -Value 'Windows 10' -Category $cat
Set-TweakRegistry -Path $WU -Name 'TargetReleaseVersionInfo' -Type String -Value '22H2'       -Category $cat
Set-TweakRegistry -Path $WU -Name 'DisableOSUpgrade'         -Type DWord  -Value 1            -Category $cat -Note 'legacy'
Set-TweakRegistry -Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\OSUpgrade' -Name 'AllowOSUpgrade' -Type DWord -Value 0 -Category $cat -Note 'legacy'
Set-TweakRegistry -Path 'HKLM:\SOFTWARE\Policies\Microsoft\WindowsStore' -Name 'DisableOSUpgrade' -Type DWord -Value 1 -Category $cat -Note 'legacy Store upgrade offer'

Write-TweakSummary -Name 'Block-Windows11Upgrade'
