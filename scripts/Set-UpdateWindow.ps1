#requires -Version 5.1
<#
.SYNOPSIS
  Temporarily allow Windows Update (for Microsoft Store / WinGet app updates), then restore the kill.
.DESCRIPTION
  -Allow   lifts ONLY: DoNotConnectToWindowsUpdateInternetLocations, DisableWindowsUpdateAccess, WUServer, WUStatusServer,
           UseWUServer (back to recorded originals), the Win10Tweaks-NoWU-* firewall rules, and wuauserv + UsoSvc
           (back to recorded start types, started). Kept: NoAutoUpdate=1 / AUOptions=1 / NoAutoRebootWithLoggedOnUsers=1 /
           SetDisableUXWUAccess=1 (no automatic OS installs), WaaSMedicSvc disabled with its key locked, uhssvc disabled,
           update tasks disabled, DO mode 99. Copilot / Spotlight / Win11 block / God Mode are not touched.
  -Restore re-applies the full Updates kill (runs Disable-AutomaticUpdates.ps1 in the mode last used).
  Timed auto-restore (30/60/120 min, even with the window closed) is in the GUI app; this script has no timer.
.EXAMPLE
  .\scripts\Set-UpdateWindow.ps1 -Allow      # then update apps in the Store / winget upgrade
  .\scripts\Set-UpdateWindow.ps1 -Restore
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param([switch]$Allow, [switch]$Restore)

. (Join-Path $PSScriptRoot 'TweakCommon.ps1')
if (Invoke-SelfElevate -ScriptPath $PSCommandPath -BoundParameters $PSBoundParameters) { return }
if ($Allow -eq $Restore) { Write-Host 'Use exactly one of -Allow or -Restore.' -ForegroundColor Yellow; return }

if ($Restore) {
    $state = Get-TweakState
    $x = @{}; if ($state.ContainsKey('MODE|Updates') -and $state['MODE|Updates'].PolicyOnly) { $x['PolicyOnly'] = $true }
    if ($WhatIfPreference) { $x['WhatIf'] = $true }
    & (Join-Path $PSScriptRoot 'Disable-AutomaticUpdates.ps1') @x
    return
}

Initialize-TweakLog -Name 'Allow-UpdatesTemporarily'
Write-TweakLog 'Windows Update will be able to reach the internet until you run -Restore.' 'WARN'
$state = Get-TweakState
foreach ($e in Get-TweakManifest | Where-Object { $_.Category -eq 'Updates' -and $script:TempLiftReg -contains $_.Name }) {
    $id = "REG|$($e.Path)|$($e.Name)"
    if ($state.ContainsKey($id)) { Restore-TweakRegistryEntry -Entry $state[$id] -Id $id }
    else {
        $cur = Get-RegValueInfo -Path $e.Path -Name $e.Name
        if ($cur.Existed -and ("$($cur.Value)" -eq "$($e.Value)")) { Remove-TweakRegistryValue -Path $e.Path -Name $e.Name }
    }
}
Remove-TweakFirewallRules
$state = Get-TweakState
foreach ($svc in Get-TweakServiceManifest | Where-Object { $script:TempLiftSvc -contains $_.Name }) {
    $id = "SVC|$($svc.Name)"
    if ($state.ContainsKey($id)) { $start = [int]$state[$id].Start; $d = $null; if ($null -ne $state[$id].DelayedAutostart) { $d = [int]$state[$id].DelayedAutostart } }
    else { $start = 3; $d = $null }
    if ($start -eq 4) { $start = 3 }
    Set-TweakServiceStart -Name $svc.Name -Start $start -Category 'Updates' -DelayedAutostart $d
    if (-not $WhatIfPreference) {
        Remove-TweakStateEntry -Id $id
        try { Start-Service -Name $svc.Name -ErrorAction Stop; Write-TweakLog "Service $($svc.Name) : -> Running" 'SET' } catch { Write-TweakLog "Service $($svc.Name) : could not start ($($_.Exception.Message))" 'WARN' }
    }
}
if (-not $WhatIfPreference) {
    if (-not (Test-Path -LiteralPath $script:TweakRoot)) { New-Item -ItemType Directory -Path $script:TweakRoot -Force | Out-Null }
    ([pscustomobject]@{ Active = $true; Since = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss'); Expires = $null; Task = $false } | ConvertTo-Json) | Set-Content -LiteralPath $script:TempAllowFile -Encoding UTF8
}
Write-TweakLog 'UPDATES TEMPORARILY ALLOWED. Update your Store / WinGet apps, then run: .\scripts\Set-UpdateWindow.ps1 -Restore' 'WARN'
Write-TweakSummary -Name 'Allow-UpdatesTemporarily'
