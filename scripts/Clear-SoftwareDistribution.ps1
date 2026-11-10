#requires -Version 5.1
<#
.SYNOPSIS
  Safe, scoped Windows Update cache cleanup.
.DESCRIPTION
  Deletes ONLY:
    %WINDIR%\SoftwareDistribution\Download\*            (downloaded update payloads)
    %WINDIR%\SoftwareDistribution\PostRebootEventCache.V2\*
    %WINDIR%\SoftwareDistribution\ReportingEvents.log
  Never touches DataStore (update history DB) or the rest of the tree.
  Sequence: check for pending reboot -> stop wuauserv + bits -> delete -> restart only services that were running.

  DO NOT RUN when:
    * an update is installing or a restart is pending (script refuses unless -Force)
    * TrustedInstaller (Windows Modules Installer) is running (refuses unless -Force)
  Deleting Download means any half-downloaded update is fetched again next time updates are enabled.
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param([switch]$Force)

. (Join-Path $PSScriptRoot 'TweakCommon.ps1')
if (Invoke-SelfElevate -ScriptPath $PSCommandPath -BoundParameters $PSBoundParameters) { return }
Initialize-TweakLog -Name 'Clear-SoftwareDistribution'

$sd = Join-Path $env:WINDIR 'SoftwareDistribution'
$pending = @()
if (Test-Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired') { $pending += 'WindowsUpdate RebootRequired' }
if (Test-Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending') { $pending += 'CBS RebootPending' }
$ti = Get-Service -Name TrustedInstaller -ErrorAction SilentlyContinue
if ($ti -and $ti.Status -eq 'Running') { $pending += 'TrustedInstaller running (servicing in progress)' }
if ($pending.Count -gt 0) {
    if (-not $Force) {
        Write-TweakLog ("Refusing to clean: " + ($pending -join '; ') + '. Reboot first, or re-run with -Force.') 'WARN'
        Write-TweakSummary -Name 'Clear-SoftwareDistribution'; return
    }
    Write-TweakLog ("-Force given despite: " + ($pending -join '; ')) 'WARN'
}

$services = @('wuauserv', 'bits')
$wasRunning = @{}
foreach ($s in $services) {
    $svc = Get-Service -Name $s -ErrorAction SilentlyContinue
    if (-not $svc) { continue }
    $wasRunning[$s] = ($svc.Status -eq 'Running')
    if ($svc.Status -ne 'Stopped') {
        if ($PSCmdlet.ShouldProcess("Service $s", 'Stop')) {
            try { Stop-Service -Name $s -Force -ErrorAction Stop; Write-TweakLog "Service $s : $($svc.Status) -> Stopped" 'SET' }
            catch { Write-TweakLog "Service $s : could not stop ($($_.Exception.Message)). Aborting cleanup." 'ERROR'; Write-TweakSummary -Name 'Clear-SoftwareDistribution'; return }
        } else { Write-TweakLog "Service $s : $($svc.Status) -> Stopped" 'WHATIF' }
    }
}

$targets = @(
    @{ Path = (Join-Path $sd 'Download');                Mode = 'Contents' },
    @{ Path = (Join-Path $sd 'PostRebootEventCache.V2'); Mode = 'Contents' },
    @{ Path = (Join-Path $sd 'ReportingEvents.log');     Mode = 'File' }
)
foreach ($t in $targets) {
    if (-not (Test-Path -LiteralPath $t.Path)) { Write-TweakLog "$($t.Path) not present" 'SKIP'; continue }
    if ($t.Mode -eq 'File') {
        $size = (Get-Item -LiteralPath $t.Path).Length
        if ($PSCmdlet.ShouldProcess($t.Path, 'Delete file')) {
            try { Remove-Item -LiteralPath $t.Path -Force -ErrorAction Stop; Write-TweakLog "$($t.Path) : $size bytes -> deleted" 'SET' }
            catch { Write-TweakLog "$($t.Path) : $($_.Exception.Message)" 'ERROR' }
        } else { Write-TweakLog "$($t.Path) : $size bytes -> deleted" 'WHATIF' }
    } else {
        $items = @(Get-ChildItem -LiteralPath $t.Path -Force -ErrorAction SilentlyContinue)
        $bytes = (Get-ChildItem -LiteralPath $t.Path -Recurse -Force -File -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum
        if ($null -eq $bytes) { $bytes = 0 }
        if ($items.Count -eq 0) { Write-TweakLog "$($t.Path) already empty" 'SKIP'; continue }
        if ($PSCmdlet.ShouldProcess($t.Path, "Delete contents ($($items.Count) items, $([math]::Round($bytes/1MB,1)) MB)")) {
            foreach ($i in $items) {
                try { Remove-Item -LiteralPath $i.FullName -Recurse -Force -ErrorAction Stop }
                catch { Write-TweakLog "$($i.FullName) : $($_.Exception.Message)" 'WARN' }
            }
            Write-TweakLog "$($t.Path)\* : $($items.Count) items / $([math]::Round($bytes/1MB,1)) MB -> deleted" 'SET'
        } else { Write-TweakLog "$($t.Path)\* : $($items.Count) items / $([math]::Round($bytes/1MB,1)) MB -> deleted" 'WHATIF' }
    }
}

foreach ($s in $services) {
    if ($wasRunning.ContainsKey($s) -and $wasRunning[$s]) {
        if ($PSCmdlet.ShouldProcess("Service $s", 'Start')) {
            try { Start-Service -Name $s -ErrorAction Stop; Write-TweakLog "Service $s : Stopped -> Running" 'SET' }
            catch { Write-TweakLog "Service $s : could not restart ($($_.Exception.Message))" 'WARN' }
        } else { Write-TweakLog "Service $s : Stopped -> Running" 'WHATIF' }
    } else { Write-TweakLog "Service $s was not running before; left stopped" 'SKIP' }
}
Write-TweakSummary -Name 'Clear-SoftwareDistribution'
