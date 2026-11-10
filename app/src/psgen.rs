//! PowerShell fallback: (1) a generated, self-contained script for the selected tweaks (clipboard),
//! (2) the full PowerShell kit embedded in the exe, exportable next to it.
use crate::catalog::*;
use crate::ops::Opts;

pub const KIT: &[(&str, &str)] = &[
    ("README.md", include_str!("../../README.md")),
    ("Apply-All-Tweaks.ps1", include_str!("../../Apply-All-Tweaks.ps1")),
    ("Apply-All-Tweaks.cmd", include_str!("../../Apply-All-Tweaks.cmd")),
    ("Verify-Tweaks.ps1", include_str!("../../Verify-Tweaks.ps1")),
    ("Revert-Tweaks.ps1", include_str!("../../Revert-Tweaks.ps1")),
    ("scripts/TweakCommon.ps1", include_str!("../../scripts/TweakCommon.ps1")),
    ("scripts/Disable-AutomaticUpdates.ps1", include_str!("../../scripts/Disable-AutomaticUpdates.ps1")),
    ("scripts/Disable-UpdateNotifications.ps1", include_str!("../../scripts/Disable-UpdateNotifications.ps1")),
    ("scripts/Block-Windows11Upgrade.ps1", include_str!("../../scripts/Block-Windows11Upgrade.ps1")),
    ("scripts/Remove-Copilot.ps1", include_str!("../../scripts/Remove-Copilot.ps1")),
    ("scripts/Disable-LockScreenSpotlight.ps1", include_str!("../../scripts/Disable-LockScreenSpotlight.ps1")),
    ("scripts/New-GodModeFolder.ps1", include_str!("../../scripts/New-GodModeFolder.ps1")),
    ("scripts/Clear-SoftwareDistribution.ps1", include_str!("../../scripts/Clear-SoftwareDistribution.ps1")),
    ("scripts/Set-UpdateWindow.ps1", include_str!("../../scripts/Set-UpdateWindow.ps1")),
    ("scripts/Remove-OneDrive.ps1", include_str!("../../scripts/Remove-OneDrive.ps1")),
];

/// Writes the embedded kit to <dir>. Returns the folder written.
pub fn export_kit(dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let root = dir.join("Win10Tweaks-PowerShell");
    for (rel, text) in KIT {
        let p = root.join(rel.replace('/', "\\"));
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(root)
}

fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

pub fn generate(cats: &[Cat], o: Opts) -> String {
    let mut s = String::new();
    s.push_str("# PrimeCrew Tweakinator 5000 - generated PowerShell (Windows PowerShell 5.1). Paste into an ELEVATED PowerShell window.\n");
    s.push_str(&format!("# {SECURITY_NOTE}\n"));
    s.push_str("# This pasted version does not record originals; use the app or Revert-Tweaks.ps1 (exported kit) to undo.\n");
    s.push_str("& {\n");
    s.push_str("if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { Write-Warning 'Run this in an elevated (Administrator) PowerShell.'; return }\n");
    s.push_str(r#"function Set-Reg([string]$p,[string]$n,[string]$t,$v){ $o=(Get-ItemProperty -LiteralPath $p -Name $n -ErrorAction SilentlyContinue).$n; if("$o" -eq "$v"){ Write-Host "[SKIP] $p\$n already $v"; return }; if(-not (Test-Path -LiteralPath $p)){ New-Item -Path $p -Force | Out-Null }; New-ItemProperty -LiteralPath $p -Name $n -PropertyType $t -Value $v -Force | Out-Null; if($null -eq $o){$o='<absent>'}; Write-Host "[SET] $p\$n : $o -> $v ($t)" }
function Disable-Tasks([string]$f,[string]$only=''){ Get-ScheduledTask -TaskPath $f -ErrorAction SilentlyContinue | Where-Object { (-not $only -or $_.TaskName -eq $only) -and $_.State -ne 'Disabled' } | ForEach-Object { $t=$_; try { $t | Disable-ScheduledTask -ErrorAction Stop | Out-Null; Write-Host "[SET] Task $($t.TaskPath)$($t.TaskName) : $($t.State) -> Disabled" } catch { Write-Warning "Task $($t.TaskPath)$($t.TaskName): protected, left as-is" } } }
function Disable-Svc([string]$n){ $k="HKLM:\SYSTEM\CurrentControlSet\Services\$n"; if(-not (Test-Path $k)){return}; $o=(Get-ItemProperty $k).Start; try { Set-Service -Name $n -StartupType Disabled -ErrorAction Stop } catch { try { Set-ItemProperty $k -Name Start -Value 4 -Type DWord -ErrorAction Stop } catch { Write-Warning "Service ${n}: protected, not changed"; return } }; Write-Host "[SET] Service $n Start: $o -> 4"; Stop-Service -Name $n -Force -ErrorAction SilentlyContinue }
"#);
    if o.restore_point {
        s.push_str("try { Checkpoint-Computer -Description 'Win10Tweaks - before Apply (pasted)' -RestorePointType MODIFY_SETTINGS -ErrorAction Stop; Write-Host '[SET] Restore point created' } catch { Write-Warning \"Restore point failed: $($_.Exception.Message)\"; if ((Read-Host 'Continue without restore point? [y/N]') -notmatch '^y') { return } }\n");
    }
    for c in Cat::ALL {
        if !cats.contains(&c) {
            continue;
        }
        s.push_str(&format!("\n# ---- {} ----\n", c.title()));
        if c == Cat::Cleanup {
            s.push_str(r#"$sd = Join-Path $env:WINDIR 'SoftwareDistribution'
$busy = (Test-Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired') -or (Test-Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending') -or ((Get-Service TrustedInstaller).Status -eq 'Running')
if ($busy) { Write-Warning 'Reboot pending / servicing in progress - cleanup skipped.' } else {
  $run = @(Get-Service wuauserv,bits | Where-Object Status -eq 'Running' | ForEach-Object Name)
  Stop-Service wuauserv,bits -Force
  foreach ($d in 'Download','PostRebootEventCache.V2') { $p = Join-Path $sd $d; if (Test-Path $p) { Get-ChildItem -LiteralPath $p -Force | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue; Write-Host "[SET] $p\* deleted" } }
  $f = Join-Path $sd 'ReportingEvents.log'; if (Test-Path $f) { Remove-Item -LiteralPath $f -Force; Write-Host "[SET] $f deleted" }
  foreach ($n in $run) { Start-Service $n }
}
"#);
            continue;
        }
        for it in REG.iter().filter(|i| i.cat == c && !(o.policy_only && i.level == Level::Hard)) {
            let v = match it.val {
                Val::D(d) => d.to_string(),
                Val::S(t) => q(t),
            };
            s.push_str(&format!("Set-Reg {} {} {} {}\n", q(it.path), q(it.name), it.val.kind(), v));
        }
        match c {
            Cat::Updates if !o.policy_only => {
                for f in UPDATE_TASK_FOLDERS {
                    s.push_str(&format!("Disable-Tasks {}\n", q(f)));
                }
                for (n, _, _) in SERVICES {
                    s.push_str(&format!("Disable-Svc {}\n", q(n)));
                }
                for n in FW_SERVICES {
                    let r = format!("{FW_PREFIX}{n}");
                    s.push_str(&format!("if ((Get-Service -Name {sv} -ErrorAction SilentlyContinue) -and -not (Get-NetFirewallRule -Name {r} -ErrorAction SilentlyContinue)) {{ New-NetFirewallRule -Name {r} -DisplayName {r} -Group 'Win10Tweaks' -Direction Outbound -Action Block -Profile Any -Service {sv} | Out-Null; Write-Host \"[SET] Firewall {r} created\" }}\n", r = q(&r), sv = q(n)));
                }
                s.push_str("# NOTE: this pasted version does NOT take over the WaaSMedicSvc registry key or run tasks as SYSTEM; use the app or the exported kit (Disable-AutomaticUpdates.ps1) for the forced kill + revertable ACL.\n");
            }
            Cat::Notifications => s.push_str("Disable-Tasks '\\Microsoft\\Windows\\UpdateOrchestrator\\' 'USO_UxBroker'\n"),
            Cat::Copilot => {
                s.push_str("Get-Process -Name Copilot,Microsoft.Copilot,M365Copilot -ErrorAction SilentlyContinue | Stop-Process -Force\n");
                let mut pk = vec![PKG_COPILOT];
                if !o.keep_m365 {
                    pk.push(PKG_M365);
                }
                for (name, fam) in pk {
                    s.push_str(&format!("Get-AppxPackage -AllUsers -Name {n} | ForEach-Object {{ Remove-AppxPackage -Package $_.PackageFullName -AllUsers; Write-Host \"[SET] Appx $($_.PackageFullName) removed\" }}\n", n = q(name)));
                    s.push_str(&format!("Get-AppxProvisionedPackage -Online | Where-Object DisplayName -eq {n} | ForEach-Object {{ Remove-AppxProvisionedPackage -Online -PackageName $_.PackageName | Out-Null; Write-Host \"[SET] Provisioned $($_.PackageName) removed\" }}\n", n = q(name)));
                    s.push_str(&format!("New-Item -Path {} -Force | Out-Null\n", q(&format!(r"{DEPROV_ROOT}\{fam}"))));
                }
            }
            Cat::GodMode => s.push_str(&format!(
                "$d=[Environment]::GetFolderPath('Desktop'); if(-not $d){{$d=Join-Path $env:USERPROFILE 'Desktop'}}; $g=Join-Path $d {}; if(-not (Test-Path -LiteralPath $g)){{ New-Item -ItemType Directory -Path $g | Out-Null; Write-Host \"[SET] $g created\" }}\n",
                q(GODMODE_NAME)
            )),
            _ => {}
        }
    }
    s.push_str("\nWrite-Host 'Done. Reboot to finish.'\n}\n");
    s
}
