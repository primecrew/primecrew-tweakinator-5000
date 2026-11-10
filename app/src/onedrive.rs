//! OneDrive removal wizard: read-only scan (the gate), download-to-PC, copy-folders-back, stop sync, remove, undo, verify.
//! Nothing in the OneDrive folder is ever deleted. Every change is logged old -> new and recorded in state.json
//! (Category "OneDrive") so Undo can put it back. Apply all / Revert all never call anything here.
use crate::catalog::*;
use crate::ops::{self, Log, Lvl};
use crate::sys::{self, Cur};
use serde_json::{json, Value};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use windows_sys::core::GUID;
use windows_sys::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Videos};

pub const MODE_ID: &str = "MODE|OneDrive";

/// (friendly name, User Shell Folders value, local-alias value OneDrive also rewrites, known-folder id, default subfolder)
const KNOWN: &[(&str, &str, Option<&str>, &GUID, &str)] = &[
    ("Desktop", "Desktop", None, &FOLDERID_Desktop, "Desktop"),
    ("Documents", "Personal", Some("{F42EE2D3-909F-4907-8871-4C22FC0BF756}"), &FOLDERID_Documents, "Documents"),
    ("Pictures", "My Pictures", Some("{0DDD015D-B06C-45D5-8C4C-F59713854639}"), &FOLDERID_Pictures, "Pictures"),
    ("Music", "My Music", Some("{A0C69A99-21C8-4671-8703-7934162FCF1D}"), &FOLDERID_Music, "Music"),
    ("Videos", "My Video", Some("{35286A68-3C57-41A1-BBB1-0EAE73D76C95}"), &FOLDERID_Videos, "Videos"),
    ("Downloads", "{374DE290-123F-4565-9164-39C4925E467B}", Some("{7D83EE9B-2244-4E70-B1F5-5393042AF1E4}"), &FOLDERID_Downloads, "Downloads"),
];

#[derive(Clone, Debug, Default)]
pub struct KnownF {
    pub label: &'static str,
    pub path: String,
    pub in_od: bool,
    pub cloud: usize,
    idx: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Account {
    pub kind: String,
    pub email: String,
    pub folder: String,
}

#[derive(Clone, Debug, Default)]
pub struct Scan {
    pub mismatch: Option<(String, String)>,
    pub installed: bool,
    pub install_where: Vec<String>,
    pub running: bool,
    pub accounts: Vec<Account>,
    pub roots: Vec<PathBuf>,
    pub known: Vec<KnownF>,
    pub files: usize,
    pub bytes: u64,
    pub cloud_files: usize,
    pub cloud_bytes: u64,
    pub cloud_dirs: usize,
    pub url_files: usize,
    pub errors: usize,
    pub removal_recorded: bool,
    pub at: String,
}

impl Scan {
    /// Reasons the Remove steps stay locked. `hard` ones can't be overridden (wrong account).
    pub fn blockers(&self) -> (Vec<String>, Vec<String>) {
        let mut hard = vec![];
        let mut soft = vec![];
        if let Some((me, s)) = &self.mismatch {
            hard.push(format!("This app is running as {me}, but {s} is signed in. Close it and start it again as {s}."));
        }
        for k in self.known.iter().filter(|k| k.in_od) {
            soft.push(format!("Your {} folder is still inside OneDrive.", k.label));
        }
        if self.cloud_files > 0 || self.cloud_dirs > 0 {
            soft.push(format!("{} files and {} folders still live only in the cloud.", fmt_n(self.cloud_files), fmt_n(self.cloud_dirs)));
        }
        (hard, soft)
    }
}

pub fn fmt_n(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn fmt_bytes(b: u64) -> String {
    let f = b as f64;
    if f >= 1073741824.0 {
        format!("{:.1} GB", f / 1073741824.0)
    } else if f >= 1048576.0 {
        format!("{:.1} MB", f / 1048576.0)
    } else {
        format!("{:.0} KB", f / 1024.0)
    }
}

pub fn removal_recorded() -> bool {
    ops::load_state().contains_key(MODE_ID)
}

fn low(p: &str) -> String {
    p.trim_end_matches('\\').to_ascii_lowercase()
}

fn inside(path: &str, root: &str) -> bool {
    let (p, r) = (low(path), low(root));
    !r.is_empty() && (p == r || p.starts_with(&format!("{r}\\")))
}

// ---------------- detection ----------------
fn local_appdata() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()))
}

fn pf(var: &str, def: &str) -> PathBuf {
    PathBuf::from(std::env::var(var).unwrap_or_else(|_| def.into()))
}

/// OneDrive.exe locations that exist (per-user, per-machine 64/32-bit, and wherever OneDriveTrigger points).
pub fn od_exes() -> Vec<PathBuf> {
    let mut v = vec![
        local_appdata().join(r"Microsoft\OneDrive\OneDrive.exe"),
        pf("ProgramFiles", r"C:\Program Files").join(r"Microsoft OneDrive\OneDrive.exe"),
        pf("ProgramFiles(x86)", r"C:\Program Files (x86)").join(r"Microsoft OneDrive\OneDrive.exe"),
    ];
    if let Some(t) = sys::reg_get(r"HKCU:\Software\Microsoft\OneDrive", "OneDriveTrigger").as_text() {
        v.push(PathBuf::from(sys::expand_env(&t)));
    }
    let mut out: Vec<PathBuf> = vec![];
    for p in v {
        if p.is_file() && !out.iter().any(|q| low(&q.display().to_string()) == low(&p.display().to_string())) {
            out.push(p);
        }
    }
    out
}

fn installed_where() -> Vec<String> {
    let mut w: Vec<String> = od_exes().iter().map(|p| p.display().to_string()).collect();
    for (k, machine) in OD_UNINSTALL_KEYS {
        if sys::key_exists(k) {
            w.push(format!("{} uninstall entry", if *machine { "all-users" } else { "this user's" }));
        }
    }
    w
}

pub fn is_running() -> bool {
    let (_, out) = sys::run("tasklist.exe", &["/FI", "IMAGENAME eq OneDrive.exe", "/FO", "CSV", "/NH"]);
    out.to_ascii_lowercase().contains("\"onedrive.exe\"")
}

fn accounts() -> Vec<Account> {
    let base = r"HKCU:\Software\Microsoft\OneDrive\Accounts";
    let mut v = vec![];
    for sub in sys::reg_subkeys(base) {
        let k = format!(r"{base}\{sub}");
        let folder = sys::reg_get(&k, "UserFolder").as_text().unwrap_or_default();
        if folder.is_empty() {
            continue; // unlinked accounts keep an empty key
        }
        let email = sys::reg_get(&k, "UserEmail").as_text().unwrap_or_default();
        let kind = if sub.eq_ignore_ascii_case("Personal") { "Personal".to_string() } else { format!("Work or school ({sub})") };
        v.push(Account { kind, email, folder });
    }
    v
}

/// Every folder OneDrive syncs into: account folders, synced SharePoint libraries (Tenants\*), and the
/// OneDrive / OneDriveConsumer / OneDriveCommercial environment variables. Existing folders only, nested ones dropped.
fn roots(accts: &[Account]) -> Vec<PathBuf> {
    let mut cand: Vec<String> = accts.iter().map(|a| a.folder.clone()).collect();
    let base = r"HKCU:\Software\Microsoft\OneDrive\Accounts";
    for sub in sys::reg_subkeys(base) {
        let t = format!(r"{base}\{sub}\Tenants");
        for ten in sys::reg_subkeys(&t) {
            for n in sys::reg_value_names(&format!(r"{t}\{ten}")) {
                if n.len() > 3 && n.as_bytes().get(1) == Some(&b':') {
                    cand.push(n);
                }
            }
        }
    }
    for n in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
        if let Some(v) = sys::reg_get(r"HKCU:\Environment", n).as_text() {
            cand.push(sys::expand_env(&v));
        }
        if let Ok(v) = std::env::var(n) {
            cand.push(v);
        }
    }
    let mut out: Vec<String> = vec![];
    for c in cand.into_iter().map(|c| c.trim_end_matches('\\').to_string()).filter(|c| !c.is_empty() && Path::new(c).is_dir()) {
        if !out.iter().any(|o| low(o) == low(&c)) {
            out.push(c);
        }
    }
    let all = out.clone();
    out.retain(|c| !all.iter().any(|o| low(o) != low(c) && inside(c, o)));
    out.into_iter().map(PathBuf::from).collect()
}

fn known_folders(roots: &[PathBuf]) -> Vec<KnownF> {
    let mut v = vec![];
    for (i, (label, val, alias, id, _)) in KNOWN.iter().enumerate() {
        let reg = sys::reg_get(USER_SHELL_FOLDERS, val).as_text().map(|s| sys::expand_env(&s));
        let path = reg.or_else(|| sys::known_folder(id)).unwrap_or_default();
        let alias_path = alias.and_then(|a| sys::reg_get(USER_SHELL_FOLDERS, a).as_text()).map(|s| sys::expand_env(&s)).unwrap_or_default();
        let hit = |p: &str| !p.is_empty() && (roots.iter().any(|r| inside(p, &r.display().to_string())) || (roots.is_empty() && low(p).contains("\\onedrive")));
        let in_od = hit(&path) || hit(&alias_path);
        // Music / Videos / Downloads are only listed when they were moved somewhere.
        if i >= 3 && !in_od {
            continue;
        }
        v.push(KnownF { label, path: if hit(&path) || alias_path.is_empty() { path } else { alias_path }, in_od, cloud: 0, idx: i });
    }
    v
}

#[derive(Default)]
struct Counts {
    files: usize,
    bytes: u64,
    cloud_files: usize,
    cloud_bytes: u64,
    cloud_dirs: usize,
    url: usize,
    errors: usize,
}

/// Counts files and cloud-only items under `root` WITHOUT opening any file (attributes come from the directory
/// listing, so nothing is downloaded). Folders that are themselves cloud-only are counted, not entered.
fn walk(root: &Path, known: &mut [KnownF], c: &mut Counts, cancel: Option<&AtomicBool>) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        if cancel.is_some_and(|x| x.load(Ordering::Relaxed)) {
            return;
        }
        let Ok(rd) = std::fs::read_dir(&d) else {
            c.errors += 1;
            continue;
        };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue; // junctions / symlinks: never followed
            }
            let Ok(md) = e.metadata() else {
                c.errors += 1;
                continue;
            };
            let cloud = md.file_attributes() & CLOUD_ONLY_MASK != 0;
            let p = e.path();
            if cloud {
                let ps = p.display().to_string();
                for k in known.iter_mut().filter(|k| k.in_od && inside(&ps, &k.path)) {
                    k.cloud += 1;
                }
            }
            if ft.is_dir() {
                if cloud {
                    c.cloud_dirs += 1;
                } else {
                    stack.push(p);
                }
            } else {
                c.files += 1;
                c.bytes += md.len();
                if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("url")) {
                    c.url += 1;
                }
                if cloud {
                    c.cloud_files += 1;
                    c.cloud_bytes += md.len();
                }
            }
        }
    }
}

fn count_cloud(roots: &[PathBuf], cancel: Option<&AtomicBool>) -> (usize, usize) {
    let mut c = Counts::default();
    for r in roots {
        walk(r, &mut [], &mut c, cancel);
    }
    (c.cloud_files, c.files)
}

/// Read-only. Logs one PASS / WARN / FAIL / INFO line per check.
pub fn scan(log: &mut Log, expert: bool) -> Scan {
    let mut s = Scan { at: chrono::Local::now().format("%H:%M:%S").to_string(), removal_recorded: removal_recorded(), ..Default::default() };
    s.mismatch = sys::user_mismatch();
    match &s.mismatch {
        Some((me, who)) => log.w(Lvl::Fail, format!("Wrong account: this app runs as {me} but {who} is signed in. HKCU, Desktop and OneDrive would be the wrong person's. Restart the app and approve UAC as {who} (an admin {who}), or sign in as the admin account itself.")),
        None => log.w(Lvl::Pass, format!("Running as the signed-in user ({})", sys::process_user())),
    }
    s.install_where = installed_where();
    s.installed = !s.install_where.is_empty();
    s.running = is_running();
    if s.installed {
        log.w(Lvl::Info, format!("OneDrive is installed{}", if expert { format!(": {}", s.install_where.join("; ")) } else { String::new() }));
    } else {
        log.w(Lvl::Pass, "OneDrive app is not installed");
    }
    log.w(Lvl::Info, format!("OneDrive is {}", if s.running { "running" } else { "not running" }));
    s.accounts = accounts();
    if s.accounts.is_empty() {
        log.w(Lvl::Pass, "No OneDrive account is linked to this PC");
    }
    for a in &s.accounts {
        log.w(Lvl::Warn, format!("Linked account: {} {} -> {} (unlink it in Step 2)", a.kind, a.email, a.folder));
    }
    s.roots = roots(&s.accounts);
    s.known = known_folders(&s.roots);
    let mut c = Counts::default();
    for r in s.roots.clone() {
        let before = (c.files, c.bytes);
        walk(&r, &mut s.known, &mut c, None);
        log.w(Lvl::Info, format!("OneDrive folder {} : {} files, {}", r.display(), fmt_n(c.files - before.0), fmt_bytes(c.bytes - before.1)));
    }
    if s.roots.is_empty() {
        log.w(Lvl::Pass, "No OneDrive folder found");
    }
    for k in &s.known {
        if k.in_od {
            log.w(Lvl::Fail, format!("{} is inside OneDrive: {}{}", k.label, k.path, if expert { format!(" (User Shell Folders\\{})", KNOWN[k.idx].1) } else { String::new() }));
        } else {
            log.w(Lvl::Pass, format!("{} is on this PC: {}", k.label, k.path));
        }
    }
    (s.files, s.bytes, s.cloud_files, s.cloud_bytes, s.cloud_dirs, s.url_files, s.errors) = (c.files, c.bytes, c.cloud_files, c.cloud_bytes, c.cloud_dirs, c.url, c.errors);
    if s.cloud_files == 0 && s.cloud_dirs == 0 {
        log.w(Lvl::Pass, format!("All {} files in OneDrive are on this PC", fmt_n(s.files)));
    } else {
        log.w(Lvl::Fail, format!("{} files ({}) and {} folders are only in the cloud{}", fmt_n(s.cloud_files), fmt_bytes(s.cloud_bytes), fmt_n(s.cloud_dirs), if expert { " (attributes OFFLINE / RECALL_ON_OPEN / RECALL_ON_DATA_ACCESS)" } else { "" }));
    }
    if s.url_files > 0 {
        log.w(Lvl::Warn, format!("{} web-link files (.url) found. OneNote notebooks appear like this and live only online: open them in OneNote and use File > Export if you want a copy on this PC.", fmt_n(s.url_files)));
    }
    if s.errors > 0 {
        log.w(Lvl::Warn, format!("{} folders/files could not be read (no permission or in use)", fmt_n(s.errors)));
    }
    if s.removal_recorded {
        log.w(Lvl::Info, "This wizard has already removed OneDrive here before (Undo is available).");
    }
    let (h, so) = s.blockers();
    if h.is_empty() && so.is_empty() {
        log.w(Lvl::Pass, "Files check passed: everything is on this PC.");
    } else {
        for b in h.iter().chain(so.iter()) {
            log.w(Lvl::Fail, format!("Not ready: {b}"));
        }
    }
    log.done("OneDrive check finished");
    s
}

// ---------------- helpers that record originals ----------------
fn cur_json(path: &str, name: &str) -> (bool, Value, Value) {
    match sys::reg_get(path, name) {
        Cur::Absent => (false, Value::Null, Value::Null),
        Cur::D(v) => (true, json!("DWord"), json!(v)),
        Cur::S(s) => (true, json!(sys::reg_kind(path, name).unwrap_or("String")), json!(s)),
        Cur::Other(k) => (true, json!(k), Value::Null),
    }
}

fn record_reg(path: &str, name: &str) {
    let (existed, kind, value) = cur_json(path, name);
    ops::record_once(&format!("ODREG|{path}|{name}"), ops::obj(json!({"Category": "OneDrive", "Kind": "Registry", "Path": path, "Name": name, "Existed": existed, "ValueKind": kind, "Value": value})));
}

fn remove_value(log: &mut Log, path: &str, name: &str, why: &str) {
    let cur = sys::reg_get(path, name);
    if cur == Cur::Absent {
        log.w(Lvl::Skip, format!("{path}\\{name} not present"));
        return;
    }
    record_reg(path, name);
    match sys::reg_delete_value(path, name) {
        Ok(()) => log.w(Lvl::Set, format!("{path}\\{name} : {} -> <absent> ({why}; original recorded)", cur.show())),
        Err(e) => log.w(Lvl::Error, format!("{path}\\{name} : {e}")),
    }
}

fn set_dword(log: &mut Log, path: &str, name: &str, v: u32, why: &str) {
    let cur = sys::reg_get(path, name);
    if cur == Cur::D(v) {
        log.w(Lvl::Skip, format!("{path}\\{name} already = {v}"));
        return;
    }
    record_reg(path, name);
    match sys::reg_set_dword(path, name, v) {
        Ok(()) => log.w(Lvl::Set, format!("{path}\\{name} : {} -> {v} (DWord) ({why})", cur.show())),
        Err(e) => log.w(Lvl::Error, format!("{path}\\{name} : {e}")),
    }
}

fn kill(log: &mut Log) {
    let (ok, _) = sys::run("taskkill.exe", &["/F", "/IM", "OneDrive.exe"]);
    if ok {
        log.w(Lvl::Set, "Process OneDrive.exe : running -> closed");
    } else {
        log.w(Lvl::Skip, "OneDrive.exe was not running");
    }
}

/// All OneDrive scheduled tasks in any folder (names carry the user SID, e.g. "OneDrive Standalone Update Task-S-1-5-21-…").
fn od_tasks() -> Vec<(String, String, String)> {
    let (_, out) = sys::ps("Get-ScheduledTask -ErrorAction SilentlyContinue | Where-Object { $_.TaskName -like 'OneDrive*' } | ForEach-Object { \"$($_.TaskPath)|$($_.TaskName)|$($_.State)\" }");
    out.lines()
        .filter_map(|l| {
            let p: Vec<&str> = l.trim().splitn(3, '|').collect();
            (p.len() == 3).then(|| (p[0].to_string(), p[1].to_string(), p[2].to_string()))
        })
        .collect()
}

fn guard(log: &mut Log) -> bool {
    if let Some((me, s)) = sys::user_mismatch() {
        log.w(Lvl::Error, format!("Stopped: the app runs as {me} but {s} is signed in. Nothing was changed."));
        log.done("OneDrive: wrong account, nothing changed");
        return false;
    }
    true
}

// ---------------- Step 1 helpers ----------------
pub fn open_onedrive(log: &mut Log) {
    match od_exes().first() {
        Some(p) => {
            sys::shell_open_unelevated(&p.display().to_string());
            log.w(Lvl::Info, "Started OneDrive (as you, not as admin). Click the cloud icon near the clock > gear > Settings.");
        }
        None => log.w(Lvl::Warn, "OneDrive.exe not found (already removed?)."),
    }
    log.done("OneDrive opened");
}

#[derive(Default, Clone, Copy)]
pub struct Progress {
    pub total: usize,
    pub left: usize,
    pub active: bool,
}

/// "Always keep on this device" for every OneDrive folder (attrib +P -U, Microsoft's documented Files On-Demand
/// command), then watches the cloud-only count until it reaches 0 or the user stops watching.
pub fn download_all(log: &mut Log, prog: Arc<Mutex<Progress>>, cancel: Arc<AtomicBool>) {
    if !guard(log) {
        return;
    }
    if !is_running() {
        log.w(Lvl::Error, "OneDrive is not running, so it can't download. Click 'Open OneDrive' first, sign in if asked, then try again.");
        log.done("Download not started");
        return;
    }
    let rs = roots(&accounts());
    let (start_left, _) = count_cloud(&rs, None);
    for r in &rs {
        let rd = r.display().to_string();
        let (ok1, o1) = sys::run("attrib.exe", &["+P", "-U", &rd]);
        let (ok2, o2) = sys::run("attrib.exe", &["+P", "-U", "/S", "/D", &format!("{rd}\\*")]);
        let errs: Vec<&str> = o1.lines().chain(o2.lines()).filter(|l| !l.trim().is_empty()).take(5).collect();
        if ok1 && ok2 && errs.is_empty() {
            log.w(Lvl::Set, format!("{rd} : Files On-Demand -> Always keep on this device (attrib +P -U /S /D)"));
        } else {
            log.w(Lvl::Warn, format!("{rd} : attrib reported: {}", errs.join(" | ")));
        }
    }
    if start_left == 0 {
        log.w(Lvl::Info, "Nothing was cloud-only; all files were already on this PC.");
        log.done("Download: nothing to do");
        return;
    }
    log.w(Lvl::Info, format!("OneDrive is now downloading {} files. You can leave this running; it can take a while.", fmt_n(start_left)));
    *prog.lock().unwrap() = Progress { total: start_left, left: start_left, active: true };
    let mut last_left = start_left;
    loop {
        for _ in 0..20 {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        if cancel.load(Ordering::Relaxed) {
            log.w(Lvl::Info, "Stopped watching. OneDrive keeps downloading in the background; click Check again later.");
            break;
        }
        let (left, _) = count_cloud(&rs, Some(&cancel));
        prog.lock().unwrap().left = left;
        if left != last_left {
            log.w(Lvl::Info, format!("Downloaded {} of {} files", fmt_n(start_left.saturating_sub(left)), fmt_n(start_left)));
            last_left = left;
        }
        if left == 0 {
            log.w(Lvl::Pass, "All files are now on this PC.");
            break;
        }
        if !is_running() {
            log.w(Lvl::Warn, "OneDrive closed, so downloading stopped. Open OneDrive and try again.");
            break;
        }
    }
    prog.lock().unwrap().active = false;
    log.done("Download finished");
}

/// For each Desktop/Documents/... still inside OneDrive: COPY its files to %USERPROFILE%\<Folder> (existing names
/// are skipped and logged, nothing is moved or deleted), then point the folder there (SHSetKnownFolderPath).
pub fn move_back(log: &mut Log) {
    if !guard(log) {
        return;
    }
    let rs = roots(&accounts());
    let mut kf = known_folders(&rs);
    let mut c = Counts::default();
    for r in &rs {
        walk(r, &mut kf, &mut c, None);
    }
    let todo: Vec<KnownF> = kf.into_iter().filter(|k| k.in_od).collect();
    if todo.is_empty() {
        log.w(Lvl::Skip, "No folder points into OneDrive; nothing to move back.");
        log.done("Move back: nothing to do");
        return;
    }
    kill(log); // so OneDrive can't fight the change mid-copy
    let home = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default());
    for k in todo {
        let (_, val, alias, id, sub) = KNOWN[k.idx];
        if k.cloud > 0 {
            log.w(Lvl::Warn, format!("{} skipped: {} items in it are still only in the cloud. Download them first.", k.label, fmt_n(k.cloud)));
            continue;
        }
        let dst = home.join(sub);
        if let Err(e) = std::fs::create_dir_all(&dst) {
            log.w(Lvl::Error, format!("{} : cannot create {} ({e})", k.label, dst.display()));
            continue;
        }
        let (mut copied, mut skipped, mut failed) = (0usize, 0usize, 0usize);
        copy_tree(log, Path::new(&k.path), &dst, &mut copied, &mut skipped, &mut failed);
        log.w(if failed == 0 { Lvl::Set } else { Lvl::Warn }, format!("{} : {} files copied {} -> {} ({} skipped because the name already existed, {} failed; nothing deleted)", k.label, fmt_n(copied), k.path, dst.display(), fmt_n(skipped), failed));
        if failed > 0 {
            log.w(Lvl::Warn, format!("{} still points to OneDrive because some files failed to copy. Fix and run again.", k.label));
            continue;
        }
        let old = sys::reg_get(USER_SHELL_FOLDERS, val).as_text().unwrap_or_default();
        ops::record_once(&format!("ODKF|{}", k.label), ops::obj(json!({"Category": "OneDrive", "Kind": "KnownFolder", "Name": val, "Value": old, "Path": dst.display().to_string()})));
        match sys::set_known_folder(id, &dst.display().to_string()) {
            Ok(()) => log.w(Lvl::Set, format!("{} folder : {} -> {}", k.label, old, dst.display())),
            Err(e) => {
                log.w(Lvl::Error, format!("{} folder : could not change location ({e})", k.label));
                continue;
            }
        }
        if let Some(a) = alias {
            if sys::reg_get(USER_SHELL_FOLDERS, a) != Cur::Absent {
                record_reg(USER_SHELL_FOLDERS, a);
                let v = format!("%USERPROFILE%\\{sub}");
                let o = sys::reg_get(USER_SHELL_FOLDERS, a).show();
                match sys::reg_set_expand(USER_SHELL_FOLDERS, a, &v) {
                    Ok(()) => log.w(Lvl::Set, format!("{USER_SHELL_FOLDERS}\\{a} : {o} -> {v}")),
                    Err(e) => log.w(Lvl::Warn, format!("{USER_SHELL_FOLDERS}\\{a} : {e}")),
                }
            }
        }
    }
    log.w(Lvl::Info, "Your original files are still in the OneDrive folder too. Sign out and back in (or restart) so every app sees the new folders.");
    log.done("Move back finished");
}

fn copy_tree(log: &mut Log, src: &Path, dst: &Path, copied: &mut usize, skipped: &mut usize, failed: &mut usize) {
    let Ok(rd) = std::fs::read_dir(src) else {
        *failed += 1;
        log.w(Lvl::Warn, format!("cannot read {}", src.display()));
        return;
    };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let to = dst.join(e.file_name());
        if ft.is_dir() {
            if std::fs::create_dir_all(&to).is_err() {
                *failed += 1;
                continue;
            }
            copy_tree(log, &e.path(), &to, copied, skipped, failed);
        } else if to.exists() {
            *skipped += 1;
            if *skipped <= 200 {
                log.w(Lvl::Skip, format!("exists, not overwritten: {}", to.display()));
            }
        } else {
            match std::fs::copy(e.path(), &to) {
                Ok(_) => *copied += 1,
                Err(x) => {
                    *failed += 1;
                    log.w(Lvl::Warn, format!("copy failed {} ({x})", e.path().display()));
                }
            }
        }
    }
}

// ---------------- Step 2: stop sync ----------------
fn stop_sync_inner(log: &mut Log) {
    kill(log);
    remove_value(log, OD_RUN_CU, "OneDrive", "OneDrive no longer starts when you sign in");
    let tasks = od_tasks();
    if tasks.is_empty() {
        log.w(Lvl::Skip, "No OneDrive scheduled tasks");
    }
    for (p, n, st) in tasks {
        ops::disable_task(log, &p, &n, &st, Cat::OneDrive, false);
    }
}

pub fn stop_sync(log: &mut Log, restore_point: bool) {
    if !guard(log) {
        return;
    }
    if restore_point && !ops::restore_point(log) {
        log.w(Lvl::Error, "Stopped: the restore point failed. Untick 'restore point' to go on without one.");
        log.done("Stop sync aborted");
        return;
    }
    stop_sync_inner(log);
    log.w(Lvl::Info, "OneDrive is closed and won't start by itself. Your files did not move.");
    log.done("Sync stopped");
}

// ---------------- Step 3: remove ----------------
#[derive(Clone, Copy)]
pub struct RemoveOpts {
    pub restore_point: bool,
    pub nav_pane: bool,
    pub new_users: bool,
}

/// (exe, args, how found). Uninstall registrations first (they know the real version folder), then the versioned
/// folders, then the in-box Windows stubs.
fn uninstallers() -> Vec<(PathBuf, Vec<&'static str>, String)> {
    let mut v: Vec<(PathBuf, Vec<&'static str>, String)> = vec![];
    for (k, machine) in OD_UNINSTALL_KEYS {
        if let Some(u) = sys::reg_get(k, "UninstallString").as_text() {
            let u = sys::expand_env(u.trim());
            let exe = if let Some(r) = u.strip_prefix('"') { r.split('"').next().unwrap_or("").to_string() } else {
                let l = u.to_ascii_lowercase();
                l.find(".exe").map(|i| u[..i + 4].to_string()).unwrap_or(u.clone())
            };
            let args = if *machine { vec!["/uninstall", "/allusers"] } else { vec!["/uninstall"] };
            v.push((PathBuf::from(exe), args, format!("UninstallString in {k}")));
        }
    }
    let newest = |base: PathBuf| -> Option<PathBuf> {
        let mut ds: Vec<PathBuf> = std::fs::read_dir(&base).ok()?.flatten().map(|e| e.path()).filter(|p| p.join("OneDriveSetup.exe").is_file()).collect();
        ds.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().split('.').map(|x| x.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>()).unwrap_or_default());
        ds.pop().map(|d| d.join("OneDriveSetup.exe"))
    };
    if let Some(p) = newest(local_appdata().join(r"Microsoft\OneDrive")) {
        v.push((p, vec!["/uninstall"], "per-user install folder".into()));
    }
    for (var, def) in [("ProgramFiles", r"C:\Program Files"), ("ProgramFiles(x86)", r"C:\Program Files (x86)")] {
        if let Some(p) = newest(pf(var, def).join("Microsoft OneDrive")) {
            v.push((p, vec!["/uninstall", "/allusers"], "all-users install folder".into()));
        }
    }
    let win = pf("SystemRoot", r"C:\Windows");
    for sub in ["System32", "SysWOW64"] {
        v.push((win.join(sub).join("OneDriveSetup.exe"), vec!["/uninstall"], format!("Windows copy in {sub}")));
    }
    let mut seen: Vec<String> = vec![];
    v.retain(|(p, _, _)| {
        let k = low(&p.display().to_string());
        let keep = p.is_file() && !seen.contains(&k);
        seen.push(k);
        keep
    });
    v
}

fn wait_gone(secs: u64) -> bool {
    for _ in 0..secs {
        if installed_where().is_empty() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    installed_where().is_empty()
}

fn uninstall(log: &mut Log) {
    if installed_where().is_empty() {
        log.w(Lvl::Skip, "OneDrive app already not installed");
        return;
    }
    for (exe, args, how) in uninstallers() {
        log.w(Lvl::Info, format!("Uninstalling: \"{}\" {} ({how})", exe.display(), args.join(" ")));
        let (ok, out) = sys::run(&exe.display().to_string(), &args);
        if !out.is_empty() {
            log.w(Lvl::Info, format!("  setup said: {}", out.lines().next().unwrap_or("")));
        }
        if wait_gone(60) {
            log.w(Lvl::Set, "OneDrive app : installed -> uninstalled");
            return;
        }
        log.w(Lvl::Warn, format!("  still installed after that attempt (exit ok={ok}); trying the next method"));
    }
    let (have, _) = sys::run("where.exe", &["winget"]);
    if have {
        log.w(Lvl::Info, "Uninstalling with winget (winget may contact its package source)");
        let (_, out) = sys::run("winget.exe", &["uninstall", "--id", "Microsoft.OneDrive", "-e", "--silent", "--accept-source-agreements", "--disable-interactivity"]);
        log.w(Lvl::Info, format!("  winget: {}", out.lines().last().unwrap_or("")));
        if wait_gone(60) {
            log.w(Lvl::Set, "OneDrive app : installed -> uninstalled (winget)");
            return;
        }
    }
    log.w(Lvl::Error, "OneDrive could not be uninstalled automatically. Use Settings > Apps > Microsoft OneDrive > Uninstall. The blocks below still stop it from syncing.");
}

fn default_hive_file() -> PathBuf {
    let d = sys::reg_get(r"HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList", "Default").as_text().map(|s| sys::expand_env(&s));
    PathBuf::from(d.unwrap_or_else(|| format!("{}\\Users\\Default", std::env::var("SystemDrive").unwrap_or("C:".into())))).join("NTUSER.DAT")
}

/// Loads C:\Users\Default\NTUSER.DAT (the template for NEW accounts), runs `f` on its Run key, unloads it again.
fn with_default_run<R>(log: &mut Log, f: impl FnOnce(&mut Log, &str) -> R) -> Option<R> {
    let file = default_hive_file();
    let (ok, out) = sys::run("reg.exe", &["load", OD_DEFAULT_HIVE, &file.display().to_string()]);
    if !ok {
        log.w(Lvl::Warn, format!("New-user template {} could not be opened ({}); skipped", file.display(), out.lines().next().unwrap_or("")));
        return None;
    }
    let key = format!("HKU:\\{}\\Software\\Microsoft\\Windows\\CurrentVersion\\Run", OD_DEFAULT_HIVE.trim_start_matches("HKU\\"));
    let r = f(log, &key);
    for _ in 0..5 {
        let (ok, _) = sys::run("reg.exe", &["unload", OD_DEFAULT_HIVE]);
        if ok {
            return Some(r);
        }
        std::thread::sleep(std::time::Duration::from_millis(600));
    }
    log.w(Lvl::Warn, format!("{OD_DEFAULT_HIVE} could not be unloaded right now; Windows releases it at restart."));
    Some(r)
}

fn block_new_users(log: &mut Log) {
    with_default_run(log, |log, key| match sys::reg_get(key, "OneDriveSetup") {
        Cur::Absent => log.w(Lvl::Skip, "New-user template has no OneDriveSetup entry"),
        cur => {
            let v = cur.as_text().unwrap_or_default();
            let kind = sys::reg_kind(key, "OneDriveSetup").unwrap_or("String");
            ops::record_once("ODDEFRUN|OneDriveSetup", ops::obj(json!({"Category": "OneDrive", "Kind": "DefaultUserRun", "Name": "OneDriveSetup", "ValueKind": kind, "Value": v})));
            match sys::reg_delete_value(key, "OneDriveSetup") {
                Ok(()) => log.w(Lvl::Set, format!("Default user Run\\OneDriveSetup : \"{v}\" -> <absent> (new accounts won't get OneDrive; original recorded)")),
                Err(e) => log.w(Lvl::Error, format!("Default user Run\\OneDriveSetup : {e}")),
            }
        }
    });
}

pub fn remove(log: &mut Log, o: RemoveOpts) {
    if !guard(log) {
        return;
    }
    log.w(Lvl::Info, format!("OS: {}", sys::os_summary()));
    if o.restore_point && !ops::restore_point(log) {
        log.w(Lvl::Error, "Stopped: the restore point failed. Untick 'restore point' to go on without one.");
        log.done("Remove aborted");
        return;
    }
    let rs = roots(&accounts());
    log.w(Lvl::Info, "----- Stop sync -----");
    stop_sync_inner(log);
    log.w(Lvl::Info, "----- Uninstall -----");
    uninstall(log);
    log.w(Lvl::Info, "----- Blocks (policies) -----");
    for it in REG.iter().filter(|i| i.cat == Cat::OneDrive) {
        ops::set_reg(log, it);
    }
    remove_value(log, OD_RUN_CU, "OneDriveSetup", "no setup at your next sign-in");
    if o.new_users {
        block_new_users(log);
    }
    if o.nav_pane {
        for k in OD_NAV_KEYS {
            if sys::key_exists(k) {
                set_dword(log, k, OD_NAV_VALUE, 0, "hidden from the Explorer side bar");
            }
        }
    }
    let mut st = ops::load_state();
    st.insert(MODE_ID.into(), json!({"Category": "OneDrive", "Kind": "Mode", "NavPane": o.nav_pane, "NewUsers": o.new_users, "RecordedAt": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()}));
    ops::save_state(&st);
    for r in &rs {
        log.w(Lvl::Info, format!("Left in place (not deleted): {} . Delete it yourself later once you're sure you have everything.", r.display()));
    }
    let e = log.errors;
    log.w(if e == 0 { Lvl::Info } else { Lvl::Warn }, format!("OneDrive removal finished with {e} error(s). Restart the PC to finish. A big Windows update can reinstall OneDrive; the blocks keep it from syncing, and you can run this wizard again."));
    log.done("OneDrive removed");
}

// ---------------- Undo ----------------
fn first_exe(cmd: &str) -> String {
    let c = sys::expand_env(cmd.trim());
    if let Some(r) = c.strip_prefix('"') { r.split('"').next().unwrap_or("").to_string() } else {
        let l = c.to_ascii_lowercase();
        l.find(".exe").map(|i| c[..i + 4].to_string()).unwrap_or(c)
    }
}

fn restore_value(log: &mut Log, path: &str, name: &str, e: &Value) -> Result<String, String> {
    let v = &e["Value"];
    match e["ValueKind"].as_str() {
        Some("DWord") => ops::num(v).ok_or("bad value".to_string()).and_then(|d| sys::reg_set_dword(path, name, d)).map(|_| v.to_string()),
        Some("ExpandString") => sys::reg_set_expand(path, name, v.as_str().unwrap_or("")).map(|_| v.to_string()),
        Some("String") => sys::reg_set_string(path, name, v.as_str().unwrap_or("")).map(|_| v.to_string()),
        k => {
            log.w(Lvl::Warn, format!("{path}\\{name}: original type {k:?} not restorable"));
            Err("type".into())
        }
    }
}

pub fn undo(log: &mut Log) {
    if !guard(log) {
        return;
    }
    log.w(Lvl::Info, "Undoing the OneDrive wizard's changes.");
    for it in REG.iter().filter(|i| i.cat == Cat::OneDrive) {
        ops::revert_reg_item(log, it);
    }
    let st = ops::load_state();
    for (id, e) in st.iter().filter(|(_, e)| e["Category"].as_str() == Some("OneDrive")) {
        let (path, name) = (e["Path"].as_str().unwrap_or(""), e["Name"].as_str().unwrap_or(""));
        if id.starts_with("ODREG|") {
            if path == USER_SHELL_FOLDERS {
                log.w(Lvl::Info, format!("{name}: folder location left on this PC (not pointed back into OneDrive)"));
                ops::forget(id);
                continue;
            }
            if path.contains("\\Classes\\") && !sys::key_exists(path) {
                log.w(Lvl::Skip, format!("{path} is gone (removed by the uninstall); nothing to restore"));
                ops::forget(id);
                continue;
            }
            if e["Existed"].as_bool() != Some(true) {
                let _ = sys::reg_delete_value(path, name);
                log.w(Lvl::Set, format!("{path}\\{name} : -> <absent> [was not set before]"));
                ops::forget(id);
                continue;
            }
            if path == OD_RUN_CU {
                let exe = first_exe(e["Value"].as_str().unwrap_or(""));
                if !Path::new(&exe).is_file() {
                    log.w(Lvl::Skip, format!("Run\\{name} not restored: {exe} no longer exists (reinstall OneDrive and it adds this itself)"));
                    ops::forget(id);
                    continue;
                }
            }
            let now = sys::reg_get(path, name).show();
            match restore_value(log, path, name, e) {
                Ok(v) => {
                    log.w(Lvl::Set, format!("{path}\\{name} : {now} -> {v} [original restored]"));
                    ops::forget(id);
                }
                Err(x) => log.w(Lvl::Error, format!("{path}\\{name} : {x}")),
            }
        } else if id.starts_with("TASK|") {
            let (p, n, s) = (e["TaskPath"].as_str().unwrap_or(""), e["TaskName"].as_str().unwrap_or(""), e["State"].as_str().unwrap_or("Ready"));
            if ops::task_state(p, n).is_empty() {
                log.w(Lvl::Skip, format!("Task {p}{n} no longer exists (removed with OneDrive; a reinstall recreates it)"));
                ops::forget(id);
            } else if s == "Disabled" || ops::enable_task(log, p, n, s) {
                ops::forget(id);
            }
        } else if id.starts_with("ODDEFRUN|") {
            let ev = e.clone();
            let done = with_default_run(log, |log, key| {
                let r = restore_value(log, key, "OneDriveSetup", &ev);
                match &r {
                    Ok(v) => log.w(Lvl::Set, format!("Default user Run\\OneDriveSetup : <absent> -> {v} [original restored]")),
                    Err(x) => log.w(Lvl::Error, format!("Default user Run\\OneDriveSetup : {x}")),
                }
                r.is_ok()
            });
            if done == Some(true) {
                ops::forget(id);
            }
        } else if id.starts_with("ODKF|") {
            log.w(Lvl::Info, format!("{} stays on this PC at {} (Undo doesn't move folders back into OneDrive; OneDrive can do that itself under Settings > Sync and backup > Manage backup).", id.trim_start_matches("ODKF|"), path));
            ops::forget(id);
        }
    }
    ops::forget(MODE_ID);
    log.w(Lvl::Info, "To get OneDrive back: run 'winget install Microsoft.OneDrive', or download it from microsoft.com/onedrive/download, or run %SystemRoot%\\SysWOW64\\OneDriveSetup.exe if it is still there. Then sign in. Restart first.");
    log.w(Lvl::Info, format!("Undo finished with {} error(s).", log.errors));
    log.done("OneDrive undo finished");
}

// ---------------- Verify (read-only) ----------------
pub fn verify_status(log: &mut Log, standalone: bool) {
    let rec = removal_recorded();
    let inst = installed_where();
    let running = is_running();
    if !rec && !standalone {
        log.w(Lvl::Info, format!("[OneDrive] wizard not used here; app {}", if inst.is_empty() { "not installed" } else { "installed" }));
        return;
    }
    match (rec, inst.is_empty()) {
        (_, true) => log.w(Lvl::Pass, "[OneDrive] app not installed"),
        (true, false) => log.w(Lvl::Fail, format!("[OneDrive] app is installed again: {}", inst.join("; "))),
        (false, false) => log.w(Lvl::Info, format!("[OneDrive] app installed: {}", inst.join("; "))),
    }
    if running {
        log.w(if rec { Lvl::Fail } else { Lvl::Info }, "[OneDrive] OneDrive.exe is running");
    }
    for it in REG.iter().filter(|i| i.cat == Cat::OneDrive) {
        let cur = sys::reg_get(it.path, it.name);
        let t = format!("[OneDrive] {}\\{}", it.path, it.name);
        if ops::matches(&cur, it.val) {
            log.w(Lvl::Pass, format!("{t} = {}", it.val.text()));
        } else if rec {
            log.w(Lvl::Fail, format!("{t} expected {}, found {}", it.val.text(), cur.show()));
        } else {
            log.w(Lvl::Info, format!("{t} = {}", cur.show()));
        }
    }
    match sys::reg_get(OD_RUN_CU, "OneDrive") {
        Cur::Absent => log.w(Lvl::Pass, "[OneDrive] does not start at sign-in (Run\\OneDrive absent)"),
        c => log.w(if rec { Lvl::Fail } else { Lvl::Info }, format!("[OneDrive] starts at sign-in: {}", c.show())),
    }
    for (p, n, s) in od_tasks() {
        log.w(if s == "Disabled" { Lvl::Pass } else if rec { Lvl::Fail } else { Lvl::Info }, format!("[OneDrive] Task {p}{n} {s}"));
    }
    for r in roots(&accounts()) {
        log.w(Lvl::Info, format!("[OneDrive] folder still on disk (your files): {}", r.display()));
    }
    if standalone {
        log.done("OneDrive verify finished");
    }
}

pub fn restart(log: &mut Log) {
    let (ok, out) = sys::run("shutdown.exe", &["/r", "/t", "20", "/c", "Restarting to finish OneDrive removal. To cancel: shutdown /a"]);
    if ok {
        log.w(Lvl::Set, "Restart : scheduled in 20 seconds (cancel with 'shutdown /a')");
    } else {
        log.w(Lvl::Error, format!("Restart failed: {out}"));
    }
    log.done("Restart scheduled");
}
