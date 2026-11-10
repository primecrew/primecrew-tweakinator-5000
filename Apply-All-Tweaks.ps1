#requires -Version 5.1
<#
.SYNOPSIS
  Master script: applies every tweak (or only the ones you pick). Self-elevates via UAC.

.DESCRIPTION
  No category switch given  => everything runs, in this order:
    Clear-SoftwareDistribution -> Disable-AutomaticUpdates -> Disable-UpdateNotifications ->
    Block-Windows11Upgrade -> Remove-Copilot -> Disable-LockScreenSpotlight -> New-GodModeFolder
  A restore point is created first (default ON; -CreateRestorePoint:$false or -NoRestorePoint to skip).
  Every change is logged to C:\ProgramData\Win10Tweaks\Logs\Apply-All-Tweaks-<timestamp>.log
  Original values are recorded once in C:\ProgramData\Win10Tweaks\state.json for Revert-Tweaks.ps1.

.EXAMPLE
  .\Apply-All-Tweaks.ps1 -WhatIf                 # dry run, changes nothing
.EXAMPLE
  .\Apply-All-Tweaks.ps1                         # everything
.EXAMPLE
  .\Apply-All-Tweaks.ps1 -Copilot -Spotlight     # only those two
.EXAMPLE
  .\Apply-All-Tweaks.ps1 -RegisterReapplyTask    # everything + re-apply at every logon
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [switch]$Updates,
    [switch]$Notifications,
    [switch]$BlockWin11,
    [switch]$Copilot,
    [switch]$Spotlight,
    [switch]$GodMode,
    [switch]$CleanSoftwareDistribution,
    [switch]$SkipCleanup,              # with "everything", skip the SoftwareDistribution cleanup
    [switch]$PolicyOnly,               # updates: policies only, keep services/tasks
    [switch]$KeepMicrosoft365App,      # Copilot: keep Microsoft.MicrosoftOfficeHub
    [switch]$CreateRestorePoint = $true,
    [switch]$NoRestorePoint,
    [switch]$EnableSystemProtection,   # turn on System Protection for C: if it is off, so the restore point can be made
    [switch]$RegisterReapplyTask,      # scheduled task: re-apply at logon (survives Microsoft resets)
    [switch]$Unattended                # used by the re-apply task: no restore point, no cleanup, no pause
)

$here = Split-Path -Parent $PSCommandPath
. (Join-Path $here 'scripts\TweakCommon.ps1')

# Self-elevate (UAC). A false switch can't be forwarded through -File, so translate it.
if (-not (Test-IsAdmin)) {
    $fwd = @{}; foreach ($k in $PSBoundParameters.Keys) { $fwd[$k] = $PSBoundParameters[$k] }
    if ($fwd.ContainsKey('CreateRestorePoint') -and -not $CreateRestorePoint) { $fwd.Remove('CreateRestorePoint'); $fwd['NoRestorePoint'] = [switch]$true }
    if ($WhatIfPreference -and -not $fwd.ContainsKey('WhatIf')) { $fwd['WhatIf'] = [switch]$true }
    Invoke-SelfElevate -ScriptPath $PSCommandPath -BoundParameters $fwd | Out-Null
    exit
}

$env:WIN10TWEAKS_LOG = Join-Path (Join-Path $env:ProgramData 'Win10Tweaks\Logs') ('Apply-All-Tweaks-{0:yyyyMMdd-HHmmss}.log' -f (Get-Date))
Initialize-TweakLog -Name 'Apply-All-Tweaks'

$os = Get-OsSummary
Write-TweakLog "OS: $($os.ProductName) | Edition $($os.EditionID) | $($os.DisplayVersion) | build $($os.Build)"
Write-TweakLog 'SECURITY: Windows 10 is past end of support (Oct 14, 2025); blocking updates - including consumer ESU security patches (available through Oct 12, 2027) - leaves this PC exposed to unpatched vulnerabilities.' 'WARN'

$picked = $Updates -or $Notifications -or $BlockWin11 -or $Copilot -or $Spotlight -or $GodMode -or $CleanSoftwareDistribution
if (-not $picked) {
    $Updates = $Notifications = $BlockWin11 = $Copilot = $Spotlight = $GodMode = [switch]$true
    $CleanSoftwareDistribution = [switch](-not ($SkipCleanup -or $Unattended))
}

# Restore point
$doRestorePoint = $CreateRestorePoint -and -not $NoRestorePoint -and -not $Unattended
if ($doRestorePoint) {
    if ($PSCmdlet.ShouldProcess('System Restore', 'Create restore point "Win10Tweaks - before Apply-All-Tweaks"')) {
        try {
            if ($EnableSystemProtection) { Enable-ComputerRestore -Drive "$env:SystemDrive\" -ErrorAction Stop; Write-TweakLog "System Protection enabled on $env:SystemDrive" 'SET' }
            Checkpoint-Computer -Description 'Win10Tweaks - before Apply-All-Tweaks' -RestorePointType 'MODIFY_SETTINGS' -ErrorAction Stop -WarningVariable rpWarn -WarningAction SilentlyContinue
            if ($rpWarn) { Write-TweakLog "Restore point not created: $rpWarn (Windows allows one every 24h by default)" 'WARN' }
            else { Write-TweakLog 'Restore point created' 'OK' }
        } catch {
            Write-TweakLog "Restore point failed: $($_.Exception.Message). Is System Protection on for $env:SystemDrive? (re-run with -EnableSystemProtection)" 'WARN'
            if (-not $Unattended) {
                $ans = Read-Host 'Continue without a restore point? [y/N]'
                if ($ans -notmatch '^(y|yes)$') { Write-TweakLog 'Aborted by user.' 'WARN'; return }
            }
        }
    } else { Write-TweakLog 'Create restore point' 'WHATIF' }
} else { Write-TweakLog 'Restore point skipped' 'INFO' }

$s = Join-Path $here 'scripts'
$wi = [bool]$WhatIfPreference
function Invoke-Child([string]$file, [hashtable]$extra = @{}) {
    Write-TweakLog "----- $file -----"
    try { & (Join-Path $s $file) @extra -WhatIf:$wi } catch { Write-TweakLog "$file threw: $($_.Exception.Message)" 'ERROR' }
}

if ($CleanSoftwareDistribution) { Invoke-Child 'Clear-SoftwareDistribution.ps1' }
if ($Updates)       { $x = @{}; if ($PolicyOnly) { $x['PolicyOnly'] = $true }; Invoke-Child 'Disable-AutomaticUpdates.ps1' $x }
if ($Notifications) { Invoke-Child 'Disable-UpdateNotifications.ps1' }
if ($BlockWin11)    { Invoke-Child 'Block-Windows11Upgrade.ps1' }
if ($Copilot)       { $x = @{}; if ($KeepMicrosoft365App) { $x['KeepMicrosoft365App'] = $true }; Invoke-Child 'Remove-Copilot.ps1' $x }
if ($Spotlight)     { Invoke-Child 'Disable-LockScreenSpotlight.ps1' }
if ($GodMode)       { Invoke-Child 'New-GodModeFolder.ps1' }

if ($RegisterReapplyTask) {
    $argLine = "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$PSCommandPath`" -Unattended"
    if ($picked) {
        foreach ($n in 'Updates','Notifications','BlockWin11','Copilot','Spotlight','GodMode') {
            if (Get-Variable -Name $n -ValueOnly) { $argLine += " -$n" }
        }
    }
    if ($PolicyOnly) { $argLine += ' -PolicyOnly' }
    if ($KeepMicrosoft365App) { $argLine += ' -KeepMicrosoft365App' }
    if ($PSCmdlet.ShouldProcess('Task \Win10Tweaks\Reapply-Tweaks', "Register at logon of $env:USERDOMAIN\$env:USERNAME (highest privileges)")) {
        try {
            $action    = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument $argLine
            $trigger   = New-ScheduledTaskTrigger -AtLogOn -User "$env:USERDOMAIN\$env:USERNAME"
            $principal = New-ScheduledTaskPrincipal -UserId "$env:USERDOMAIN\$env:USERNAME" -LogonType Interactive -RunLevel Highest
            $settings  = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Minutes 15)
            Register-ScheduledTask -TaskPath '\Win10Tweaks\' -TaskName 'Reapply-Tweaks' -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force -ErrorAction Stop | Out-Null
            Write-TweakLog "Task \Win10Tweaks\Reapply-Tweaks registered: powershell.exe $argLine" 'SET'
            Write-TweakLog 'The task runs the scripts from this folder - do not move/delete the folder.' 'INFO'
        } catch { Write-TweakLog "Could not register re-apply task: $($_.Exception.Message)" 'ERROR' }
    } else { Write-TweakLog "Register task \Win10Tweaks\Reapply-Tweaks : powershell.exe $argLine" 'WHATIF' }
}

Write-TweakLog 'Done. Reboot, then run Verify-Tweaks.ps1.' 'OK'
Write-TweakLog "Log: $env:WIN10TWEAKS_LOG"
Remove-Item Env:\WIN10TWEAKS_LOG -ErrorAction SilentlyContinue
