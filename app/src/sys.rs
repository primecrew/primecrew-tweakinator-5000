//! Thin Windows system layer: registry (winreg), child processes (sc.exe / taskkill / powershell.exe,
//! always local, never network), elevation, known folders.
use std::os::windows::process::CommandExt;
use std::process::Command;
use winreg::enums::*;
use winreg::{RegKey, RegValue};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Debug, PartialEq)]
pub enum Cur {
    Absent,
    D(u32),
    S(String),
    Other(String), // value exists with an unexpected type; kind name
}

impl Cur {
    pub fn show(&self) -> String {
        match self {
            Cur::Absent => "<absent>".into(),
            Cur::D(v) => format!("{v} (DWord)"),
            Cur::S(s) => format!("\"{s}\" (String)"),
            Cur::Other(k) => format!("<{k}>"),
        }
    }
    pub fn as_text(&self) -> Option<String> {
        match self {
            Cur::D(v) => Some(v.to_string()),
            Cur::S(s) => Some(s.clone()),
            _ => None,
        }
    }
}

/// "HKLM:\SOFTWARE\..." -> (hive, "SOFTWARE\...")
fn split(path: &str) -> (RegKey, String) {
    let (h, rest) = path.split_once(":\\").unwrap_or(("HKLM", path));
    let hive = match h.to_ascii_uppercase().as_str() {
        "HKCU" => RegKey::predef(HKEY_CURRENT_USER),
        "HKU" => RegKey::predef(HKEY_USERS),
        _ => RegKey::predef(HKEY_LOCAL_MACHINE),
    };
    (hive, rest.to_string())
}

pub fn key_exists(path: &str) -> bool {
    let (hive, sub) = split(path);
    hive.open_subkey_with_flags(&sub, KEY_READ).is_ok()
}

pub fn reg_get(path: &str, name: &str) -> Cur {
    let (hive, sub) = split(path);
    let Ok(k) = hive.open_subkey_with_flags(&sub, KEY_READ) else { return Cur::Absent };
    match k.get_raw_value(name) {
        Err(_) => Cur::Absent,
        Ok(RegValue { vtype, bytes }) => match vtype {
            REG_DWORD if bytes.len() >= 4 => Cur::D(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
            REG_SZ | REG_EXPAND_SZ => {
                let w: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                let s = String::from_utf16_lossy(&w);
                Cur::S(s.trim_end_matches('\0').to_string())
            }
            other => Cur::Other(format!("{other:?}")),
        },
    }
}

pub fn reg_set_dword(path: &str, name: &str, v: u32) -> Result<(), String> {
    let (hive, sub) = split(path);
    let (k, _) = hive.create_subkey(&sub).map_err(|e| e.to_string())?;
    k.set_value(name, &v).map_err(|e| e.to_string())
}

pub fn reg_set_string(path: &str, name: &str, v: &str) -> Result<(), String> {
    let (hive, sub) = split(path);
    let (k, _) = hive.create_subkey(&sub).map_err(|e| e.to_string())?;
    k.set_value(name, &v.to_string()).map_err(|e| e.to_string())
}

pub fn reg_set_expand(path: &str, name: &str, v: &str) -> Result<(), String> {
    let (hive, sub) = split(path);
    let (k, _) = hive.create_subkey(&sub).map_err(|e| e.to_string())?;
    let mut bytes: Vec<u8> = v.encode_utf16().chain(std::iter::once(0)).flat_map(|c| c.to_le_bytes()).collect();
    bytes.shrink_to_fit();
    k.set_raw_value(name, &RegValue { vtype: REG_EXPAND_SZ, bytes }).map_err(|e| e.to_string())
}

/// Raw kind name of a value ("String", "ExpandString", "DWord", ...) as PowerShell's GetValueKind reports it.
pub fn reg_kind(path: &str, name: &str) -> Option<&'static str> {
    let (hive, sub) = split(path);
    let k = hive.open_subkey_with_flags(&sub, KEY_READ).ok()?;
    let v = k.get_raw_value(name).ok()?;
    Some(match v.vtype {
        REG_SZ => "String",
        REG_EXPAND_SZ => "ExpandString",
        REG_DWORD => "DWord",
        REG_QWORD => "QWord",
        REG_MULTI_SZ => "MultiString",
        _ => "Binary",
    })
}

pub fn reg_subkeys(path: &str) -> Vec<String> {
    let (hive, sub) = split(path);
    let Ok(k) = hive.open_subkey_with_flags(&sub, KEY_READ) else { return vec![] };
    k.enum_keys().flatten().collect()
}

pub fn reg_value_names(path: &str) -> Vec<String> {
    let (hive, sub) = split(path);
    let Ok(k) = hive.open_subkey_with_flags(&sub, KEY_READ) else { return vec![] };
    k.enum_values().flatten().map(|(n, _)| n).collect()
}

pub fn reg_delete_value(path: &str, name: &str) -> Result<(), String> {
    let (hive, sub) = split(path);
    let k = hive.open_subkey_with_flags(&sub, KEY_SET_VALUE).map_err(|e| e.to_string())?;
    k.delete_value(name).map_err(|e| e.to_string())
}

pub fn reg_create_key(path: &str) -> Result<(), String> {
    let (hive, sub) = split(path);
    hive.create_subkey(&sub).map(|_| ()).map_err(|e| e.to_string())
}

pub fn reg_delete_key(path: &str) -> Result<(), String> {
    let (hive, sub) = split(path);
    hive.delete_subkey_all(&sub).map_err(|e| e.to_string())
}

/// Run a local executable hidden; returns (success, stdout+stderr).
pub fn run(exe: &str, args: &[&str]) -> (bool, String) {
    match Command::new(exe).args(args).creation_flags(CREATE_NO_WINDOW).output() {
        Ok(o) => {
            let mut s = String::from_utf8_lossy(&o.stdout).to_string();
            let e = String::from_utf8_lossy(&o.stderr);
            if !e.trim().is_empty() {
                s.push_str(&e);
            }
            (o.status.success(), s.trim().to_string())
        }
        Err(e) => (false, e.to_string()),
    }
}

/// Run a PowerShell snippet (local only). Used where Windows has no simple Win32 API:
/// Appx removal, scheduled-task enumeration/changes, restore point, service stop/start.
pub fn ps(script: &str) -> (bool, String) {
    let full = format!("$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'; {script}");
    run(
        "powershell.exe",
        &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &full],
    )
}

pub fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

pub fn is_admin() -> bool {
    unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 }
}

/// Re-launch this exe with the "runas" verb (UAC). Returns true if the elevated copy started.
pub fn relaunch_elevated() -> bool {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let Ok(exe) = std::env::current_exe() else { return false };
    let w = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let verb = w("runas");
    let file = w(&exe.to_string_lossy());
    let r = unsafe {
        ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL)
    };
    (r as isize) > 32
}

/// The Desktop of the account this process runs as: SHGetKnownFolderPath(FOLDERID_Desktop) (honours OneDrive /
/// redirected Desktops), else %USERPROFILE%\Desktop. No hard-coded user names.
pub fn desktop_dir() -> std::path::PathBuf {
    dirs::desktop_dir().filter(|p| p.exists()).unwrap_or_else(|| {
        let home = std::env::var_os("USERPROFILE").map(std::path::PathBuf::from).or_else(dirs::home_dir).unwrap_or_else(|| std::path::PathBuf::from(r"C:\Users\Public"));
        home.join("Desktop")
    })
}

/// %VAR% expansion from this process's environment (enough for User Shell Folders / Run values).
pub fn expand_env(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('%') {
            Some(j) if j > 0 => {
                let var = &after[..j];
                match std::env::var(var) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => {
                        out.push('%');
                        out.push_str(var);
                        out.push('%');
                    }
                }
                rest = &after[j + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn wide_to_string(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe {
        let mut n = 0;
        while *p.add(n) != 0 {
            n += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
    }
}

/// "DOMAIN\user" of the account this process (the elevated one) runs as.
pub fn process_user() -> String {
    use windows_sys::Win32::System::WindowsProgramming::GetUserNameW;
    let mut buf = [0u16; 260];
    let mut n = buf.len() as u32;
    let user = if unsafe { GetUserNameW(buf.as_mut_ptr(), &mut n) } != 0 { wide_to_string(buf.as_ptr()) } else { std::env::var("USERNAME").unwrap_or_default() };
    let dom = std::env::var("USERDOMAIN").unwrap_or_default();
    if dom.is_empty() { user } else { format!("{dom}\\{user}") }
}

/// "DOMAIN\user" signed in to this Windows session (WTSQuerySessionInformation). With an over-the-shoulder UAC
/// elevation (a different admin account typed in), this differs from process_user(): HKCU, %LOCALAPPDATA% and the
/// Desktop then belong to the admin account, not to the person sitting at the PC.
pub fn session_user() -> Option<String> {
    use windows_sys::Win32::System::RemoteDesktop::{WTSFreeMemory, WTSQuerySessionInformationW, WTSUserName, WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION};
    const WTS_DOMAIN_NAME: i32 = 7;
    let q = |class: i32| -> Option<String> {
        let mut p: *mut u16 = std::ptr::null_mut();
        let mut n = 0u32;
        let ok = unsafe { WTSQuerySessionInformationW(WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, class, &mut p, &mut n) };
        if ok == 0 || p.is_null() {
            return None;
        }
        let s = wide_to_string(p);
        unsafe { WTSFreeMemory(p as _) };
        Some(s)
    };
    let user = q(WTSUserName)?;
    if user.is_empty() {
        return None;
    }
    let dom = q(WTS_DOMAIN_NAME).unwrap_or_default();
    Some(if dom.is_empty() { user } else { format!("{dom}\\{user}") })
}

/// Some((elevated, signed_in)) when the elevated account is not the signed-in user.
pub fn user_mismatch() -> Option<(String, String)> {
    let me = process_user();
    let s = session_user()?;
    let parts = |x: &str| -> (String, String) {
        match x.rsplit_once('\\') {
            Some((d, u)) => (d.to_ascii_lowercase(), u.to_ascii_lowercase()),
            None => (String::new(), x.to_ascii_lowercase()),
        }
    };
    let ((d1, u1), (d2, u2)) = (parts(&me), parts(&s));
    let same = u1 == u2 && (d1.is_empty() || d2.is_empty() || d1 == d2);
    (!same).then_some((me, s))
}

/// Current path of a Windows known folder for this process's user (SHGetKnownFolderPath).
pub fn known_folder(id: &windows_sys::core::GUID) -> Option<String> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::SHGetKnownFolderPath;
    let mut p: windows_sys::core::PWSTR = std::ptr::null_mut();
    let hr = unsafe { SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut p) };
    let s = (hr >= 0).then(|| wide_to_string(p));
    unsafe { CoTaskMemFree(p as _) };
    s
}

/// Points a known folder at `path` (SHSetKnownFolderPath; updates User Shell Folders and notifies Explorer).
/// Moves nothing: files are copied separately by the caller.
pub fn set_known_folder(id: &windows_sys::core::GUID, path: &str) -> Result<(), String> {
    use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    use windows_sys::Win32::UI::Shell::SHSetKnownFolderPath;
    let w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let init = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        let hr = SHSetKnownFolderPath(id, 0, std::ptr::null_mut(), w.as_ptr());
        if init >= 0 {
            CoUninitialize();
        }
        if hr >= 0 { Ok(()) } else { Err(format!("SHSetKnownFolderPath HRESULT 0x{:08X}", hr as u32)) }
    }
}

/// Start a program (or open a folder) as the signed-in user's shell would, i.e. NOT elevated: explorer.exe hands
/// it to the running desktop shell. Needed for OneDrive.exe, which refuses to run with full admin rights.
pub fn shell_open_unelevated(target: &str) -> bool {
    Command::new("explorer.exe").arg(target).spawn().is_ok()
}

pub fn os_summary() -> String {
    let p = r"HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let g = |n: &str| reg_get(p, n).as_text().unwrap_or_default();
    format!(
        "{} | {} | {} | build {}.{} | {} | user {}",
        g("ProductName"),
        g("EditionID"),
        g("DisplayVersion"),
        g("CurrentBuild"),
        g("UBR"),
        std::env::var("COMPUTERNAME").unwrap_or_default(),
        process_user()
    )
}

/// CurrentBuild from the Windows NT key (19045 = 22H2, 22000+ = Windows 11).
pub fn os_build() -> u32 {
    let p = r"HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    reg_get(p, "CurrentBuild").as_text().and_then(|s| s.parse().ok()).unwrap_or(0)
}

pub fn os_product_name() -> String {
    let p = r"HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    reg_get(p, "ProductName").as_text().unwrap_or_default()
}

pub fn is_windows_11() -> bool {
    os_build() >= crate::catalog::WIN11_BUILD || os_product_name().to_ascii_lowercase().contains("windows 11")
}

/// Reasons a reboot appears to be pending (empty = none). Read-only.
pub fn reboot_pending_reasons() -> Vec<String> {
    let mut v = vec![];
    if key_exists(r"HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired") {
        v.push("Windows Update reports a reboot is required".into());
    }
    if key_exists(r"HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending") {
        v.push("Component servicing reports a reboot is pending".into());
    }
    if key_exists(r"HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\PostRebootReporting") {
        v.push("Windows Update is waiting to report after a reboot".into());
    }
    match reg_get(r"HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager", "PendingFileRenameOperations") {
        Cur::Absent => {}
        Cur::S(s) if s.trim().is_empty() => {}
        Cur::Other(_) | Cur::S(_) | Cur::D(_) => v.push("Windows has file renames waiting for a reboot".into()),
    }
    v
}

/// Whether the signed-in session account is in the local Administrators group.
pub fn session_user_is_admin() -> Option<bool> {
    if user_mismatch().is_none() {
        return Some(is_admin());
    }
    let sess = session_user()?;
    let short = sess.rsplit_once('\\').map(|(_, u)| u).unwrap_or(&sess);
    let (_, out) = run("net.exe", &["localgroup", "Administrators"]);
    if out.trim().is_empty() {
        return None;
    }
    let full = sess.to_ascii_lowercase();
    let short = short.to_ascii_lowercase();
    Some(out.lines().any(|l| {
        let t = l.trim().to_ascii_lowercase();
        !t.is_empty()
            && !t.starts_with("alias")
            && !t.starts_with("comment")
            && !t.starts_with("members")
            && !t.starts_with("the command")
            && !t.starts_with("---")
            && (t == full || t == short || t.ends_with(&format!("\\{short}")))
    }))
}

fn dword_at(path: &str, name: &str) -> Option<u32> {
    match reg_get(path, name) {
        Cur::D(v) => Some(v),
        _ => None,
    }
}

/// Third-party antivirus display names from Security Center (excludes Microsoft Defender).
pub fn third_party_antivirus() -> Vec<String> {
    let (_, out) = ps(
        "Get-CimInstance -Namespace root/SecurityCenter2 -ClassName AntiVirusProduct -ErrorAction SilentlyContinue | ForEach-Object { $_.displayName }",
    );
    out.lines()
        .map(|l| l.trim().to_string())
        .filter(|n| {
            if n.is_empty() {
                return false;
            }
            let l = n.to_ascii_lowercase();
            !l.contains("windows defender") && !l.contains("microsoft defender")
        })
        .collect()
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Preflight {
    pub elevated: bool,
    pub process_user: String,
    pub session_user: Option<String>,
    pub session_is_admin: Option<bool>,
    pub mismatch: Option<(String, String)>,
    pub enable_lua: Option<u32>,
    pub consent_admin: Option<u32>,
    pub tamper_on: Option<bool>,
    pub third_party_av: Vec<String>,
    pub reboot_pending: Vec<String>,
    pub build: u32,
    pub product_name: String,
    pub windows11: bool,
}

impl Preflight {
    pub fn uac_summary() -> &'static str {
        "The UAC level does not change what this app can do once it is running as administrator. This app never changes UAC settings."
    }

    pub fn uac_label(&self) -> String {
        if self.enable_lua == Some(0) {
            return "User Account Control is off".into();
        }
        let consent = match self.consent_admin {
            Some(0) => "Elevate without prompting",
            Some(1) => "Prompt for credentials on the secure desktop",
            Some(2) => "Prompt for consent on the secure desktop",
            Some(3) => "Prompt for credentials",
            Some(4) => "Prompt for consent",
            Some(5) => "Prompt for consent for non-Windows programs (Windows default)",
            Some(n) => return format!("UAC is on (ConsentPromptBehaviorAdmin = {n})"),
            None => return if self.enable_lua.is_none() && self.consent_admin.is_none() {
                "UAC level could not be read".into()
            } else {
                "UAC is on".into()
            },
        };
        format!("UAC is on — {consent}")
    }
}

/// Read-only snapshot of this PC, used by the pre-flight check. Changes nothing.
pub fn preflight() -> Preflight {
    const UAC: &str = r"HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System";
    const TAMPER: &str = r"HKLM:\SOFTWARE\Microsoft\Windows Defender\Features";
    Preflight {
        elevated: is_admin(),
        process_user: process_user(),
        session_user: session_user(),
        session_is_admin: session_user_is_admin(),
        mismatch: user_mismatch(),
        enable_lua: dword_at(UAC, "EnableLUA"),
        consent_admin: dword_at(UAC, "ConsentPromptBehaviorAdmin"),
        tamper_on: dword_at(TAMPER, "TamperProtection").map(|v| v == 5),
        third_party_av: third_party_antivirus(),
        reboot_pending: reboot_pending_reasons(),
        build: os_build(),
        product_name: os_product_name(),
        windows11: is_windows_11(),
    }
}

// ---------------- registry key security (service keys only) ----------------
// Used ONLY for HKLM\SYSTEM\CurrentControlSet\Services\<update service>. Never for files or binaries.
mod sec {
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, ERROR_SUCCESS, HANDLE, LUID};
    use windows_sys::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, LUID_AND_ATTRIBUTES,
        OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Registry::{RegCloseKey, RegGetKeySecurity, RegOpenKeyExW, RegSetKeySecurity, HKEY, HKEY_LOCAL_MACHINE};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    const READ_CONTROL: u32 = 0x0002_0000;
    const WRITE_DAC: u32 = 0x0004_0000;
    const WRITE_OWNER: u32 = 0x0008_0000;
    pub const OGD: u32 = OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    pub const OWNER: u32 = OWNER_SECURITY_INFORMATION;
    pub const DACL: u32 = DACL_SECURITY_INFORMATION;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Enables a privilege the elevated admin token already holds (SeTakeOwnership / SeRestore).
    pub fn enable_privilege(name: &str) -> Result<(), String> {
        unsafe {
            let mut tok: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut tok) == 0 {
                return Err(format!("OpenProcessToken: {}", std::io::Error::last_os_error()));
            }
            let mut luid = LUID { LowPart: 0, HighPart: 0 };
            let n = w(name);
            if LookupPrivilegeValueW(std::ptr::null(), n.as_ptr(), &mut luid) == 0 {
                CloseHandle(tok);
                return Err(format!("LookupPrivilegeValue {name}: {}", std::io::Error::last_os_error()));
            }
            let tp = TOKEN_PRIVILEGES { PrivilegeCount: 1, Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }] };
            let ok = AdjustTokenPrivileges(tok, 0, &tp, 0, std::ptr::null_mut(), std::ptr::null_mut());
            let err = std::io::Error::last_os_error();
            CloseHandle(tok);
            // AdjustTokenPrivileges returns success with ERROR_NOT_ALL_ASSIGNED (1300) if the token lacks it.
            if ok == 0 || err.raw_os_error() == Some(1300) {
                return Err(format!("{name} not available: {err}"));
            }
            Ok(())
        }
    }

    fn open(sub: &str, access: u32) -> Result<HKEY, String> {
        let s = w(sub);
        let mut h: HKEY = std::ptr::null_mut();
        let r = unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, s.as_ptr(), 0, access, &mut h) };
        if r != ERROR_SUCCESS {
            return Err(format!("open HKLM\\{sub}: {}", std::io::Error::from_raw_os_error(r as i32)));
        }
        Ok(h)
    }

    /// Owner + group + DACL of HKLM\<sub> as SDDL.
    pub fn get_sddl(sub: &str) -> Result<String, String> {
        let h = open(sub, READ_CONTROL)?;
        unsafe {
            let mut len = 0u32;
            RegGetKeySecurity(h, OGD, std::ptr::null_mut(), &mut len);
            let mut buf = vec![0u8; len.max(64) as usize];
            let r = RegGetKeySecurity(h, OGD, buf.as_mut_ptr() as PSECURITY_DESCRIPTOR, &mut len);
            RegCloseKey(h);
            if r != ERROR_SUCCESS {
                return Err(format!("RegGetKeySecurity: {}", std::io::Error::from_raw_os_error(r as i32)));
            }
            let mut p: *mut u16 = std::ptr::null_mut();
            if ConvertSecurityDescriptorToStringSecurityDescriptorW(buf.as_mut_ptr() as PSECURITY_DESCRIPTOR, SDDL_REVISION_1, OGD, &mut p, std::ptr::null_mut()) == 0 {
                return Err(format!("SD->SDDL: {}", std::io::Error::last_os_error()));
            }
            let mut n = 0;
            while *p.add(n) != 0 {
                n += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
            LocalFree(p as _);
            Ok(s)
        }
    }

    /// Writes the parts of `sddl` selected by `info` to HKLM\<sub> (handle-based: no propagation to subkeys).
    pub fn set_sddl(sub: &str, sddl: &str, info: u32) -> Result<(), String> {
        let mut access = 0;
        if info & OWNER != 0 {
            access |= WRITE_OWNER;
        }
        if info & (DACL | GROUP_SECURITY_INFORMATION) != 0 {
            access |= WRITE_DAC;
        }
        let h = open(sub, access)?;
        unsafe {
            let s = w(sddl);
            let mut psd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(s.as_ptr(), SDDL_REVISION_1, &mut psd, std::ptr::null_mut()) == 0 {
                RegCloseKey(h);
                return Err(format!("bad SDDL '{sddl}': {}", std::io::Error::last_os_error()));
            }
            let r = RegSetKeySecurity(h, info, psd);
            LocalFree(psd as _);
            RegCloseKey(h);
            if r != ERROR_SUCCESS {
                return Err(format!("RegSetKeySecurity: {}", std::io::Error::from_raw_os_error(r as i32)));
            }
        }
        Ok(())
    }
}

/// "HKLM:\SYSTEM\..." -> "SYSTEM\..."
fn hklm_sub(path: &str) -> String {
    path.split_once(":\\").map(|(_, r)| r.to_string()).unwrap_or_else(|| path.to_string())
}

pub fn key_sddl(path: &str) -> Result<String, String> {
    sec::get_sddl(&hklm_sub(path))
}

/// Ensure SeTakeOwnershipPrivilege + SeRestorePrivilege are enabled (needed to take and to give back ownership).
pub fn enable_owner_privileges() -> Result<(), String> {
    sec::enable_privilege("SeTakeOwnershipPrivilege")?;
    sec::enable_privilege("SeRestorePrivilege")
}

/// Take ownership of an HKLM key for BUILTIN\Administrators, then write `dacl_sddl` ("D:...") as its DACL.
pub fn take_key(path: &str, dacl_sddl: &str) -> Result<(), String> {
    enable_owner_privileges()?;
    let sub = hklm_sub(path);
    sec::set_sddl(&sub, "O:BA", sec::OWNER)?;
    sec::set_sddl(&sub, dacl_sddl, sec::DACL)
}

/// Put back a recorded owner + group + DACL (SeRestorePrivilege lets us hand ownership back to SYSTEM / TrustedInstaller).
pub fn restore_key(path: &str, sddl: &str) -> Result<(), String> {
    enable_owner_privileges()?;
    sec::set_sddl(&hklm_sub(path), sddl, sec::OGD)
}
