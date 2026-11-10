#requires -Version 5.1
<#
.SYNOPSIS
  Removes Windows Spotlight, lock-screen fun facts/tips, suggestions, News and interests and widgets.

.DESCRIPTION
  Per-user Settings-app values (HKCU\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager, DWORD 0).
  These are the values the Settings UI writes; Microsoft does not document them as policy:
    RotatingLockScreenEnabled         Spotlight as lock-screen background
    RotatingLockScreenOverlayEnabled  "Get fun facts, tips, tricks and more on your lock screen"
    SubscribedContent-338387Enabled   lock-screen tips/facts content
    SubscribedContent-338388Enabled   Start suggestions
    SubscribedContent-338389Enabled   tips/tricks notifications
    SubscribedContent-338393Enabled   Settings suggested content
    SubscribedContent-353694Enabled   Settings suggested content
    SubscribedContent-353696Enabled   Settings suggested content
    SubscribedContent-310093Enabled   "Windows welcome experience" after updates
    SystemPaneSuggestionsEnabled, SoftLandingEnabled, SilentInstalledAppsEnabled,
    PreInstalledAppsEnabled, OemPreInstalledAppsEnabled   (auto-installed promo apps / suggestions)
  CloudContent policy (CloudContent.admx). NOTE: Microsoft lists most of these as Enterprise/Education only;
  Windows 10 Pro/Home generally ignore them. Written anyway so they apply if the edition changes:
    HKCU\Software\Policies\Microsoft\Windows\CloudContent
      DisableWindowsSpotlightFeatures=1, ConfigureWindowsSpotlight=2 (2 = Spotlight disabled),
      DisableWindowsSpotlightOnActionCenter=1, DisableWindowsSpotlightOnSettings=1,
      DisableWindowsSpotlightWindowsWelcomeExperience=1, DisableThirdPartySuggestions=1,
      DisableTailoredExperiencesWithDiagnosticData=1
    HKLM\SOFTWARE\Policies\Microsoft\Windows\CloudContent
      DisableWindowsConsumerFeatures=1, DisableSoftLanding=1, DisableCloudOptimizedContent=1
  News and interests (works on Win10 Pro/Home):
    HKLM\SOFTWARE\Policies\Microsoft\Windows\Windows Feeds  EnableFeeds = 0
    HKCU\Software\Microsoft\Windows\CurrentVersion\Feeds    ShellFeedsTaskbarViewMode = 2 (off)
  Widgets (Windows 11 policies in NewsAndInterests.admx; no-op on Win10, harmless):
    HKLM\SOFTWARE\Policies\Microsoft\Dsh  AllowNewsAndInterests = 0, DisableWidgetsOnLockScreen = 1
    HKCU\...\Explorer\Advanced            TaskbarDa = 0
  Search highlights: HKLM\SOFTWARE\Policies\Microsoft\Windows\Windows Search  EnableDynamicContentInWSB = 0

  Windows 10 lock-screen widgets (Weather / Daily Wonder / Quiz cards, pushed in 2025) have NO documented
  registry switch on Win10. Manual fix (see README): Settings > Personalization > Lock screen > set background
  to Windows spotlight, set the widget/status app to None, then switch background back to Picture.
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param()

. (Join-Path $PSScriptRoot 'TweakCommon.ps1')
if (Invoke-SelfElevate -ScriptPath $PSCommandPath -BoundParameters $PSBoundParameters) { return }
Initialize-TweakLog -Name 'Disable-LockScreenSpotlight'
$cat = 'Spotlight'

Write-TweakLog '--- ContentDeliveryManager (per user) ---'
$cdm = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager'
$cdmValues = @(
    'RotatingLockScreenEnabled', 'RotatingLockScreenOverlayEnabled',
    'SubscribedContent-338387Enabled', 'SubscribedContent-338388Enabled', 'SubscribedContent-338389Enabled',
    'SubscribedContent-338393Enabled', 'SubscribedContent-353694Enabled', 'SubscribedContent-353696Enabled',
    'SubscribedContent-310093Enabled', 'SystemPaneSuggestionsEnabled', 'SoftLandingEnabled',
    'SilentInstalledAppsEnabled', 'PreInstalledAppsEnabled', 'OemPreInstalledAppsEnabled'
)
foreach ($n in $cdmValues) { Set-TweakRegistry -Path $cdm -Name $n -Type DWord -Value 0 -Category $cat }

Write-TweakLog '--- CloudContent policies (Enterprise/Education-only per Microsoft; harmless on Pro) ---'
$ccU = 'HKCU:\Software\Policies\Microsoft\Windows\CloudContent'
Set-TweakRegistry -Path $ccU -Name 'DisableWindowsSpotlightFeatures'                 -Type DWord -Value 1 -Category $cat
Set-TweakRegistry -Path $ccU -Name 'ConfigureWindowsSpotlight'                       -Type DWord -Value 2 -Category $cat -Note '2 = disabled'
Set-TweakRegistry -Path $ccU -Name 'DisableWindowsSpotlightOnActionCenter'           -Type DWord -Value 1 -Category $cat
Set-TweakRegistry -Path $ccU -Name 'DisableWindowsSpotlightOnSettings'               -Type DWord -Value 1 -Category $cat
Set-TweakRegistry -Path $ccU -Name 'DisableWindowsSpotlightWindowsWelcomeExperience' -Type DWord -Value 1 -Category $cat
Set-TweakRegistry -Path $ccU -Name 'DisableThirdPartySuggestions'                    -Type DWord -Value 1 -Category $cat
Set-TweakRegistry -Path $ccU -Name 'DisableTailoredExperiencesWithDiagnosticData'    -Type DWord -Value 1 -Category $cat
$ccM = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent'
Set-TweakRegistry -Path $ccM -Name 'DisableWindowsConsumerFeatures' -Type DWord -Value 1 -Category $cat
Set-TweakRegistry -Path $ccM -Name 'DisableSoftLanding'             -Type DWord -Value 1 -Category $cat
Set-TweakRegistry -Path $ccM -Name 'DisableCloudOptimizedContent'   -Type DWord -Value 1 -Category $cat

Write-TweakLog '--- News and interests / widgets / search highlights ---'
Set-TweakRegistry -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Feeds' -Name 'EnableFeeds' -Type DWord -Value 0 -Category $cat -Note 'News and interests off'
Set-TweakRegistry -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Feeds' -Name 'ShellFeedsTaskbarViewMode' -Type DWord -Value 2 -Category $cat -Note '2 = off'
Set-TweakRegistry -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Dsh' -Name 'AllowNewsAndInterests'      -Type DWord -Value 0 -Category $cat -Note 'Win11 widgets policy'
Set-TweakRegistry -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Dsh' -Name 'DisableWidgetsOnLockScreen' -Type DWord -Value 1 -Category $cat -Note 'Win11/Insider policy'
Set-TweakRegistry -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced' -Name 'TaskbarDa' -Type DWord -Value 0 -Category $cat -Note 'Win11 widgets button; no-op on Win10'
Set-TweakRegistry -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Search' -Name 'EnableDynamicContentInWSB' -Type DWord -Value 0 -Category $cat -Note 'search highlights off'

Write-TweakLog 'Lock-screen Weather/Daily Wonder cards on Win10 need the one-time Settings step in README (no documented registry value).' 'WARN'
Write-TweakLog 'Sign out / restart Explorer to see taskbar changes.' 'INFO'
Write-TweakSummary -Name 'Disable-LockScreenSpotlight'
