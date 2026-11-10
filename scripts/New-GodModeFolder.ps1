#requires -Version 5.1
<#
.SYNOPSIS
  Creates the "God Mode" (All Tasks) shell folder on the Desktop.
.DESCRIPTION
  Folder name: God Mode.{ED7BA470-8E54-465E-825C-99712043E01C}
  Location: [Environment]::GetFolderPath('Desktop') of the account running the script
  (falls back to %USERPROFILE%\Desktop). Works with OneDrive-redirected desktops.
  No admin needed, but runs fine elevated (same account => same Desktop).
.PARAMETER Path
  Optional explicit parent folder.
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param([string]$Path)

. (Join-Path $PSScriptRoot 'TweakCommon.ps1')
Initialize-TweakLog -Name 'New-GodModeFolder'

if (-not $Path) { $Path = [Environment]::GetFolderPath('Desktop') }
if (-not $Path -or -not (Test-Path -LiteralPath $Path)) { $Path = Join-Path $env:USERPROFILE 'Desktop' }
$folder = Join-Path $Path 'God Mode.{ED7BA470-8E54-465E-825C-99712043E01C}'

if (Test-Path -LiteralPath $folder) {
    Write-TweakLog "$folder already exists" 'SKIP'
} elseif ($PSCmdlet.ShouldProcess($folder, 'Create God Mode folder')) {
    try {
        New-Item -ItemType Directory -Path $folder -Force -ErrorAction Stop | Out-Null
        Save-TweakOriginal -Id "GODMODE|$folder" -Data @{ Category = 'GodMode'; Kind = 'Folder'; Path = $folder; Existed = $false }
        Write-TweakLog "$folder : <absent> -> created" 'SET'
    } catch { Write-TweakLog "$folder : create failed ($($_.Exception.Message))" 'ERROR' }
} else { Write-TweakLog "$folder : <absent> -> created" 'WHATIF' }

Write-TweakSummary -Name 'New-GodModeFolder'
