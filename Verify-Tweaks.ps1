#requires -Version 5.1
<#
.SYNOPSIS
  Read-only check of every tweak. Prints PASS / FAIL / WARN per item. Changes nothing.
.DESCRIPTION
  Self-elevates (UAC) so it can see all-user Appx packages; use -NoElevate to run as a normal user
  (HKCU checks then apply to that user).
  WARN = set as intended but Microsoft scopes it to another edition/OS, or Windows protects the item.
#>
[CmdletBinding()]
param([switch]$NoElevate)

$here = Split-Path -Parent $PSCommandPath
. (Join-Path $here 'scripts\TweakCommon.ps1')
if (-not $NoElevate) { if (Invoke-SelfElevate -ScriptPath $PSCommandPath -BoundParameters $PSBoundParameters) { exit } }

$results = New-Object System.Collections.Generic.List[object]
function Add-Result([string]$Category, [string]$Item, [string]$Status, [string]$Detail) {
    $results.Add([pscustomobject]@{ Category = $Category; Status = $Status; Item = $Item; Detail = $Detail })
}

$os = Get-OsSummary
Write-Host ("OS: {0} | Edition {1} | {2} | build {3} | {4}" -f $os.ProductName, $os.EditionID, $os.DisplayVersion, $os.Build, $env:COMPUTERNAME) -ForegroundColor Cyan
if ($os.DisplayVersion -ne '22H2') { Add-Result 'OS' 'Version' 'WARN' "DisplayVersion=$($os.DisplayVersion); kit targets 22H2" } else { Add-Result 'OS' 'Version' 'PASS' "22H2 build $($os.Build)" }

$st0 = Get-TweakState
$hard = -not ($st0.ContainsKey('MODE|Updates') -and $st0['MODE|Updates'].PolicyOnly)
$temp = Get-TweakTempAllow
if ($temp) { Add-Result 'Updates' 'Temporary allow' 'WARN' "UPDATES TEMPORARILY ALLOWED since $($temp.Since) - run scripts\Set-UpdateWindow.ps1 -Restore (or the app's Restore update kill)" }

# Registry values
foreach ($e in Get-TweakManifest) {
    $info = Get-RegValueInfo -Path $e.Path -Name $e.Name
    $item = "$($e.Path)\$($e.Name)"
    if ($e.Level -eq 'Hard' -and -not $hard) { Add-Result $e.Category $item 'SKIP' 'hard kill item; last apply was -PolicyOnly'; continue }
    if ($temp -and $script:TempLiftReg -contains $e.Name -and -not ($info.Existed -and ("$($info.Value)" -eq "$($e.Value)"))) { Add-Result $e.Category $item 'WARN' 'lifted by temporary allow'; continue }
    if ($info.Existed -and ("$($info.Value)" -eq "$($e.Value)")) {
        if ($e.Level -eq 'BestEffort') { Add-Result $e.Category $item 'WARN' "= $($e.Value) (set; may be ignored on this edition/OS)" }
        else { Add-Result $e.Category $item 'PASS' "= $($e.Value)" }
    } else {
        Add-Result $e.Category $item 'FAIL' ("expected $($e.Value), found " + (Format-RegInfo $info))
    }
}

# Services
foreach ($svc in Get-TweakServiceManifest) {
    $i = Get-ServiceStartInfo -Name $svc.Name
    $st = (Get-Service -Name $svc.Name -ErrorAction SilentlyContinue).Status
    if (-not $i) { if ($svc.Name -eq 'uhssvc') { Add-Result 'Updates' "Service uhssvc" 'PASS' 'not installed' } else { Add-Result 'Updates' "Service $($svc.Name)" 'WARN' 'not present' }; continue }
    if ($i.Start -eq 4) { Add-Result 'Updates' "Service $($svc.Name)" 'PASS' "Start=4 (Disabled), status=$st" }
    elseif (-not $hard) { Add-Result 'Updates' "Service $($svc.Name)" 'SKIP' "Start=$($i.Start) (-PolicyOnly)" }
    elseif ($temp -and $script:TempLiftSvc -contains $svc.Name) { Add-Result 'Updates' "Service $($svc.Name)" 'WARN' "Start=$($i.Start), status=$st (temporarily allowed)" }
    else { Add-Result 'Updates' "Service $($svc.Name)" 'FAIL' "Start=$($i.Start), status=$st (expected 4; re-run Disable-AutomaticUpdates.ps1)" }
}
if ($hard) {
    foreach ($n in $script:LockServices) {
        try {
            $sd = Get-TweakKeySddl -SubKey "SYSTEM\CurrentControlSet\Services\$n"
            if ($sd.StartsWith('O:BA') -and $sd.Contains($script:LockDenyAce)) { Add-Result 'Updates' "Key Services\$n" 'PASS' 'locked (owner Administrators, SYSTEM may not set values)' }
            else { Add-Result 'Updates' "Key Services\$n" 'FAIL' "not locked: $sd" }
        } catch { Add-Result 'Updates' "Key Services\$n" 'WARN' $_.Exception.Message }
    }
    $fw = @(Get-NetFirewallRule -PolicyStore PersistentStore -ErrorAction SilentlyContinue | Where-Object { $_.Name -like "$($script:FwPrefix)*" })
    foreach ($s in $script:FwServices) {
        if (-not (Get-Service -Name $s -ErrorAction SilentlyContinue)) { continue }
        $n = "$($script:FwPrefix)$s"; $r = $fw | Where-Object { $_.Name -eq $n }
        if ($r -and "$($r.Enabled)" -eq 'True' -and "$($r.Direction)" -eq 'Outbound' -and "$($r.Action)" -eq 'Block') { Add-Result 'Updates' "Firewall $n" 'PASS' 'outbound block enabled' }
        elseif (-not $r -and $temp) { Add-Result 'Updates' "Firewall $n" 'WARN' 'absent (temporarily allowed)' }
        else { Add-Result 'Updates' "Firewall $n" 'FAIL' $(if ($r) { "$($r.Enabled)/$($r.Direction)/$($r.Action)" } else { 'missing' }) }
        $sid = (Get-ItemProperty -LiteralPath "HKLM:\SYSTEM\CurrentControlSet\Services\$s" -Name ServiceSidType -ErrorAction SilentlyContinue).ServiceSidType
        if ($sid -ne 1 -and $sid -ne 3) { Add-Result 'Updates' "Firewall $n" 'WARN' "$s has no service SID (ServiceSidType=$sid); the service-scoped rule may not match" }
    }
}

# Scheduled tasks
foreach ($f in $script:UpdateTaskFolders) {
    foreach ($t in @(Get-ScheduledTask -TaskPath $f -ErrorAction SilentlyContinue)) {
        $n = "Task $($t.TaskPath)$($t.TaskName)"
        if ($t.State -eq 'Disabled') { Add-Result 'Updates' $n 'PASS' 'Disabled' }
        else { Add-Result 'Updates' $n 'WARN' "$($t.State) (refused even as SYSTEM, or -PolicyOnly; harmless while the update services are Start=4)" }
    }
}

# WU agent view
try {
    $au = (New-Object -ComObject Microsoft.Update.AutoUpdate).Settings
    if ($au.NotificationLevel -eq 1) { Add-Result 'Updates' 'WU agent NotificationLevel' 'PASS' '1 = automatic updates disabled' }
    else { Add-Result 'Updates' 'WU agent NotificationLevel' 'FAIL' "$($au.NotificationLevel) (1 = disabled)" }
} catch { Add-Result 'Updates' 'WU agent NotificationLevel' 'WARN' "could not query ($($_.Exception.Message))" }

# Copilot
$all = $false; if (Test-IsAdmin) { $all = $true }
$pk = if ($all) { @(Get-AppxPackage -AllUsers -Name '*Copilot*' -ErrorAction SilentlyContinue) } else { @(Get-AppxPackage -Name '*Copilot*' -ErrorAction SilentlyContinue) }
if ($pk.Count -eq 0) { Add-Result 'Copilot' 'Appx *Copilot*' 'PASS' ("none installed" + $(if ($all) { ' (all users)' } else { ' (current user)' })) }
else { Add-Result 'Copilot' 'Appx *Copilot*' 'FAIL' (($pk | ForEach-Object { $_.PackageFullName }) -join ', ') }
$hub = @(Get-AppxPackage -Name 'Microsoft.MicrosoftOfficeHub' -ErrorAction SilentlyContinue)
if ($hub.Count -eq 0) { Add-Result 'Copilot' 'Appx Microsoft.MicrosoftOfficeHub' 'PASS' 'not installed' } else { Add-Result 'Copilot' 'Appx Microsoft.MicrosoftOfficeHub' 'WARN' 'installed (expected if -KeepMicrosoft365App)' }
$prov = @(Get-AppxProvisionedPackage -Online -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -like '*Copilot*' })
if ($all) { if ($prov.Count -eq 0) { Add-Result 'Copilot' 'Provisioned *Copilot*' 'PASS' 'none' } else { Add-Result 'Copilot' 'Provisioned *Copilot*' 'FAIL' (($prov | ForEach-Object { $_.PackageName }) -join ', ') } }
foreach ($fam in $script:CopilotFamilies) {
    $k = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Appx\AppxAllUserStore\Deprovisioned\$fam"
    if (Test-Path -LiteralPath $k) { Add-Result 'Copilot' "Deprovisioned\$fam" 'PASS' 'present' }
    elseif ($fam -like '*OfficeHub*') { Add-Result 'Copilot' "Deprovisioned\$fam" 'WARN' 'absent (expected if -KeepMicrosoft365App)' }
    else { Add-Result 'Copilot' "Deprovisioned\$fam" 'FAIL' 'absent' }
}
if (@(Get-Process -Name 'Copilot','Microsoft.Copilot' -ErrorAction SilentlyContinue).Count -gt 0) { Add-Result 'Copilot' 'Copilot process' 'FAIL' 'running' } else { Add-Result 'Copilot' 'Copilot process' 'PASS' 'not running' }

# God Mode
$desk = [Environment]::GetFolderPath('Desktop'); if (-not $desk) { $desk = Join-Path $env:USERPROFILE 'Desktop' }
$gm = Join-Path $desk 'God Mode.{ED7BA470-8E54-465E-825C-99712043E01C}'
if (Test-Path -LiteralPath $gm) { Add-Result 'GodMode' $gm 'PASS' 'exists' } else { Add-Result 'GodMode' $gm 'FAIL' 'missing' }

# Re-apply task (optional)
$rt = Get-ScheduledTask -TaskPath '\Win10Tweaks\' -TaskName 'Reapply-Tweaks' -ErrorAction SilentlyContinue
if ($rt) { Add-Result 'Reapply' 'Task \Win10Tweaks\Reapply-Tweaks' 'PASS' "$($rt.State)" } else { Add-Result 'Reapply' 'Task \Win10Tweaks\Reapply-Tweaks' 'WARN' 'not registered (optional: -RegisterReapplyTask)' }

# Output
foreach ($r in $results) {
    $c = switch ($r.Status) { 'PASS' { 'Green' } 'FAIL' { 'Red' } default { 'Yellow' } }
    Write-Host ('[{0}] {1,-13} {2} {3}' -f $r.Status, $r.Category, $r.Item, $r.Detail) -ForegroundColor $c
}
$p = @($results | Where-Object Status -eq 'PASS').Count; $f = @($results | Where-Object Status -eq 'FAIL').Count; $w = @($results | Where-Object Status -eq 'WARN').Count
Write-Host ''
Write-Host "PASS $p  FAIL $f  WARN $w" -ForegroundColor Cyan
Write-Host ''
Write-Host 'Last 5 installed updates (Get-HotFix):' -ForegroundColor Cyan
try { Get-HotFix | Sort-Object { $_.InstalledOn } -Descending | Select-Object -First 5 HotFixID, Description, InstalledOn | Format-Table -AutoSize | Out-String | Write-Host } catch { Write-Host "Get-HotFix failed: $($_.Exception.Message)" }
Write-Host 'Note: gpresult /r lists GPO-delivered policy only; values written by this kit are local registry policy and will not show there as an applied GPO.' -ForegroundColor DarkGray
