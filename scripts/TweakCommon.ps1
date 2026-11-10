#requires -Version 5.1
<#
  TweakCommon.ps1 - shared helpers for the Win10 tweak kit.
  Dot-sourced by every script. PowerShell 5.1 compatible.
  - Self-elevation
  - Logging (console + C:\ProgramData\Win10Tweaks\Logs\*.log)
  - Registry / service / scheduled-task setters that record the ORIGINAL value
    the first time they touch something (C:\ProgramData\Win10Tweaks\state.json),
    so Revert-Tweaks.ps1 can put it back.
  All setters honour -WhatIf through SupportsShouldProcess.
#>

$script:TweakRoot  = Join-Path $env:ProgramData 'Win10Tweaks'
$script:StateFile  = Join-Path $script:TweakRoot 'state.json'
$script:LogDir     = Join-Path $script:TweakRoot 'Logs'
$script:TempAllowFile = Join-Path $script:TweakRoot 'tempallow.json'
$script:DeadWsus      = 'http://127.0.0.1:8530'          # local, non-routable "intranet update server"
$script:LockServices  = @('WaaSMedicSvc')                 # service KEYS we take over (never binaries)
$script:LockDenyAce   = '(D;;0x2;;;SY)'                   # deny SYSTEM "Set Value" on that key only
$script:AdminFullAce  = '(A;;KA;;;BA)'
$script:FwPrefix      = 'Win10Tweaks-NoWU-'
$script:FwServices    = @('wuauserv', 'UsoSvc', 'WaaSMedicSvc', 'uhssvc')
$script:TempLiftReg   = @('DoNotConnectToWindowsUpdateInternetLocations', 'DisableWindowsUpdateAccess', 'WUServer', 'WUStatusServer', 'UseWUServer')
$script:TempLiftSvc   = @('wuauserv', 'UsoSvc')
$script:RestoreTaskPath = '\Win10Tweaks\'
$script:RestoreTaskName = 'Win10Tweaks-RestoreWUKill'
if (-not (Get-Variable -Name TweakErrors -Scope Script -ErrorAction SilentlyContinue)) { $script:TweakErrors = 0 }
if (-not (Get-Variable -Name LogFile -Scope Script -ErrorAction SilentlyContinue)) { $script:LogFile = $null }

function Test-IsAdmin {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    return ([Security.Principal.WindowsPrincipal]$id).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Invoke-SelfElevate {
    <# Returns $true if it relaunched elevated (caller should exit). Forwards present switches and string params. #>
    param([Parameter(Mandatory)][string]$ScriptPath, [System.Collections.IDictionary]$BoundParameters = @{})
    if (Test-IsAdmin) { return $false }
    Write-Host 'Not elevated - relaunching as Administrator (UAC prompt)...' -ForegroundColor Yellow
    $argList = @('-NoProfile', '-NoExit', '-ExecutionPolicy', 'Bypass', '-File', ('"{0}"' -f $ScriptPath))
    foreach ($k in @($BoundParameters.Keys)) {
        $v = $BoundParameters[$k]
        if ($v -is [System.Management.Automation.SwitchParameter]) {
            if ($v.IsPresent) { $argList += "-$k" }
        } elseif ($v -is [bool]) {
            if ($v) { $argList += "-$k" }
        } else {
            $argList += "-$k"; $argList += ('"{0}"' -f $v)
        }
    }
    try {
        Start-Process -FilePath 'powershell.exe' -Verb RunAs -ArgumentList ($argList -join ' ') -ErrorAction Stop | Out-Null
    } catch {
        Write-Host "Elevation was cancelled or failed: $($_.Exception.Message)" -ForegroundColor Red
    }
    return $true
}

function Initialize-TweakLog {
    param([Parameter(Mandatory)][string]$Name)
    foreach ($d in @($script:TweakRoot, $script:LogDir)) {
        if (-not (Test-Path -LiteralPath $d)) { New-Item -ItemType Directory -Path $d -Force -WhatIf:$false -Confirm:$false | Out-Null }
    }
    if ($env:WIN10TWEAKS_LOG) { $script:LogFile = $env:WIN10TWEAKS_LOG }
    else { $script:LogFile = Join-Path $script:LogDir ('{0}-{1:yyyyMMdd-HHmmss}.log' -f $Name, (Get-Date)) }
    Write-TweakLog "=== $Name started (user=$env:USERNAME, computer=$env:COMPUTERNAME, WhatIf=$WhatIfPreference) ===" 'INFO'
}

function Write-TweakLog {
    param([string]$Message, [ValidateSet('INFO','SET','SKIP','WARN','ERROR','WHATIF','OK','PASS','FAIL')][string]$Level = 'INFO')
    $line = '{0:yyyy-MM-dd HH:mm:ss} [{1}] {2}' -f (Get-Date), $Level, $Message
    $color = switch ($Level) {
        'SET' { 'Green' } 'OK' { 'Green' } 'PASS' { 'Green' } 'SKIP' { 'DarkGray' }
        'WARN' { 'Yellow' } 'WHATIF' { 'Cyan' } 'ERROR' { 'Red' } 'FAIL' { 'Red' } default { 'Gray' }
    }
    Write-Host $line -ForegroundColor $color
    if ($Level -eq 'ERROR') { $script:TweakErrors++ }
    if ($script:LogFile) {
        try { Add-Content -LiteralPath $script:LogFile -Value $line -WhatIf:$false -Confirm:$false -ErrorAction Stop } catch { }
    }
}

# ---------------- state (original values) ----------------
function Get-TweakState {
    $h = @{}
    if (Test-Path -LiteralPath $script:StateFile) {
        try {
            $o = Get-Content -LiteralPath $script:StateFile -Raw | ConvertFrom-Json
            foreach ($p in $o.PSObject.Properties) { $h[$p.Name] = $p.Value }
        } catch { Write-TweakLog "Could not read state file $($script:StateFile): $($_.Exception.Message)" 'WARN' }
    }
    return $h
}

function Save-TweakOriginal {
    <# Records the original value only the FIRST time an item is touched (re-runs never overwrite it). #>
    param([Parameter(Mandatory)][string]$Id, [Parameter(Mandatory)][hashtable]$Data)
    if ($WhatIfPreference) { return }
    $state = Get-TweakState
    if ($state.ContainsKey($Id)) { return }
    $Data['RecordedAt'] = (Get-Date).ToString('s')
    $state[$Id] = [pscustomobject]$Data
    if (-not (Test-Path -LiteralPath $script:TweakRoot)) { New-Item -ItemType Directory -Path $script:TweakRoot -Force | Out-Null }
    ($state | ConvertTo-Json -Depth 6) | Set-Content -LiteralPath $script:StateFile -Encoding UTF8 -WhatIf:$false -Confirm:$false
}

function Remove-TweakStateEntry {
    param([Parameter(Mandatory)][string]$Id)
    if ($WhatIfPreference) { return }
    $state = Get-TweakState
    if ($state.ContainsKey($Id)) {
        $state.Remove($Id)
        ($state | ConvertTo-Json -Depth 6) | Set-Content -LiteralPath $script:StateFile -Encoding UTF8 -WhatIf:$false -Confirm:$false
    }
}

# ---------------- registry ----------------
function Get-RegValueInfo {
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$Name)
    $none = [pscustomobject]@{ Existed = $false; Kind = $null; Value = $null }
    if (-not (Test-Path -LiteralPath $Path)) { return $none }
    $key = Get-Item -LiteralPath $Path -ErrorAction SilentlyContinue
    if (-not $key) { return $none }
    if (-not ($key.GetValueNames() -contains $Name)) { return $none }
    $kind = $key.GetValueKind($Name).ToString()
    $val  = $key.GetValue($Name, $null, 'DoNotExpandEnvironmentNames')
    return [pscustomobject]@{ Existed = $true; Kind = $kind; Value = $val }
}

function Format-RegInfo {
    param($Info)
    if (-not $Info.Existed) { return '<absent>' }
    return ('{0} ({1})' -f $Info.Value, $Info.Kind)
}

function Set-TweakRegistry {
    [CmdletBinding(SupportsShouldProcess = $true)]
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][ValidateSet('DWord','String','QWord','ExpandString')][string]$Type,
        [Parameter(Mandatory)]$Value,
        [string]$Category = 'General',
        [string]$Note = ''
    )
    $cur = Get-RegValueInfo -Path $Path -Name $Name
    $old = Format-RegInfo $cur
    $target = "$Path\$Name"
    if ($cur.Existed -and $cur.Kind -eq $Type -and ("$($cur.Value)" -eq "$Value")) {
        Write-TweakLog "$target already = $Value ($Type)" 'SKIP'; return
    }
    if ($PSCmdlet.ShouldProcess($target, "Set $Type = $Value (was $old)")) {
        try {
            Save-TweakOriginal -Id "REG|$Path|$Name" -Data @{ Category = $Category; Kind = 'Registry'; Path = $Path; Name = $Name; Existed = $cur.Existed; ValueKind = $cur.Kind; Value = $cur.Value }
            if (-not (Test-Path -LiteralPath $Path)) { New-Item -Path $Path -Force -ErrorAction Stop | Out-Null }
            New-ItemProperty -LiteralPath $Path -Name $Name -PropertyType $Type -Value $Value -Force -ErrorAction Stop | Out-Null
            $msg = "$target : $old -> $Value ($Type)"; if ($Note) { $msg += "  [$Note]" }
            Write-TweakLog $msg 'SET'
        } catch {
            Write-TweakLog "$target : failed to set ($($_.Exception.Message))" 'ERROR'
        }
    } else {
        Write-TweakLog "$target : $old -> $Value ($Type)" 'WHATIF'
    }
}

function New-TweakRegistryKey {
    <# Creates an (empty) key, e.g. AppxAllUserStore\Deprovisioned\<PFN>. #>
    [CmdletBinding(SupportsShouldProcess = $true)]
    param([Parameter(Mandatory)][string]$Path, [string]$Category = 'General')
    if (Test-Path -LiteralPath $Path) { Write-TweakLog "$Path already exists" 'SKIP'; return }
    if ($PSCmdlet.ShouldProcess($Path, 'Create registry key')) {
        try {
            Save-TweakOriginal -Id "KEY|$Path" -Data @{ Category = $Category; Kind = 'RegistryKey'; Path = $Path; Existed = $false }
            New-Item -Path $Path -Force -ErrorAction Stop | Out-Null
            Write-TweakLog "$Path : <absent> -> key created" 'SET'
        } catch { Write-TweakLog "$Path : failed to create ($($_.Exception.Message))" 'ERROR' }
    } else { Write-TweakLog "$Path : <absent> -> key created" 'WHATIF' }
}

function Restore-TweakRegistryEntry {
    <# Restores a value from a state entry; deletes it if it did not exist originally. #>
    [CmdletBinding(SupportsShouldProcess = $true)]
    param([Parameter(Mandatory)]$Entry, [Parameter(Mandatory)][string]$Id)
    $target = "$($Entry.Path)\$($Entry.Name)"
    $cur = Format-RegInfo (Get-RegValueInfo -Path $Entry.Path -Name $Entry.Name)
    if ($Entry.Existed) {
        $kind = $Entry.ValueKind; if ($kind -eq 'MultiString') { $kind = 'MultiString' }
        if ($PSCmdlet.ShouldProcess($target, "Restore original $($Entry.Value) ($kind)")) {
            try {
                if (-not (Test-Path -LiteralPath $Entry.Path)) { New-Item -Path $Entry.Path -Force | Out-Null }
                New-ItemProperty -LiteralPath $Entry.Path -Name $Entry.Name -PropertyType $kind -Value $Entry.Value -Force -ErrorAction Stop | Out-Null
                Write-TweakLog "$target : $cur -> $($Entry.Value) ($kind) [original restored]" 'SET'
                Remove-TweakStateEntry -Id $Id
            } catch { Write-TweakLog "$target : restore failed ($($_.Exception.Message))" 'ERROR' }
        } else { Write-TweakLog "$target : $cur -> $($Entry.Value) ($kind)" 'WHATIF' }
    } else {
        Remove-TweakRegistryValue -Path $Entry.Path -Name $Entry.Name
        if (-not $WhatIfPreference) { Remove-TweakStateEntry -Id $Id }
    }
}

function Remove-TweakRegistryValue {
    [CmdletBinding(SupportsShouldProcess = $true)]
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$Name)
    $info = Get-RegValueInfo -Path $Path -Name $Name
    $target = "$Path\$Name"
    if (-not $info.Existed) { Write-TweakLog "$target already absent" 'SKIP'; return }
    if ($PSCmdlet.ShouldProcess($target, 'Delete value')) {
        try {
            Remove-ItemProperty -LiteralPath $Path -Name $Name -Force -ErrorAction Stop
            Write-TweakLog "$target : $(Format-RegInfo $info) -> <absent>" 'SET'
        } catch { Write-TweakLog "$target : delete failed ($($_.Exception.Message))" 'ERROR' }
    } else { Write-TweakLog "$target : $(Format-RegInfo $info) -> <absent>" 'WHATIF' }
}

# ---------------- services ----------------
function Get-ServiceStartInfo {
    param([Parameter(Mandatory)][string]$Name)
    $p = "HKLM:\SYSTEM\CurrentControlSet\Services\$Name"
    if (-not (Test-Path -LiteralPath $p)) { return $null }
    $i = Get-ItemProperty -LiteralPath $p -ErrorAction SilentlyContinue
    $delayed = $null
    if ($i.PSObject.Properties.Name -contains 'DelayedAutostart') { $delayed = $i.DelayedAutostart }
    return [pscustomobject]@{ Start = $i.Start; DelayedAutostart = $delayed; RegPath = $p }
}

function Set-TweakServiceStart {
    <# Start: 2=Automatic 3=Manual 4=Disabled. Tries SCM (Set-Service) first, then the service's registry Start value. #>
    [CmdletBinding(SupportsShouldProcess = $true)]
    param([Parameter(Mandatory)][string]$Name, [Parameter(Mandatory)][ValidateSet(2,3,4)][int]$Start,
          [string]$Category = 'General', [switch]$StopNow, [Nullable[int]]$DelayedAutostart = $null, [switch]$Force)
    $info = Get-ServiceStartInfo -Name $Name
    if (-not $info) { Write-TweakLog "Service $Name not present - skipped" 'SKIP'; return }
    if ($Force -and $Start -eq 4 -and ($script:LockServices -contains $Name) -and -not $WhatIfPreference) { Lock-TweakServiceKey -Name $Name -DenySystem | Out-Null }
    $names = @{ 2 = 'Automatic'; 3 = 'Manual'; 4 = 'Disabled' }
    $target = "Service $Name (Start)"
    if ($info.Start -eq $Start) {
        Write-TweakLog "$target already = $Start ($($names[$Start]))" 'SKIP'
    } elseif ($PSCmdlet.ShouldProcess($target, "Set Start = $Start ($($names[$Start])) (was $($info.Start))")) {
        Save-TweakOriginal -Id "SVC|$Name" -Data @{ Category = $Category; Kind = 'Service'; Name = $Name; Start = $info.Start; DelayedAutostart = $info.DelayedAutostart }
        $done = $false
        try {
            Set-Service -Name $Name -StartupType $names[$Start] -ErrorAction Stop
            $done = $true
            Write-TweakLog "$target : $($info.Start) -> $Start ($($names[$Start])) via SCM" 'SET'
        } catch {
            try {
                Set-ItemProperty -LiteralPath $info.RegPath -Name Start -Value $Start -Type DWord -ErrorAction Stop
                $done = $true
                Write-TweakLog "$target : $($info.Start) -> $Start ($($names[$Start])) via registry (SCM refused; effective after reboot)" 'SET'
            } catch {
                $err = $_.Exception.Message
                if ($Force -and (Lock-TweakServiceKey -Name $Name)) {
                    try {
                        Set-ItemProperty -LiteralPath $info.RegPath -Name Start -Value $Start -Type DWord -ErrorAction Stop
                        $done = $true
                        Write-TweakLog "$target : $($info.Start) -> $Start ($($names[$Start])) via registry after taking the service KEY (effective after reboot)" 'SET'
                    } catch { Write-TweakLog "$target : could not force ($($_.Exception.Message))" 'ERROR' }
                } elseif ($Force) {
                    Write-TweakLog "$target : could not force ($err)" 'ERROR'
                } else {
                    Write-TweakLog "$target : Windows refused the change (protected service: $err)." 'WARN'
                }
            }
        }
        if ($done -and $null -ne $DelayedAutostart) {
            try { Set-ItemProperty -LiteralPath $info.RegPath -Name DelayedAutostart -Value ([int]$DelayedAutostart) -Type DWord -ErrorAction Stop } catch { }
        }
    } else {
        Write-TweakLog "$target : $($info.Start) -> $Start ($($names[$Start]))" 'WHATIF'
    }
    if ($StopNow) {
        $svc = Get-Service -Name $Name -ErrorAction SilentlyContinue
        if ($svc -and $svc.Status -ne 'Stopped') {
            if ($PSCmdlet.ShouldProcess("Service $Name", 'Stop')) {
                try { Stop-Service -Name $Name -Force -ErrorAction Stop; Write-TweakLog "Service $Name : $($svc.Status) -> Stopped" 'SET' }
                catch { Write-TweakLog "Service $Name : could not stop now ($($_.Exception.Message)); it will stay down after reboot if Start=4 stuck" 'WARN' }
            } else { Write-TweakLog "Service $Name : $($svc.Status) -> Stopped" 'WHATIF' }
        }
    }
}

# ---------------- scheduled tasks ----------------
function Disable-TweakTask {
    [CmdletBinding(SupportsShouldProcess = $true)]
    param([Parameter(Mandatory)][string]$TaskPath, [Parameter(Mandatory)][string]$TaskName, [string]$Category = 'General', [switch]$Force)
    $t = Get-ScheduledTask -TaskPath $TaskPath -TaskName $TaskName -ErrorAction SilentlyContinue
    $target = "Task $TaskPath$TaskName"
    if (-not $t) { Write-TweakLog "$target not present - skipped" 'SKIP'; return }
    if ($t.State -eq 'Disabled') { Write-TweakLog "$target already Disabled" 'SKIP'; return }
    if ($PSCmdlet.ShouldProcess($target, "Disable (was $($t.State))")) {
        try {
            Disable-ScheduledTask -TaskPath $TaskPath -TaskName $TaskName -ErrorAction Stop | Out-Null
            Save-TweakOriginal -Id "TASK|$TaskPath$TaskName" -Data @{ Category = $Category; Kind = 'Task'; TaskPath = $TaskPath; TaskName = $TaskName; State = "$($t.State)" }
            Write-TweakLog "$target : $($t.State) -> Disabled" 'SET'
        } catch {
            $why = $_.Exception.Message
            if (-not $Force) { Write-TweakLog "$target : Windows refused (protected task: $why). Left as-is." 'WARN'; return }
            # Admins are denied on protected tasks, SYSTEM is not: run schtasks once as SYSTEM via a throw-away task. No ACL/file changes.
            Invoke-TweakAsSystem -Execute 'schtasks.exe' -Argument ('/Change /TN "{0}{1}" /DISABLE' -f $TaskPath, $TaskName) | Out-Null
            $now = (Get-ScheduledTask -TaskPath $TaskPath -TaskName $TaskName -ErrorAction SilentlyContinue).State
            if ("$now" -eq 'Disabled') {
                Save-TweakOriginal -Id "TASK|$TaskPath$TaskName" -Data @{ Category = $Category; Kind = 'Task'; TaskPath = $TaskPath; TaskName = $TaskName; State = "$($t.State)"; Method = 'System' }
                Write-TweakLog "$target : $($t.State) -> Disabled (as SYSTEM; admin was refused: $why)" 'SET'
            } else { Write-TweakLog "$target : refused even as SYSTEM (now $now). Harmless while the update services are Start=4." 'WARN' }
        }
    } else { Write-TweakLog "$target : $($t.State) -> Disabled" 'WHATIF' }
}

function Disable-TweakTaskFolder {
    [CmdletBinding(SupportsShouldProcess = $true)]
    param([Parameter(Mandatory)][string]$TaskPath, [string]$Category = 'General', [switch]$Force)
    $tasks = @(Get-ScheduledTask -TaskPath $TaskPath -ErrorAction SilentlyContinue)
    if ($tasks.Count -eq 0) { Write-TweakLog "Task folder $TaskPath empty or not present" 'SKIP'; return }
    foreach ($t in $tasks) { Disable-TweakTask -TaskPath $t.TaskPath -TaskName $t.TaskName -Category $Category -Force:$Force }
}

function Get-OsSummary {
    $cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -ErrorAction SilentlyContinue
    return [pscustomobject]@{
        ProductName    = $cv.ProductName
        EditionID      = $cv.EditionID
        DisplayVersion = $cv.DisplayVersion
        Build          = ('{0}.{1}' -f $cv.CurrentBuild, $cv.UBR)
    }
}

function Write-TweakSummary {
    param([string]$Name)
    if ($script:TweakErrors -gt 0) { Write-TweakLog "=== $Name finished with $($script:TweakErrors) error(s). Log: $($script:LogFile) ===" 'WARN' }
    else { Write-TweakLog "=== $Name finished. Log: $($script:LogFile) ===" 'OK' }
}

# ---------------- manifest (used by Verify-Tweaks / Revert-Tweaks) ----------------
function Get-TweakManifest {
    <# Every registry value this kit writes. Level: Core = expected to work on Win10 Pro 22H2;
       BestEffort = written, but Microsoft scopes it to other editions/OS (WARN instead of FAIL). #>
    $WU  = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate'
    $AU  = "$WU\AU"
    $cdm = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager'
    $ccU = 'HKCU:\Software\Policies\Microsoft\Windows\CloudContent'
    $ccM = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent'
    $adv = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced'
    $m = @(
        @('Updates', $AU, 'NoAutoUpdate', 'DWord', 1, 'Core'),
        @('Updates', $AU, 'AUOptions', 'DWord', 1, 'Core'),
        @('Updates', $AU, 'NoAutoRebootWithLoggedOnUsers', 'DWord', 1, 'Core'),
        @('Updates', $WU, 'SetDisableUXWUAccess', 'DWord', 1, 'Core'),
        @('Updates', 'HKLM:\SOFTWARE\Policies\Microsoft\WindowsStore', 'AutoDownload', 'DWord', 2, 'Core'),
        # Hard kill (skipped with -PolicyOnly). DoNotConnect / DisableWindowsUpdateAccess only apply with an intranet server set,
        # so WU is pointed at a local address that serves nothing.
        @('Updates', $WU, 'DoNotConnectToWindowsUpdateInternetLocations', 'DWord', 1, 'Hard'),
        @('Updates', $WU, 'DisableWindowsUpdateAccess', 'DWord', 1, 'Hard'),
        @('Updates', $WU, 'WUServer', 'String', $script:DeadWsus, 'Hard'),
        @('Updates', $WU, 'WUStatusServer', 'String', $script:DeadWsus, 'Hard'),
        @('Updates', $AU, 'UseWUServer', 'DWord', 1, 'Hard'),
        @('Updates', 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\DeliveryOptimization', 'DODownloadMode', 'DWord', 99, 'Hard'),
        @('Notifications', $WU, 'SetUpdateNotificationLevel', 'DWord', 1, 'Core'),
        @('Notifications', $WU, 'UpdateNotificationLevel', 'DWord', 2, 'Core'),
        @('Notifications', 'HKLM:\SOFTWARE\Microsoft\WindowsUpdate\UX\Settings', 'RestartNotificationsAllowed2', 'DWord', 0, 'BestEffort'),
        @('Notifications', 'HKCU:\Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement', 'ScoobeSystemSettingEnabled', 'DWord', 0, 'Core'),
        @('BlockWin11', $WU, 'TargetReleaseVersion', 'DWord', 1, 'Core'),
        @('BlockWin11', $WU, 'ProductVersion', 'String', 'Windows 10', 'Core'),
        @('BlockWin11', $WU, 'TargetReleaseVersionInfo', 'String', '22H2', 'Core'),
        @('BlockWin11', $WU, 'DisableOSUpgrade', 'DWord', 1, 'Core'),
        @('BlockWin11', 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\OSUpgrade', 'AllowOSUpgrade', 'DWord', 0, 'Core'),
        @('BlockWin11', 'HKLM:\SOFTWARE\Policies\Microsoft\WindowsStore', 'DisableOSUpgrade', 'DWord', 1, 'Core'),
        @('Copilot', 'HKCU:\Software\Policies\Microsoft\Windows\WindowsCopilot', 'TurnOffWindowsCopilot', 'DWord', 1, 'Core'),
        @('Copilot', 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsCopilot', 'TurnOffWindowsCopilot', 'DWord', 1, 'Core'),
        @('Copilot', $adv, 'ShowCopilotButton', 'DWord', 0, 'Core'),
        @('Copilot', 'HKLM:\SOFTWARE\Policies\Microsoft\Edge', 'HubsSidebarEnabled', 'DWord', 0, 'Core')
    )
    foreach ($n in 'RotatingLockScreenEnabled','RotatingLockScreenOverlayEnabled','SubscribedContent-338387Enabled','SubscribedContent-338388Enabled','SubscribedContent-338389Enabled','SubscribedContent-338393Enabled','SubscribedContent-353694Enabled','SubscribedContent-353696Enabled','SubscribedContent-310093Enabled','SystemPaneSuggestionsEnabled','SoftLandingEnabled','SilentInstalledAppsEnabled','PreInstalledAppsEnabled','OemPreInstalledAppsEnabled') {
        $m += ,@('Spotlight', $cdm, $n, 'DWord', 0, 'Core')
    }
    $m += @(
        @('Spotlight', $ccU, 'DisableWindowsSpotlightFeatures', 'DWord', 1, 'BestEffort'),
        @('Spotlight', $ccU, 'ConfigureWindowsSpotlight', 'DWord', 2, 'BestEffort'),
        @('Spotlight', $ccU, 'DisableWindowsSpotlightOnActionCenter', 'DWord', 1, 'BestEffort'),
        @('Spotlight', $ccU, 'DisableWindowsSpotlightOnSettings', 'DWord', 1, 'BestEffort'),
        @('Spotlight', $ccU, 'DisableWindowsSpotlightWindowsWelcomeExperience', 'DWord', 1, 'BestEffort'),
        @('Spotlight', $ccU, 'DisableThirdPartySuggestions', 'DWord', 1, 'BestEffort'),
        @('Spotlight', $ccU, 'DisableTailoredExperiencesWithDiagnosticData', 'DWord', 1, 'BestEffort'),
        @('Spotlight', $ccM, 'DisableWindowsConsumerFeatures', 'DWord', 1, 'BestEffort'),
        @('Spotlight', $ccM, 'DisableSoftLanding', 'DWord', 1, 'BestEffort'),
        @('Spotlight', $ccM, 'DisableCloudOptimizedContent', 'DWord', 1, 'BestEffort'),
        @('Spotlight', 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Feeds', 'EnableFeeds', 'DWord', 0, 'Core'),
        @('Spotlight', 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Feeds', 'ShellFeedsTaskbarViewMode', 'DWord', 2, 'BestEffort'),
        @('Spotlight', 'HKLM:\SOFTWARE\Policies\Microsoft\Dsh', 'AllowNewsAndInterests', 'DWord', 0, 'BestEffort'),
        @('Spotlight', 'HKLM:\SOFTWARE\Policies\Microsoft\Dsh', 'DisableWidgetsOnLockScreen', 'DWord', 1, 'BestEffort'),
        @('Spotlight', $adv, 'TaskbarDa', 'DWord', 0, 'BestEffort'),
        @('Spotlight', 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Search', 'EnableDynamicContentInWSB', 'DWord', 0, 'Core')
    )
    foreach ($r in $m) {
        [pscustomobject]@{ Category = $r[0]; Path = $r[1]; Name = $r[2]; Type = $r[3]; Value = $r[4]; Level = $r[5] }
    }
}

function Get-TweakServiceManifest {
    # Default = stock Windows 10 22H2 start type, used by Revert only when no recorded original exists.
    @(
        [pscustomobject]@{ Name = 'wuauserv';     Target = 4; DefaultStart = 3; DefaultDelayed = $null },
        [pscustomobject]@{ Name = 'UsoSvc';       Target = 4; DefaultStart = 2; DefaultDelayed = 1 },
        [pscustomobject]@{ Name = 'WaaSMedicSvc'; Target = 4; DefaultStart = 3; DefaultDelayed = $null },
        [pscustomobject]@{ Name = 'uhssvc';       Target = 4; DefaultStart = 2; DefaultDelayed = 1 }   # Microsoft Update Health Service (KB4023057), if installed
    )
}
$script:UpdateTaskFolders = @('\Microsoft\Windows\WaaSMedic\', '\Microsoft\Windows\UpdateOrchestrator\', '\Microsoft\Windows\WindowsUpdate\')
$script:CopilotFamilies   = @('Microsoft.Copilot_8wekyb3d8bbwe', 'Microsoft.MicrosoftOfficeHub_8wekyb3d8bbwe')

# ---------------- hard kill helpers (Updates, not used with -PolicyOnly) ----------------
function Enable-TweakPrivilege {
    <# Enables SeTakeOwnershipPrivilege / SeRestorePrivilege that an elevated admin token already holds. #>
    param([Parameter(Mandatory)][string[]]$Name)
    if (-not ('W10TPriv' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class W10TPriv {
    [StructLayout(LayoutKind.Sequential, Pack = 4)]
    struct TP1 { public int Count; public long Luid; public int Attr; }
    [DllImport("advapi32.dll", SetLastError = true)] static extern bool OpenProcessToken(IntPtr h, int acc, out IntPtr tok);
    [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)] static extern bool LookupPrivilegeValue(string sys, string name, out long luid);
    [DllImport("advapi32.dll", SetLastError = true)] static extern bool AdjustTokenPrivileges(IntPtr tok, bool all, ref TP1 ns, int len, IntPtr prev, IntPtr rl);
    [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    public static void Enable(string name) {
        IntPtr t;
        if (!OpenProcessToken(GetCurrentProcess(), 0x28, out t)) throw new System.ComponentModel.Win32Exception();
        try {
            TP1 tp = new TP1(); tp.Count = 1; tp.Attr = 2;
            if (!LookupPrivilegeValue(null, name, out tp.Luid)) throw new System.ComponentModel.Win32Exception();
            bool ok = AdjustTokenPrivileges(t, false, ref tp, 0, IntPtr.Zero, IntPtr.Zero);
            int err = Marshal.GetLastWin32Error();
            if (!ok || err == 1300) throw new System.ComponentModel.Win32Exception(err);
        } finally { CloseHandle(t); }
    }
}
'@
    }
    foreach ($n in $Name) { [W10TPriv]::Enable($n) }
}

function Get-TweakKeySddl {
    param([Parameter(Mandatory)][string]$SubKey)   # under HKLM
    $k = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($SubKey, [Microsoft.Win32.RegistryKeyPermissionCheck]::Default, [System.Security.AccessControl.RegistryRights]::ReadPermissions)
    if (-not $k) { throw "HKLM\$SubKey not found" }
    try { return $k.GetAccessControl('Owner,Group,Access').GetSecurityDescriptorSddlForm('Owner,Group,Access') } finally { $k.Close() }
}

function Set-TweakKeySddl {
    param([Parameter(Mandatory)][string]$SubKey, [Parameter(Mandatory)][string]$Sddl, [Parameter(Mandatory)][System.Security.AccessControl.AccessControlSections]$Sections)
    $rights = [System.Security.AccessControl.RegistryRights]::ReadPermissions
    if ($Sections -band [System.Security.AccessControl.AccessControlSections]::Owner) { $rights = $rights -bor [System.Security.AccessControl.RegistryRights]::TakeOwnership }
    if ($Sections -band ([System.Security.AccessControl.AccessControlSections]::Access -bor [System.Security.AccessControl.AccessControlSections]::Group)) { $rights = $rights -bor [System.Security.AccessControl.RegistryRights]::ChangePermissions }
    $k = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($SubKey, [Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree, $rights)
    if (-not $k) { throw "HKLM\$SubKey not found" }
    try {
        $sec = New-Object System.Security.AccessControl.RegistrySecurity
        $sec.SetSecurityDescriptorSddlForm($Sddl, $Sections)
        $k.SetAccessControl($sec)
    } finally { $k.Close() }
}

function Lock-TweakServiceKey {
    <# Owner -> Administrators + Administrators FullControl on HKLM\SYSTEM\CurrentControlSet\Services\<Name> (the KEY, never the binary).
       -DenySystem also denies SYSTEM "Set Value" on that key so WaaSMedic/servicing can't write Start back (SYSTEM can still read it).
       Original owner + SDDL go to state.json (ACL|<path>); Revert puts them back. Returns $true when the key is ours. #>
    param([Parameter(Mandatory)][string]$Name, [switch]$DenySystem)
    $sub = "SYSTEM\CurrentControlSet\Services\$Name"; $path = "HKLM:\$sub"
    try { $cur = Get-TweakKeySddl -SubKey $sub } catch { Write-TweakLog "$path ACL : cannot read ($($_.Exception.Message))" 'ERROR'; return $false }
    $i = $cur.IndexOf('D:'); $d = if ($i -ge 0) { $cur.Substring($i + 2) } else { '' }
    $si = $d.IndexOf('S:'); if ($si -ge 0) { $d = $d.Substring(0, $si) }
    $j = $d.IndexOf('('); if ($j -lt 0) { $j = $d.Length }
    $flags = $d.Substring(0, $j); $aces = $d.Substring($j)
    if ($cur.StartsWith('O:BA') -and $aces.Contains($script:AdminFullAce) -and ($aces.Contains($script:LockDenyAce) -or -not $DenySystem)) {
        Write-TweakLog "$path ACL already taken" 'SKIP'; return $true
    }
    Save-TweakOriginal -Id "ACL|$path" -Data @{ Category = 'Updates'; Kind = 'KeyAcl'; Path = $path; Sddl = $cur }
    $rest = $aces.Replace($script:AdminFullAce, '').Replace($script:LockDenyAce, '')
    $deny = ''; if ($DenySystem) { $deny = $script:LockDenyAce }
    $dacl = "D:$flags$deny$($script:AdminFullAce)$rest"
    try {
        Enable-TweakPrivilege -Name 'SeTakeOwnershipPrivilege', 'SeRestorePrivilege'
        Set-TweakKeySddl -SubKey $sub -Sddl 'O:BA' -Sections Owner
        Set-TweakKeySddl -SubKey $sub -Sddl $dacl -Sections Access
        Write-TweakLog "$path ACL : $(($cur -split 'G:')[0]) -> O:BA; DACL + $($script:AdminFullAce)$(if ($DenySystem) { " + $($script:LockDenyAce) [SYSTEM may read, not write]" }) (original recorded)" 'SET'
        return $true
    } catch { Write-TweakLog "$path ACL : take ownership failed ($($_.Exception.Message))" 'ERROR'; return $false }
}

function Restore-TweakKeyAcls {
    $state = Get-TweakState
    foreach ($id in @($state.Keys | Where-Object { $_ -like 'ACL|*' })) {
        $e = $state[$id]; $sub = ($e.Path -replace '^HKLM:\\', '')
        try {
            Enable-TweakPrivilege -Name 'SeTakeOwnershipPrivilege', 'SeRestorePrivilege'
            Set-TweakKeySddl -SubKey $sub -Sddl $e.Sddl -Sections 'Owner,Group,Access'
            Write-TweakLog "$($e.Path) ACL : -> $($e.Sddl) [original owner + permissions restored]" 'SET'
            Remove-TweakStateEntry -Id $id
        } catch { Write-TweakLog "$($e.Path) ACL : restore failed ($($_.Exception.Message)); original kept in state.json" 'ERROR' }
    }
}

function Invoke-TweakAsSystem {
    <# Runs one command as SYSTEM through a throw-away scheduled task (deleted right after). Returns LastTaskResult. #>
    param([Parameter(Mandatory)][string]$Execute, [string]$Argument = '')
    $n = 'Win10Tweaks-SysHelper'; $tp = '\Win10Tweaks\'
    $a = New-ScheduledTaskAction -Execute $Execute -Argument $Argument
    $p = New-ScheduledTaskPrincipal -UserId 'NT AUTHORITY\SYSTEM' -LogonType ServiceAccount -RunLevel Highest
    Register-ScheduledTask -TaskName $n -TaskPath $tp -Action $a -Principal $p -Force | Out-Null
    try {
        Start-ScheduledTask -TaskName $n -TaskPath $tp
        $i = 0
        do { Start-Sleep -Milliseconds 400; $i++; $t = Get-ScheduledTask -TaskName $n -TaskPath $tp; $inf = Get-ScheduledTaskInfo -TaskName $n -TaskPath $tp }
        while (($t.State -eq 'Running' -or $inf.LastTaskResult -eq 267011) -and $i -lt 75)
        return $inf.LastTaskResult
    } finally { Unregister-ScheduledTask -TaskName $n -TaskPath $tp -Confirm:$false -ErrorAction SilentlyContinue }
}

function Enable-TweakTaskAny {
    param([Parameter(Mandatory)][string]$TaskPath, [Parameter(Mandatory)][string]$TaskName)
    try { Enable-ScheduledTask -TaskPath $TaskPath -TaskName $TaskName -ErrorAction Stop | Out-Null; return $true } catch { }
    Invoke-TweakAsSystem -Execute 'schtasks.exe' -Argument ('/Change /TN "{0}{1}" /ENABLE' -f $TaskPath, $TaskName) | Out-Null
    return ("$((Get-ScheduledTask -TaskPath $TaskPath -TaskName $TaskName -ErrorAction SilentlyContinue).State)" -ne 'Disabled')
}

function Set-TweakFirewallBlock {
    [CmdletBinding(SupportsShouldProcess = $true)]
    param()
    foreach ($s in $script:FwServices) {
        $n = "$($script:FwPrefix)$s"
        if (-not (Get-Service -Name $s -ErrorAction SilentlyContinue)) { Write-TweakLog "Firewall ${n}: service $s not present" 'SKIP'; continue }
        $r = Get-NetFirewallRule -Name $n -ErrorAction SilentlyContinue
        if ($r -and "$($r.Enabled)" -eq 'True') { Write-TweakLog "Firewall rule $n already present" 'SKIP'; continue }
        if (-not $PSCmdlet.ShouldProcess($n, "Block outbound for service $s")) { Write-TweakLog "Firewall rule $n : <absent> -> Block outbound ($s)" 'WHATIF'; continue }
        try {
            if ($r) { Enable-NetFirewallRule -Name $n -ErrorAction Stop; Write-TweakLog "Firewall rule $n : disabled -> enabled" 'SET' }
            else {
                New-NetFirewallRule -Name $n -DisplayName $n -Group 'Win10Tweaks' -Description "Win10Tweaks: block outbound traffic of update service $s" -Direction Outbound -Action Block -Profile Any -Service $s -ErrorAction Stop | Out-Null
                Save-TweakOriginal -Id "FW|$n" -Data @{ Category = 'Updates'; Kind = 'Firewall'; Name = $n }
                Write-TweakLog "Firewall rule $n : <absent> -> Block outbound for service $s" 'SET'
            }
        } catch { Write-TweakLog "Firewall rule $n : $($_.Exception.Message)" 'ERROR' }
    }
}

function Remove-TweakFirewallRules {
    [CmdletBinding(SupportsShouldProcess = $true)]
    param()
    $rules = @(Get-NetFirewallRule -PolicyStore PersistentStore -ErrorAction SilentlyContinue | Where-Object { $_.Name -like "$($script:FwPrefix)*" })
    if ($rules.Count -eq 0) { Write-TweakLog "No $($script:FwPrefix)* firewall rules present" 'SKIP' }
    foreach ($r in $rules) {
        if ($PSCmdlet.ShouldProcess($r.Name, 'Delete firewall rule')) {
            try { Remove-NetFirewallRule -Name $r.Name -ErrorAction Stop; Write-TweakLog "Firewall rule $($r.Name) : present -> deleted" 'SET'; Remove-TweakStateEntry -Id "FW|$($r.Name)" }
            catch { Write-TweakLog "Firewall rule $($r.Name) : $($_.Exception.Message)" 'ERROR' }
        } else { Write-TweakLog "Firewall rule $($r.Name) : present -> deleted" 'WHATIF' }
    }
}

function Set-TweakUpdatesMode {
    param([bool]$PolicyOnly)
    if ($WhatIfPreference) { return }
    $state = Get-TweakState
    $state['MODE|Updates'] = [pscustomobject]@{ Category = 'Updates'; Kind = 'Mode'; PolicyOnly = $PolicyOnly; RecordedAt = (Get-Date).ToString('s') }
    if (-not (Test-Path -LiteralPath $script:TweakRoot)) { New-Item -ItemType Directory -Path $script:TweakRoot -Force | Out-Null }
    ($state | ConvertTo-Json -Depth 6) | Set-Content -LiteralPath $script:StateFile -Encoding UTF8 -WhatIf:$false -Confirm:$false
}

function Clear-TweakTempAllow {
    <# Ends a temporary allow: removes tempallow.json and the one-shot restore task (if the GUI created one). #>
    if ($WhatIfPreference) { return }
    if (Test-Path -LiteralPath $script:TempAllowFile) { Remove-Item -LiteralPath $script:TempAllowFile -Force; Write-TweakLog 'Temporary update allow : active -> ended' 'SET' }
    if (Get-ScheduledTask -TaskPath $script:RestoreTaskPath -TaskName $script:RestoreTaskName -ErrorAction SilentlyContinue) {
        Unregister-ScheduledTask -TaskPath $script:RestoreTaskPath -TaskName $script:RestoreTaskName -Confirm:$false
        Write-TweakLog "Task $($script:RestoreTaskPath)$($script:RestoreTaskName) : registered -> removed" 'SET'
    }
}

function Get-TweakTempAllow {
    if (-not (Test-Path -LiteralPath $script:TempAllowFile)) { return $null }
    try { $o = Get-Content -LiteralPath $script:TempAllowFile -Raw | ConvertFrom-Json; if ($o.Active) { return $o } } catch { }
    return $null
}
