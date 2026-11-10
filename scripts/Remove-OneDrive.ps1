#requires -Version 5.1
<#
.SYNOPSIS
  OneDrive removal (Windows 10 22H2), scan-gated. Same steps and state.json IDs as the app's OneDrive wizard.

.DESCRIPTION
  Default (no switches): READ-ONLY check. Reports PASS / WARN / FAIL for:
    - OneDrive installed / running, linked accounts (HKCU\Software\Microsoft\OneDrive\Accounts\*)
    - OneDrive folders (account UserFolder, %OneDrive%, %OneDriveConsumer%, %OneDriveCommercial%)
    - Known folders (User Shell Folders) still inside OneDrive: Desktop, Documents, Pictures, Music, Videos, Downloads
    - Cloud-only files (attributes RECALL_ON_DATA_ACCESS 0x400000, RECALL_ON_OPEN 0x40000, OFFLINE 0x1000).
      Listing a folder reads attributes only; nothing is downloaded. Cloud-only folders are counted, not entered.
  -Download  : asks OneDrive to keep every file on this PC (attrib +P -U /S /D, i.e. "Always keep on this device").
  -Remove    : REFUSES unless the check passes (no known folder in OneDrive, no cloud-only files), or you pass
               -IKnowMyFilesAreSafe. Always refuses when the elevated account is not the signed-in user.
               Steps: restore point (unless -NoRestorePoint); stop OneDrive; HKCU Run\OneDrive -> removed (recorded);
               OneDrive* scheduled tasks -> Disabled (recorded); uninstall via OneDriveSetup.exe /uninstall
               (per-machine installs: /uninstall /allusers; winget only as a last resort);
               policies DisableFileSyncNGSC=1 (HKLM\SOFTWARE\Policies\Microsoft\Windows\OneDrive),
               DisablePersonalSync=1 (HKCU\SOFTWARE\Policies\Microsoft\OneDrive),
               KFMBlockOptIn=1 + PreventNetworkTrafficPreUserSignIn=1 (HKLM\SOFTWARE\Policies\Microsoft\OneDrive);
               HKCU Run\OneDriveSetup removed; Default user (new accounts) Run\OneDriveSetup removed (unless -KeepNewUserSetup);
               Explorer side bar entry {018D5C66-...} System.IsPinnedToNameSpaceTree=0 (unless -KeepNavPane).
               The OneDrive folder and its files are NEVER deleted.
  -Revert    : puts back everything recorded above (does not reinstall OneDrive; prints how).
  -WhatIf    : show what would change.
  Sources: learn.microsoft.com/sharepoint/use-group-policy, /files-on-demand-windows, /per-machine-installation;
  support.microsoft.com "Turn off, disable, or uninstall OneDrive".
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [switch]$Download,
    [switch]$Remove,
    [switch]$Revert,
    [switch]$IKnowMyFilesAreSafe,
    [switch]$KeepNavPane,
    [switch]$KeepNewUserSetup,
    [switch]$NoRestorePoint
)

. (Join-Path $PSScriptRoot 'TweakCommon.ps1')
if (Invoke-SelfElevate -ScriptPath $PSCommandPath -BoundParameters $PSBoundParameters) { return }
Initialize-TweakLog -Name 'Remove-OneDrive'
$cat = 'OneDrive'
$RunCU = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$Usf = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders'
$NavClsid = '{018D5C66-4533-4307-9B53-224DE2ED1FE6}'
$NavKeys = @(
    "HKCU:\Software\Classes\CLSID\$NavClsid", "HKCU:\Software\Classes\WOW6432Node\CLSID\$NavClsid",
    "HKLM:\SOFTWARE\Classes\CLSID\$NavClsid", "HKLM:\SOFTWARE\Classes\WOW6432Node\CLSID\$NavClsid")
$UninstKeys = @(
    @{ Key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\OneDriveSetup.exe'; Machine = $false },
    @{ Key = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\OneDriveSetup.exe'; Machine = $true },
    @{ Key = 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\OneDriveSetup.exe'; Machine = $true })
$Known = @(
    @{ Label = 'Desktop'; Name = 'Desktop'; Sub = 'Desktop' }, @{ Label = 'Documents'; Name = 'Personal'; Sub = 'Documents' },
    @{ Label = 'Pictures'; Name = 'My Pictures'; Sub = 'Pictures' }, @{ Label = 'Music'; Name = 'My Music'; Sub = 'Music' },
    @{ Label = 'Videos'; Name = 'My Video'; Sub = 'Videos' }, @{ Label = 'Downloads'; Name = '{374DE290-123F-4565-9164-39C4925E467B}'; Sub = 'Downloads' })
$CloudMask = 0x400000 -bor 0x40000 -bor 0x1000
$DefHive = 'HKU\Win10Tweaks_DefaultUser'
$DefRun = 'Registry::HKEY_USERS\Win10Tweaks_DefaultUser\Software\Microsoft\Windows\CurrentVersion\Run'

function Get-SessionUserMismatch {
    $console = (Get-CimInstance Win32_ComputerSystem -ErrorAction SilentlyContinue).UserName   # DOMAIN\user of the console session
    $me = "$env:USERDOMAIN\$env:USERNAME"
    if (-not $console) { return $null }
    if ($console -ieq $me -or ($console -split '\\')[-1] -ieq $env:USERNAME) { return $null }
    return @($me, $console)
}

function Get-OdAccounts {
    $base = 'HKCU:\Software\Microsoft\OneDrive\Accounts'
    if (-not (Test-Path $base)) { return @() }
    Get-ChildItem $base -ErrorAction SilentlyContinue | ForEach-Object {
        $p = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
        if ($p.UserFolder) { [pscustomobject]@{ Kind = $_.PSChildName; Email = $p.UserEmail; Folder = $p.UserFolder } }
    }
}

function Get-OdRoots($accts) {
    $c = @($accts | ForEach-Object { $_.Folder }) + @($env:OneDrive, $env:OneDriveConsumer, $env:OneDriveCommercial)
    $c = @($c | Where-Object { $_ -and (Test-Path -LiteralPath $_) } | ForEach-Object { (Get-Item -LiteralPath $_).FullName.TrimEnd('\') } | Sort-Object -Unique)
    @($c | Where-Object { $x = $_; -not ($c | Where-Object { $_ -ne $x -and $x.StartsWith("$_\", 'OrdinalIgnoreCase') }) })
}

function Get-OdInstalled {
    $w = @()
    foreach ($u in $UninstKeys) { if ((Get-RegValueInfo -Path $u.Key -Name 'UninstallString').Existed) { $w += $u.Key } }
    foreach ($p in @("$env:LOCALAPPDATA\Microsoft\OneDrive\OneDrive.exe", "$env:ProgramFiles\Microsoft OneDrive\OneDrive.exe", "${env:ProgramFiles(x86)}\Microsoft OneDrive\OneDrive.exe")) {
        if ($p -and (Test-Path -LiteralPath $p)) { $w += $p }
    }
    $w
}

function Invoke-OdScan {
    $r = [ordered]@{ Mismatch = Get-SessionUserMismatch; Accounts = @(Get-OdAccounts); Known = @(); Files = 0; Cloud = 0; CloudBytes = [int64]0; CloudDirs = 0; Errors = 0 }
    $r.Roots = @(Get-OdRoots $r.Accounts)
    $r.Installed = @(Get-OdInstalled)
    $r.Running = [bool](Get-Process OneDrive -ErrorAction SilentlyContinue)
    if ($r.Mismatch) { Write-TweakLog "Elevated as $($r.Mismatch[0]) but $($r.Mismatch[1]) is signed in: this would check the wrong account." 'FAIL' }
    Write-TweakLog ("OneDrive app: {0}{1}" -f $(if ($r.Installed) { 'installed (' + ($r.Installed -join '; ') + ')' } else { 'not installed' }), $(if ($r.Running) { ', running' } else { '' })) 'INFO'
    foreach ($a in $r.Accounts) { Write-TweakLog "Linked account: $($a.Kind) $($a.Email) -> $($a.Folder) (unlink: OneDrive Settings > Account > Unlink this PC)" 'WARN' }
    foreach ($k in $Known) {
        $i = Get-RegValueInfo -Path $Usf -Name $k.Name
        if (-not $i.Existed) { continue }
        $path = [Environment]::ExpandEnvironmentVariables([string]$i.Value).TrimEnd('\')
        $in = [bool]($r.Roots | Where-Object { $path -ieq $_ -or $path.StartsWith("$_\", 'OrdinalIgnoreCase') })
        $r.Known += [pscustomobject]@{ Label = $k.Label; Name = $k.Name; Sub = $k.Sub; Path = $path; InOd = $in }
        if ($in) { Write-TweakLog "$($k.Label) -> $path is inside OneDrive (Settings > Sync and backup > Manage backup > stop backup, keep files on this PC)" 'FAIL' }
        else { Write-TweakLog "$($k.Label) -> $path" 'PASS' }
    }
    foreach ($root in $r.Roots) {
        $stack = New-Object System.Collections.Stack; $stack.Push([IO.DirectoryInfo]$root)
        while ($stack.Count) {
            $d = $stack.Pop()
            try { $items = $d.EnumerateFileSystemInfos() } catch { $r.Errors++; continue }
            try {
                foreach ($e in $items) {
                    $at = [int64]$e.Attributes
                    if ($e -is [IO.DirectoryInfo]) {
                        if ($at -band $CloudMask) { $r.CloudDirs++ }
                        else { $stack.Push($e) }
                    } else {
                        $r.Files++
                        if ($at -band $CloudMask) { $r.Cloud++; $r.CloudBytes += $e.Length }
                    }
                }
            } catch { $r.Errors++ }
        }
    }
    if ($r.Cloud -or $r.CloudDirs) { Write-TweakLog ("{0:N0} cloud-only files ({1:N1} MB) and {2:N0} cloud-only folders: run with -Download and wait" -f $r.Cloud, ($r.CloudBytes / 1MB), $r.CloudDirs) 'FAIL' }
    else { Write-TweakLog ("All {0:N0} files in OneDrive are on this PC" -f $r.Files) 'PASS' }
    if ($r.Errors) { Write-TweakLog "$($r.Errors) folders could not be read" 'WARN' }
    $r.Safe = (-not ($r.Known | Where-Object InOd)) -and -not $r.Cloud -and -not $r.CloudDirs
    [pscustomobject]$r
}

function Save-OdReg([string]$Path, [string]$Name) {
    $i = Get-RegValueInfo -Path $Path -Name $Name
    Save-TweakOriginal -Id "ODREG|$Path|$Name" -Data @{ Category = $cat; Kind = 'Registry'; Path = $Path; Name = $Name; Existed = $i.Existed; ValueKind = $i.Kind; Value = $i.Value }
}

function Remove-OdValue([string]$Path, [string]$Name) {
    if (-not (Get-RegValueInfo -Path $Path -Name $Name).Existed) { Write-TweakLog "$Path\$Name not present" 'SKIP'; return }
    if ($PSCmdlet.ShouldProcess("$Path\$Name", 'Delete (original recorded)')) { Save-OdReg $Path $Name }
    Remove-TweakRegistryValue -Path $Path -Name $Name
}

function Stop-OdSync {
    if ($PSCmdlet.ShouldProcess('OneDrive.exe', 'Stop')) {
        Get-Process OneDrive -ErrorAction SilentlyContinue | ForEach-Object { Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue; Write-TweakLog "OneDrive.exe (pid $($_.Id)) : running -> stopped" 'SET' }
    }
    Remove-OdValue $RunCU 'OneDrive'
    foreach ($t in @(Get-ScheduledTask -ErrorAction SilentlyContinue | Where-Object { $_.TaskName -like 'OneDrive*' })) {
        Disable-TweakTask -TaskPath $t.TaskPath -TaskName $t.TaskName -Category $cat
    }
}

function Invoke-OdUninstall {
    if (-not (Get-OdInstalled)) { Write-TweakLog 'OneDrive app already not installed' 'SKIP'; return }
    $cands = @()
    foreach ($u in $UninstKeys) {
        $i = Get-RegValueInfo -Path $u.Key -Name 'UninstallString'
        if ($i.Existed) {
            $s = [Environment]::ExpandEnvironmentVariables([string]$i.Value).Trim()
            $exe = if ($s.StartsWith('"')) { $s.Split('"')[1] } else { $s.Substring(0, $s.ToLower().IndexOf('.exe') + 4) }
            $cands += @{ Exe = $exe; Args = $(if ($u.Machine) { '/uninstall /allusers' } else { '/uninstall' }) }
        }
    }
    $newest = { param($b) Get-ChildItem -LiteralPath $b -Directory -ErrorAction SilentlyContinue | Where-Object { Test-Path (Join-Path $_.FullName 'OneDriveSetup.exe') } | Sort-Object { try { [version]$_.Name } catch { [version]'0.0' } } | Select-Object -Last 1 }
    $d = & $newest "$env:LOCALAPPDATA\Microsoft\OneDrive"; if ($d) { $cands += @{ Exe = Join-Path $d.FullName 'OneDriveSetup.exe'; Args = '/uninstall' } }
    foreach ($pfr in @($env:ProgramFiles, ${env:ProgramFiles(x86)})) { if ($pfr) { $d = & $newest "$pfr\Microsoft OneDrive"; if ($d) { $cands += @{ Exe = Join-Path $d.FullName 'OneDriveSetup.exe'; Args = '/uninstall /allusers' } } } }
    foreach ($sub in 'System32', 'SysWOW64') { $cands += @{ Exe = "$env:SystemRoot\$sub\OneDriveSetup.exe"; Args = '/uninstall' } }
    $seen = @{}
    foreach ($c in $cands) {
        if (-not (Test-Path -LiteralPath $c.Exe) -or $seen[$c.Exe.ToLower()]) { continue }
        $seen[$c.Exe.ToLower()] = 1
        if (-not $PSCmdlet.ShouldProcess("`"$($c.Exe)`" $($c.Args)", 'Run uninstaller')) { continue }
        Write-TweakLog "Uninstalling: `"$($c.Exe)`" $($c.Args)" 'INFO'
        try { Start-Process -FilePath $c.Exe -ArgumentList $c.Args -Wait -ErrorAction Stop } catch { Write-TweakLog "  failed: $($_.Exception.Message)" 'WARN' }
        for ($n = 0; $n -lt 60 -and (Get-OdInstalled); $n++) { Start-Sleep 1 }
        if (-not (Get-OdInstalled)) { Write-TweakLog 'OneDrive app : installed -> removed' 'SET'; return }
    }
    if (-not $WhatIfPreference -and (Get-Command winget -ErrorAction SilentlyContinue)) {
        Write-TweakLog 'Trying winget (may contact the winget source)' 'WARN'
        & winget uninstall --id Microsoft.OneDrive --silent --accept-source-agreements 2>&1 | Select-Object -Last 2 | ForEach-Object { Write-TweakLog "  winget: $_" 'INFO' }
    }
    if (Get-OdInstalled) { Write-TweakLog 'OneDrive is still installed. The policies below still stop it syncing.' 'WARN' } else { Write-TweakLog 'OneDrive app : installed -> removed' 'SET' }
}

function Invoke-WithDefaultHive([scriptblock]$Do) {
    $file = Join-Path (Split-Path $env:PUBLIC) 'Default\NTUSER.DAT'
    if ($WhatIfPreference) { Write-TweakLog "Would load $file and remove Run\OneDriveSetup" 'WHATIF'; return }
    & reg.exe load $DefHive $file 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { Write-TweakLog "New-user template $file could not be opened; skipped" 'WARN'; return }
    try { & $Do } finally {
        [gc]::Collect(); [gc]::WaitForPendingFinalizers()
        for ($n = 0; $n -lt 5; $n++) { & reg.exe unload $DefHive 2>&1 | Out-Null; if ($LASTEXITCODE -eq 0) { break }; Start-Sleep -Milliseconds 600 }
    }
}

function Invoke-OdRemove {
    if (-not $NoRestorePoint -and $PSCmdlet.ShouldProcess('System Restore', 'Create restore point')) {
        try { Checkpoint-Computer -Description 'Win10Tweaks OneDrive removal' -RestorePointType MODIFY_SETTINGS -ErrorAction Stop; Write-TweakLog 'Restore point created' 'OK' }
        catch { Write-TweakLog "Restore point failed: $($_.Exception.Message). Use -NoRestorePoint to go on without one." 'ERROR'; return }
    }
    $roots = @(Get-OdRoots @(Get-OdAccounts))
    Write-TweakLog '--- Stop sync ---'; Stop-OdSync
    Write-TweakLog '--- Uninstall ---'; Invoke-OdUninstall
    Write-TweakLog '--- Blocks (policies) ---'
    Set-TweakRegistry -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\OneDrive' -Name 'DisableFileSyncNGSC' -Type DWord -Value 1 -Category $cat
    Set-TweakRegistry -Path 'HKCU:\SOFTWARE\Policies\Microsoft\OneDrive' -Name 'DisablePersonalSync' -Type DWord -Value 1 -Category $cat
    Set-TweakRegistry -Path 'HKLM:\SOFTWARE\Policies\Microsoft\OneDrive' -Name 'KFMBlockOptIn' -Type DWord -Value 1 -Category $cat
    Set-TweakRegistry -Path 'HKLM:\SOFTWARE\Policies\Microsoft\OneDrive' -Name 'PreventNetworkTrafficPreUserSignIn' -Type DWord -Value 1 -Category $cat
    Remove-OdValue $RunCU 'OneDriveSetup'
    if (-not $KeepNewUserSetup) {
        Invoke-WithDefaultHive {
            $i = Get-RegValueInfo -Path $DefRun -Name 'OneDriveSetup'
            if (-not $i.Existed) { Write-TweakLog 'New-user template has no OneDriveSetup entry' 'SKIP'; return }
            Save-TweakOriginal -Id 'ODDEFRUN|OneDriveSetup' -Data @{ Category = $cat; Kind = 'DefaultUserRun'; Name = 'OneDriveSetup'; ValueKind = $i.Kind; Value = $i.Value }
            Remove-ItemProperty -LiteralPath $DefRun -Name 'OneDriveSetup' -Force
            Write-TweakLog "Default user Run\OneDriveSetup : $($i.Value) -> <absent>" 'SET'
        }
    }
    if (-not $KeepNavPane) {
        foreach ($k in $NavKeys) {
            if (-not (Test-Path -LiteralPath $k)) { continue }
            $i = Get-RegValueInfo -Path $k -Name 'System.IsPinnedToNameSpaceTree'
            if ($i.Existed -and $i.Value -eq 0) { Write-TweakLog "$k already hidden" 'SKIP'; continue }
            if ($PSCmdlet.ShouldProcess("$k\System.IsPinnedToNameSpaceTree", "$(Format-RegInfo $i) -> 0")) {
                Save-OdReg $k 'System.IsPinnedToNameSpaceTree'
                New-ItemProperty -LiteralPath $k -Name 'System.IsPinnedToNameSpaceTree' -PropertyType DWord -Value 0 -Force | Out-Null
                Write-TweakLog "$k\System.IsPinnedToNameSpaceTree : $(Format-RegInfo $i) -> 0" 'SET'
            }
        }
    }
    Save-TweakOriginal -Id 'MODE|OneDrive' -Data @{ Category = $cat; Kind = 'Mode'; NavPane = (-not $KeepNavPane); NewUsers = (-not $KeepNewUserSetup) }
    foreach ($r in $roots) { Write-TweakLog "Left in place (not deleted): $r" 'INFO' }
    Write-TweakLog 'Done. Restart the PC to finish.' 'OK'
}

function Invoke-OdRevert {
    $state = Get-TweakState
    foreach ($id in @($state.Keys | Where-Object { $_ -like 'REG|*' })) {
        if ($state[$id].Category -eq $cat) { Restore-TweakRegistryEntry -Entry $state[$id] -Id $id }
    }
    foreach ($id in @($state.Keys | Where-Object { $_ -like 'ODREG|*' })) {
        $e = $state[$id]
        if ($e.Path -like '*User Shell Folders*') { Remove-TweakStateEntry -Id $id; continue }
        if ($e.Path -like '*\Classes\*' -and -not (Test-Path -LiteralPath $e.Path)) { Write-TweakLog "$($e.Path) gone (OneDrive uninstalled); nothing to restore" 'SKIP'; Remove-TweakStateEntry -Id $id; continue }
        if ($e.Path -eq $RunCU -and $e.Existed) {
            $s = [Environment]::ExpandEnvironmentVariables([string]$e.Value).Trim()
            $exe = if ($s.StartsWith('"')) { $s.Split('"')[1] } else { ($s -split '\s+/')[0] }
            if (-not (Test-Path -LiteralPath $exe)) { Write-TweakLog "$RunCU\$($e.Name) not restored: $exe is not installed (reinstall OneDrive first, then run -Revert again)" 'WARN'; continue }
        }
        Restore-TweakRegistryEntry -Entry $e -Id $id
    }
    foreach ($id in @($state.Keys | Where-Object { $_ -like 'TASK|*' })) {
        $e = $state[$id]
        if ($e.Category -ne $cat) { continue }
        if (-not (Get-ScheduledTask -TaskPath $e.TaskPath -TaskName $e.TaskName -ErrorAction SilentlyContinue)) { Write-TweakLog "Task $($e.TaskPath)$($e.TaskName) gone; forgotten" 'SKIP'; Remove-TweakStateEntry -Id $id; continue }
        if ($PSCmdlet.ShouldProcess("Task $($e.TaskPath)$($e.TaskName)", 'Enable')) {
            if (Enable-TweakTaskAny -TaskPath $e.TaskPath -TaskName $e.TaskName) { Write-TweakLog "Task $($e.TaskPath)$($e.TaskName) : Disabled -> $($e.State)" 'SET'; Remove-TweakStateEntry -Id $id }
        }
    }
    if ($state.ContainsKey('ODDEFRUN|OneDriveSetup')) {
        $e = $state['ODDEFRUN|OneDriveSetup']
        if ($PSCmdlet.ShouldProcess('Default user Run\OneDriveSetup', "Restore $($e.Value)")) {
            Invoke-WithDefaultHive {
                if (-not (Test-Path -LiteralPath $DefRun)) { New-Item -Path $DefRun -Force | Out-Null }
                $kind = if ($e.ValueKind) { $e.ValueKind } else { 'String' }
                New-ItemProperty -LiteralPath $DefRun -Name 'OneDriveSetup' -PropertyType $kind -Value $e.Value -Force | Out-Null
                Write-TweakLog "Default user Run\OneDriveSetup : <absent> -> $($e.Value)" 'SET'
                Remove-TweakStateEntry -Id 'ODDEFRUN|OneDriveSetup'
            }
        }
    }
    foreach ($id in @($state.Keys | Where-Object { $_ -like 'ODKF|*' })) { Write-TweakLog "$($id.Substring(5)) stays at $($state[$id].Path) (not moved back into OneDrive)" 'INFO' }
    Remove-TweakStateEntry -Id 'MODE|OneDrive'
    Write-TweakLog 'To use OneDrive again: winget install Microsoft.OneDrive, or https://www.microsoft.com/onedrive/download, or run %SystemRoot%\SysWOW64\OneDriveSetup.exe; then sign in.' 'INFO'
}

if ($Revert) {
    if (Get-SessionUserMismatch) { Write-TweakLog 'Elevated account is not the signed-in user. Refusing.' 'ERROR'; exit 2 }
    Invoke-OdRevert; Write-TweakSummary -Name 'Remove-OneDrive'; return
}
$scan = Invoke-OdScan
if ($Download) {
    if ($scan.Mismatch) { Write-TweakLog 'Refusing: wrong account.' 'ERROR'; exit 2 }
    foreach ($r in $scan.Roots) {
        if ($PSCmdlet.ShouldProcess($r, 'attrib +P -U /S /D (Always keep on this device)')) {
            & attrib.exe +P -U "$r" | Out-Null
            & attrib.exe +P -U /S /D "$r\*" | Out-Null
            Write-TweakLog "$r : pinned (OneDrive now downloads everything; keep OneDrive running, then re-run this script to check)" 'SET'
        }
    }
}
if ($Remove) {
    if ($scan.Mismatch) { Write-TweakLog 'Refusing: the elevated account is not the signed-in user (it would act on the wrong OneDrive).' 'ERROR'; exit 2 }
    if (-not $scan.Safe -and -not $IKnowMyFilesAreSafe) {
        Write-TweakLog 'Refusing: the files check did not pass (see FAIL lines). Fix them, or re-run with -IKnowMyFilesAreSafe.' 'ERROR'; exit 3
    }
    if (-not $scan.Safe) { Write-TweakLog 'Files check overridden with -IKnowMyFilesAreSafe' 'WARN' }
    Invoke-OdRemove
}
if (-not ($Download -or $Remove)) { Write-TweakLog $(if ($scan.Safe) { 'Check passed: safe to run with -Remove.' } else { 'Check not passed yet: nothing was changed.' }) $(if ($scan.Safe) { 'PASS' } else { 'WARN' }) }
Write-TweakSummary -Name 'Remove-OneDrive'
