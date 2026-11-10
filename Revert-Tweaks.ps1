#requires -Version 5.1
<#
.SYNOPSIS
  Undoes what this kit set. Self-elevates (UAC). Supports -WhatIf.
.DESCRIPTION
  Uses C:\ProgramData\Win10Tweaks\state.json (original values recorded the first time each item was touched):
    - value existed before  -> original data/type restored
    - value did not exist   -> deleted (back to "Not configured")
  Without a recorded original it falls back to deleting the value / stock 22H2 service start types.
  It cannot restore an unknown earlier state perfectly, and it cannot reinstall removed apps
  (reinstall Copilot / Microsoft 365 Copilot from the Microsoft Store if you want them back).
  No switch = revert everything. SoftwareDistribution cleanup has nothing to revert.
.EXAMPLE
  .\Revert-Tweaks.ps1 -Updates      # re-enable Windows Update only (e.g. to install ESU patches)
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param([switch]$Updates, [switch]$Notifications, [switch]$BlockWin11, [switch]$Copilot, [switch]$Spotlight, [switch]$GodMode, [switch]$ReapplyTask)

$here = Split-Path -Parent $PSCommandPath
. (Join-Path $here 'scripts\TweakCommon.ps1')
if (-not (Test-IsAdmin)) {
    $fwd = @{}; foreach ($k in $PSBoundParameters.Keys) { $fwd[$k] = $PSBoundParameters[$k] }
    if ($WhatIfPreference -and -not $fwd.ContainsKey('WhatIf')) { $fwd['WhatIf'] = [switch]$true }
    Invoke-SelfElevate -ScriptPath $PSCommandPath -BoundParameters $fwd | Out-Null
    exit
}
Initialize-TweakLog -Name 'Revert-Tweaks'

$cats = @()
if ($Updates) { $cats += 'Updates' }; if ($Notifications) { $cats += 'Notifications' }; if ($BlockWin11) { $cats += 'BlockWin11' }
if ($Copilot) { $cats += 'Copilot' }; if ($Spotlight) { $cats += 'Spotlight' }; if ($GodMode) { $cats += 'GodMode' }
$all = ($cats.Count -eq 0 -and -not $ReapplyTask)
if ($all) { $cats = @('Updates','Notifications','BlockWin11','Copilot','Spotlight','GodMode'); $ReapplyTask = [switch]$true }
Write-TweakLog "Reverting: $($cats -join ', ')$(if ($ReapplyTask) { ', ReapplyTask' })"

# Remove the re-apply task first so it can't re-apply mid-revert
if ($ReapplyTask) {
    $rt = Get-ScheduledTask -TaskPath '\Win10Tweaks\' -TaskName 'Reapply-Tweaks' -ErrorAction SilentlyContinue
    if ($rt) {
        if ($PSCmdlet.ShouldProcess('Task \Win10Tweaks\Reapply-Tweaks', 'Unregister')) {
            try { Unregister-ScheduledTask -TaskPath '\Win10Tweaks\' -TaskName 'Reapply-Tweaks' -Confirm:$false -ErrorAction Stop; Write-TweakLog 'Task \Win10Tweaks\Reapply-Tweaks : registered -> removed' 'SET' }
            catch { Write-TweakLog "Could not remove re-apply task: $($_.Exception.Message)" 'ERROR' }
        } else { Write-TweakLog 'Task \Win10Tweaks\Reapply-Tweaks : registered -> removed' 'WHATIF' }
    } else { Write-TweakLog 'Re-apply task not registered' 'SKIP' }
}

$state = Get-TweakState

if ($cats -contains 'Updates') {
    Clear-TweakTempAllow
    Write-TweakLog '--- Firewall rules ---'
    Remove-TweakFirewallRules
    Write-TweakLog '--- Services ---'
    foreach ($svc in Get-TweakServiceManifest) {
        $id = "SVC|$($svc.Name)"
        if ($state.ContainsKey($id)) { $start = [int]$state[$id].Start; $del = $state[$id].DelayedAutostart }
        else {
            $cur = Get-ServiceStartInfo -Name $svc.Name
            if (-not $cur -or $cur.Start -ne 4) { Write-TweakLog "Service $($svc.Name) not disabled by this kit - left alone" 'SKIP'; continue }
            $start = $svc.DefaultStart; $del = $svc.DefaultDelayed; Write-TweakLog "No recorded original for $($svc.Name); using stock default Start=$start" 'WARN'
        }
        $d = $null; if ($null -ne $del) { $d = [int]$del }
        Set-TweakServiceStart -Name $svc.Name -Start $start -Category 'Revert' -DelayedAutostart $d
        if (-not $WhatIfPreference) { Remove-TweakStateEntry -Id $id; $state = Get-TweakState }
    }
    Write-TweakLog '--- Service key owner/permissions ---'
    if (-not $WhatIfPreference) { Restore-TweakKeyAcls; $state = Get-TweakState } else { Write-TweakLog 'Would restore recorded ACL|* owner + SDDL' 'WHATIF' }
    Write-TweakLog '--- Scheduled tasks ---'
    foreach ($id in @($state.Keys | Where-Object { $_ -like 'TASK|*' })) {
        $e = $state[$id]
        if ($e.Category -eq 'OneDrive') { continue }   # OneDrive tasks: scripts\Remove-OneDrive.ps1 -Revert
        if ($e.State -eq 'Disabled') { Remove-TweakStateEntry -Id $id; continue }
        $target = "Task $($e.TaskPath)$($e.TaskName)"
        if ($PSCmdlet.ShouldProcess($target, 'Enable')) {
            if (Enable-TweakTaskAny -TaskPath $e.TaskPath -TaskName $e.TaskName) { Write-TweakLog "$target : Disabled -> $($e.State)" 'SET'; Remove-TweakStateEntry -Id $id }
            else { Write-TweakLog "$target : enable failed" 'WARN' }
        } else { Write-TweakLog "$target : Disabled -> Ready" 'WHATIF' }
    }
    Remove-TweakStateEntry -Id 'MODE|Updates'
    $state = Get-TweakState
}

Write-TweakLog '--- Registry values ---'
foreach ($e in Get-TweakManifest | Where-Object { $cats -contains $_.Category }) {
    $id = "REG|$($e.Path)|$($e.Name)"
    if ($state.ContainsKey($id)) { Restore-TweakRegistryEntry -Entry $state[$id] -Id $id }
    else {
        $cur = Get-RegValueInfo -Path $e.Path -Name $e.Name
        if ($cur.Existed -and ("$($cur.Value)" -eq "$($e.Value)")) {
            Write-TweakLog "No recorded original for $($e.Path)\$($e.Name); deleting (Windows falls back to its default)" 'WARN'
            Remove-TweakRegistryValue -Path $e.Path -Name $e.Name
        } else { Write-TweakLog "$($e.Path)\$($e.Name) not set by this kit (now $(Format-RegInfo $cur)) - left alone" 'SKIP' }
    }
}

if ($cats -contains 'Copilot') {
    Write-TweakLog '--- Copilot deprovision markers ---'
    foreach ($fam in $script:CopilotFamilies) {
        $k = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Appx\AppxAllUserStore\Deprovisioned\$fam"
        if (Test-Path -LiteralPath $k) {
            if ($PSCmdlet.ShouldProcess($k, 'Delete key')) {
                try { Remove-Item -LiteralPath $k -Force -ErrorAction Stop; Write-TweakLog "$k : present -> deleted" 'SET'; Remove-TweakStateEntry -Id "KEY|$k" }
                catch { Write-TweakLog "$k : $($_.Exception.Message)" 'ERROR' }
            } else { Write-TweakLog "$k : present -> deleted" 'WHATIF' }
        }
    }
    Write-TweakLog 'Removed apps are NOT reinstalled. Get Copilot / Microsoft 365 Copilot from the Microsoft Store if wanted (needs wuauserv enabled).' 'INFO'
}

if ($cats -contains 'GodMode') {
    $desk = [Environment]::GetFolderPath('Desktop'); if (-not $desk) { $desk = Join-Path $env:USERPROFILE 'Desktop' }
    $gm = Join-Path $desk 'God Mode.{ED7BA470-8E54-465E-825C-99712043E01C}'
    if (Test-Path -LiteralPath $gm) {
        if ($PSCmdlet.ShouldProcess($gm, 'Remove folder')) {
            try { Remove-Item -LiteralPath $gm -Force -Recurse -ErrorAction Stop; Write-TweakLog "$gm : present -> removed" 'SET'; Remove-TweakStateEntry -Id "GODMODE|$gm" }
            catch { Write-TweakLog "$gm : $($_.Exception.Message)" 'ERROR' }
        } else { Write-TweakLog "$gm : present -> removed" 'WHATIF' }
    } else { Write-TweakLog "$gm not present" 'SKIP' }
}

Write-TweakLog 'Reboot to finish. Lock-screen background: re-pick Windows spotlight in Settings if you want it back.' 'INFO'
Write-TweakSummary -Name 'Revert-Tweaks'
