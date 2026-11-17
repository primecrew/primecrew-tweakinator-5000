//! Apply / Verify / Revert. Every change is logged as "target : old -> new".
//! Originals are recorded once in C:\ProgramData\Win10Tweaks\state.json (same format as the PowerShell kit).
use crate::catalog::*;
use crate::sys::{self, Cur};
use serde_json::{json, Map, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lvl {
    Info,
    Set,
    Skip,
    Warn,
    Error,
    Pass,
    Fail,
}

impl Lvl {
    pub fn tag(self) -> &'static str {
        match self {
            Lvl::Info => "INFO",
            Lvl::Set => "SET",
            Lvl::Skip => "SKIP",
            Lvl::Warn => "WARN",
            Lvl::Error => "ERROR",
            Lvl::Pass => "PASS",
            Lvl::Fail => "FAIL",
        }
    }
}

pub enum Msg {
    Line(Lvl, String, Option<String>),
    Done(String),
    /// Shown as a beginner-mode dialog after Apply (and Restore update kill).
    Summary(ApplySummary),
}

#[derive(Clone, Debug)]
pub struct ApplySummary {
    pub title: String,
    pub set: usize,
    pub skip: usize,
    pub errors: usize,
    pub problems: Vec<(String, String)>,
}

/// Shown at the top of the on-screen log in Beginner mode, and written once at the start of Apply.
pub const LOG_LEGEND: &str = "'Access denied' followed by SET means Windows resisted and the app handled it. Only red ERROR lines need attention.";

pub fn plain_error(text: &str) -> String {
    let t = text.to_ascii_lowercase();
    if t.contains("aborted: no restore") || t.contains("restore point failed") || t.contains("restore point not created") {
        "A System Restore point could not be created. System Protection may be off for drive C, or Windows already made one in the last 24 hours.".into()
    } else if t.contains("0x80073cf1") || t.contains("package was not found") {
        "An old leftover app entry was already gone, so there was nothing to remove.".into()
    } else if t.contains("appx") || t.contains("provisioned") {
        "A built-in Windows app could not be removed.".into()
    } else if t.contains("firewall") {
        "A firewall rule could not be added or removed.".into()
    } else if t.contains("acl") || t.contains("ownership") {
        "Windows would not let the app change permissions on a protected service key.".into()
    } else if t.contains("service") && (t.contains("could not force") || t.contains("could not stop") || t.contains("could not start")) {
        "A Windows service could not be changed or stopped.".into()
    } else if t.contains("access is denied") || t.contains("os error 5") {
        "Windows blocked a setting change.".into()
    } else if t.contains("ed7ba470") || t.contains("god mode") {
        "The God Mode folder could not be created on the Desktop.".into()
    } else {
        let one = text.split('\n').next().unwrap_or(text);
        if one.chars().count() > 180 {
            format!("{}…", one.chars().take(180).collect::<String>())
        } else {
            one.to_string()
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Opts {
    pub policy_only: bool,
    pub keep_m365: bool,
    pub restore_point: bool,
    pub force_cleanup: bool,
}

pub struct Log {
    tx: Sender<Msg>,
    file: Option<std::fs::File>,
    pub errors: usize,
    pub sets: usize,
    pub skips: usize,
    pub problems: Vec<(String, String)>,
    pub counts: [usize; 3], // pass, fail, warn
}

pub fn root_dir() -> PathBuf {
    PathBuf::from(std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into())).join("Win10Tweaks")
}

impl Log {
    pub fn new(tx: Sender<Msg>, name: &str) -> Self {
        let dir = root_dir().join("Logs");
        let _ = std::fs::create_dir_all(&dir);
        let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let file = std::fs::File::create(dir.join(format!("GUI-{name}-{ts}.log"))).ok();
        Log { tx, file, errors: 0, sets: 0, skips: 0, problems: vec![], counts: [0; 3] }
    }
    pub fn w(&mut self, l: Lvl, s: impl Into<String>) {
        self.w_detail(l, s, None);
    }
    pub fn w_detail(&mut self, l: Lvl, s: impl Into<String>, detail: Option<String>) {
        let s = s.into();
        match l {
            Lvl::Error => {
                self.errors += 1;
                self.problems.push((s.clone(), plain_error(&s)));
            }
            Lvl::Set => self.sets += 1,
            Lvl::Skip => self.skips += 1,
            Lvl::Pass => self.counts[0] += 1,
            Lvl::Fail => self.counts[1] += 1,
            _ => {}
        }
        if let Some(f) = self.file.as_mut() {
            let extra = detail.as_deref().map(|d| format!("  [{d}]")).unwrap_or_default();
            let _ = writeln!(f, "{} [{}] {}{}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), l.tag(), s, extra);
        }
        let _ = self.tx.send(Msg::Line(l, s, detail));
    }
    /// Written to the log file only (not the on-screen log), e.g. per-file detail in Beginner mode.
    pub fn file_only(&mut self, l: Lvl, s: impl Into<String>) {
        if let Some(f) = self.file.as_mut() {
            let _ = writeln!(f, "{} [{}] {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), l.tag(), s.into());
        }
    }
    pub(crate) fn warn_v(&mut self, s: impl Into<String>) {
        self.counts[2] += 1;
        self.w(Lvl::Warn, s);
    }
    pub fn done(&self, s: impl Into<String>) {
        let _ = self.tx.send(Msg::Done(s.into()));
    }
    pub fn emit_summary(&self, title: impl Into<String>) {
        let _ = self.tx.send(Msg::Summary(ApplySummary {
            title: title.into(),
            set: self.sets,
            skip: self.skips,
            errors: self.errors,
            problems: self.problems.clone(),
        }));
    }
}

// ---------------- state.json ----------------
fn state_path() -> PathBuf {
    root_dir().join("state.json")
}

pub fn load_state() -> Map<String, Value> {
    let Ok(raw) = std::fs::read_to_string(state_path()) else { return Map::new() };
    let raw = raw.trim_start_matches('\u{feff}');
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

pub(crate) fn save_state(m: &Map<String, Value>) {
    let _ = std::fs::create_dir_all(root_dir());
    if let Ok(s) = serde_json::to_string_pretty(&Value::Object(m.clone())) {
        let _ = std::fs::write(state_path(), s);
    }
}

pub(crate) fn record_once(id: &str, mut data: Map<String, Value>) {
    let mut st = load_state();
    if st.contains_key(id) {
        return;
    }
    data.insert("RecordedAt".into(), json!(chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()));
    st.insert(id.into(), Value::Object(data));
    save_state(&st);
}

pub(crate) fn forget(id: &str) {
    let mut st = load_state();
    if st.remove(id).is_some() {
        save_state(&st);
    }
}

pub(crate) fn obj(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

// ---------------- registry ----------------
pub(crate) fn matches(cur: &Cur, v: Val) -> bool {
    match (cur, v) {
        (Cur::D(a), Val::D(b)) => *a == b,
        (Cur::S(a), Val::S(b)) => a == b,
        _ => false,
    }
}

pub(crate) fn set_reg(log: &mut Log, it: &RegItem) {
    let target = format!("{}\\{}", it.path, it.name);
    if it.min_build > 0 && sys::os_build() < it.min_build {
        log.w(Lvl::Skip, format!("{target} is for Windows 11 (build {}+); skipped on this PC (build {})", it.min_build, sys::os_build()));
        return;
    }
    let cur = sys::reg_get(it.path, it.name);
    if matches(&cur, it.val) {
        log.w(Lvl::Skip, format!("{target} already = {} ({})", it.val.text(), it.val.kind()));
        return;
    }
    let (existed, kind, value) = match &cur {
        Cur::Absent => (false, Value::Null, Value::Null),
        Cur::D(v) => (true, json!("DWord"), json!(v)),
        Cur::S(s) => (true, json!("String"), json!(s)),
        Cur::Other(k) => (true, json!(k), Value::Null),
    };
    record_once(
        &format!("REG|{}|{}", it.path, it.name),
        obj(json!({"Category": it.cat.id(), "Kind": "Registry", "Path": it.path, "Name": it.name,
                   "Existed": existed, "ValueKind": kind, "Value": value})),
    );
    let r = match it.val {
        Val::D(v) => sys::reg_set_dword(it.path, it.name, v),
        Val::S(s) => sys::reg_set_string(it.path, it.name, s),
    };
    match r {
        Ok(()) => log.w(Lvl::Set, format!("{target} : {} -> {} ({})", cur.show(), it.val.text(), it.val.kind())),
        Err(e) => {
            if let Some((pp, pn)) = it.covered_by {
                let policy = REG.iter().find(|i| i.path == pp && i.name == pn);
                let desired = policy.map(|i| i.val);
                let cur_pol = sys::reg_get(pp, pn);
                if desired.is_some_and(|v| matches(&cur_pol, v)) {
                    let why = if it.name == "ShellFeedsTaskbarViewMode" {
                        "Windows locks this setting; the policy above already turns News and interests off"
                    } else {
                        "Windows locks this setting; the policy above already applies the change"
                    };
                    log.w(Lvl::Skip, format!("{target} : {why}"));
                    return;
                }
            }
            log.w(Lvl::Error, format!("{target} : failed ({e})"));
        }
    }
}

fn apply_reg(log: &mut Log, cat: Cat) {
    apply_reg_mode(log, cat, false);
}

fn apply_reg_mode(log: &mut Log, cat: Cat, policy_only: bool) {
    for it in REG.iter().filter(|i| i.cat == cat && !(policy_only && i.level == Level::Hard)) {
        set_reg(log, it);
    }
}

// ---------------- services ----------------
fn svc_key(name: &str) -> String {
    format!(r"HKLM:\SYSTEM\CurrentControlSet\Services\{name}")
}

fn svc_start(name: &str) -> Option<u32> {
    if !sys::key_exists(&svc_key(name)) {
        return None;
    }
    match sys::reg_get(&svc_key(name), "Start") {
        Cur::D(v) => Some(v),
        _ => None,
    }
}

fn set_service(log: &mut Log, name: &str, start: u32, delayed: Option<u32>, stop: bool, force: bool) {
    let target = format!("Service {name} (Start)");
    let Some(cur) = svc_start(name) else {
        log.w(Lvl::Skip, format!("Service {name} not present"));
        return;
    };
    // Kill mode: take the service KEY (never the binary) so WaaSMedic can't be re-enabled behind our back.
    if force && start == 4 && LOCK_SERVICES.contains(&name) {
        lock_service_key(log, name, true);
    }
    if cur == start {
        log.w(Lvl::Skip, format!("{target} already = {start}"));
    } else {
        let d = match sys::reg_get(&svc_key(name), "DelayedAutostart") {
            Cur::D(v) => json!(v),
            _ => Value::Null,
        };
        record_once(&format!("SVC|{name}"), obj(json!({"Category": "Updates", "Kind": "Service", "Name": name, "Start": cur, "DelayedAutostart": d})));
        let mode = match (start, delayed) {
            (2, Some(1)) => "delayed-auto",
            (2, _) => "auto",
            (3, _) => "demand",
            _ => "disabled",
        };
        let (ok, out) = sys::run("sc.exe", &["config", name, "start=", mode]);
        if ok {
            log.w(Lvl::Set, format!("{target} : {cur} -> {start} ({mode}) via SCM"));
        } else {
            let scm = out.lines().last().unwrap_or("").to_string();
            let mut r = sys::reg_set_dword(&svc_key(name), "Start", start);
            if r.is_err() && force {
                log.w(Lvl::Info, format!("{target} : SCM and registry refused ({scm}); taking ownership of the service registry key (not the binary)"));
                if lock_service_key(log, name, false) {
                    r = sys::reg_set_dword(&svc_key(name), "Start", start);
                }
            }
            match r {
                Ok(()) => {
                    if let Some(dv) = delayed {
                        let _ = sys::reg_set_dword(&svc_key(name), "DelayedAutostart", dv);
                    }
                    log.w_detail(Lvl::Set, format!("{target} : {cur} -> {start} via registry (Windows protects this service — normal; takes effect after reboot)"), Some(format!("SCM refused: {scm}")));
                }
                Err(e) if force => log.w(Lvl::Error, format!("{target} : could not force Start={start} ({e})")),
                Err(e) => log.w(Lvl::Warn, format!("{target} : Windows refused the change ({e})")),
            }
        }
    }
    if stop {
        let (ok, out) = sys::ps(&format!(
            "$s=Get-Service -Name {n} -ErrorAction SilentlyContinue; if($s -and $s.Status -ne 'Stopped'){{ $o=$s.Status; Stop-Service -Name {n} -Force; \"$o\" }}",
            n = sys::ps_quote(name)
        ));
        if ok && !out.is_empty() {
            log.w(Lvl::Set, format!("Service {name} : {out} -> Stopped"));
        } else if !ok {
            log.w(Lvl::Warn, format!("Service {name} : could not stop now ({}); Start=4 keeps it down after reboot", out.lines().next().unwrap_or("")));
        }
    }
}

/// Splits "O:..G:..D:<flags>(aces)" into (dacl flags, aces).
fn dacl_parts(sddl: &str) -> (String, String) {
    let Some(i) = sddl.find("D:") else { return (String::new(), String::new()) };
    let d = &sddl[i + 2..];
    let d = d.split("S:").next().unwrap_or(d);
    let j = d.find('(').unwrap_or(d.len());
    (d[..j].to_string(), d[j..].to_string())
}

/// Owner -> BUILTIN\Administrators and Administrators FullControl on HKLM\...\Services\<name>.
/// With `deny_system` (LOCK_SERVICES) also deny SYSTEM "Set Value" on that one key so WaaSMedic/servicing can't write
/// Start back. SYSTEM keeps read access, so the service manager still reads the config. Original owner+SDDL -> state.json.
fn lock_service_key(log: &mut Log, name: &str, deny_system: bool) -> bool {
    let k = svc_key(name);
    let cur = match sys::key_sddl(&k) {
        Ok(v) => v,
        Err(e) => {
            log.w(Lvl::Error, format!("{k} ACL : cannot read ({e})"));
            return false;
        }
    };
    let (flags, aces) = dacl_parts(&cur);
    let has_admin = aces.contains(ADMIN_FULL_ACE);
    let has_deny = aces.contains(LOCK_DENY_ACE);
    if cur.starts_with("O:BA") && has_admin && (has_deny || !deny_system) {
        log.w(Lvl::Skip, format!("{k} ACL already taken (owner Administrators{})", if deny_system { ", SYSTEM Set Value denied" } else { "" }));
        return true;
    }
    record_once(&format!("ACL|{k}"), obj(json!({"Category": "Updates", "Kind": "KeyAcl", "Path": k, "Sddl": cur})));
    let rest = aces.replace(ADMIN_FULL_ACE, "").replace(LOCK_DENY_ACE, "");
    let dacl = format!("D:{flags}{}{ADMIN_FULL_ACE}{rest}", if deny_system { LOCK_DENY_ACE } else { "" });
    let owner = cur.split("G:").next().unwrap_or("").to_string();
    match sys::take_key(&k, &dacl) {
        Ok(()) => {
            log.w(Lvl::Set, format!("{k} ACL : {owner} -> O:BA; DACL + {ADMIN_FULL_ACE}{} (original SDDL recorded for Revert)", if deny_system { format!(" + {LOCK_DENY_ACE} [SYSTEM may read, not write]") } else { String::new() }));
            true
        }
        Err(e) => {
            log.w(Lvl::Error, format!("{k} ACL : take ownership failed ({e})"));
            false
        }
    }
}

fn restore_acls(log: &mut Log) {
    let st = load_state();
    for (id, e) in st.iter().filter(|(k, _)| k.starts_with("ACL|")) {
        let (p, sd) = (e["Path"].as_str().unwrap_or(""), e["Sddl"].as_str().unwrap_or(""));
        if p.is_empty() || sd.is_empty() {
            forget(id);
            continue;
        }
        let now = sys::key_sddl(p).unwrap_or_default();
        match sys::restore_key(p, sd) {
            Ok(()) => {
                log.w(Lvl::Set, format!("{p} ACL : {now} -> {sd} [original owner + permissions restored]"));
                forget(id);
            }
            Err(x) => log.w(Lvl::Error, format!("{p} ACL : restore failed ({x}); original kept in state.json: {sd}")),
        }
    }
}

// ---------------- run one command as SYSTEM (temporary task, deleted right after) ----------------
fn run_as_system(exe: &str, args: &str) -> (bool, String) {
    let script = format!(
        "$n='Win10Tweaks-SysHelper'; $tp='\\Win10Tweaks\\'; \
         $a=New-ScheduledTaskAction -Execute {e} -Argument {a}; \
         $p=New-ScheduledTaskPrincipal -UserId 'NT AUTHORITY\\SYSTEM' -LogonType ServiceAccount -RunLevel Highest; \
         Register-ScheduledTask -TaskName $n -TaskPath $tp -Action $a -Principal $p -Force | Out-Null; \
         try {{ Start-ScheduledTask -TaskName $n -TaskPath $tp; $i=0; do {{ Start-Sleep -Milliseconds 400; $i++; $t=Get-ScheduledTask -TaskName $n -TaskPath $tp; $inf=Get-ScheduledTaskInfo -TaskName $n -TaskPath $tp }} while (($t.State -eq 'Running' -or $inf.LastTaskResult -eq 267011) -and $i -lt 75); \"$($inf.LastTaskResult)\" }} \
         finally {{ Unregister-ScheduledTask -TaskName $n -TaskPath $tp -Confirm:$false -ErrorAction SilentlyContinue }}",
        e = sys::ps_quote(exe),
        a = sys::ps_quote(args)
    );
    let (ok, out) = sys::ps(&script);
    (ok && out.trim().lines().last() == Some("0"), out)
}

pub(crate) fn task_state(path: &str, name: &str) -> String {
    let (_, out) = sys::ps(&format!("(Get-ScheduledTask -TaskPath {} -TaskName {} -ErrorAction SilentlyContinue).State", sys::ps_quote(path), sys::ps_quote(name)));
    out.trim().to_string()
}

// ---------------- scheduled tasks ----------------
fn list_tasks(folder: &str) -> Vec<(String, String, String)> {
    let (_, out) = sys::ps(&format!(
        "Get-ScheduledTask -TaskPath {} -ErrorAction SilentlyContinue | ForEach-Object {{ \"$($_.TaskPath)|$($_.TaskName)|$($_.State)\" }}",
        sys::ps_quote(folder)
    ));
    out.lines()
        .filter_map(|l| {
            let p: Vec<&str> = l.trim().splitn(3, '|').collect();
            (p.len() == 3).then(|| (p[0].to_string(), p[1].to_string(), p[2].to_string()))
        })
        .collect()
}

pub(crate) fn disable_task(log: &mut Log, path: &str, name: &str, state: &str, cat: Cat, force: bool) {
    let target = format!("Task {path}{name}");
    if state == "Disabled" {
        log.w(Lvl::Skip, format!("{target} already Disabled"));
        return;
    }
    let (ok, out) = sys::ps(&format!("Disable-ScheduledTask -TaskPath {} -TaskName {} | Out-Null", sys::ps_quote(path), sys::ps_quote(name)));
    if ok {
        record_once(&format!("TASK|{path}{name}"), obj(json!({"Category": cat.id(), "Kind": "Task", "TaskPath": path, "TaskName": name, "State": state, "Method": "Admin"})));
        log.w(Lvl::Set, format!("{target} : {state} -> Disabled"));
        return;
    }
    let why = out.lines().next().unwrap_or("").to_string();
    if !force {
        log.w(Lvl::Warn, format!("{target} : Windows refused (protected task: {why}). Left as-is."));
        return;
    }
    // Protected tasks: admins are denied, SYSTEM is not. Run schtasks once as SYSTEM via a throw-away task. No ACL/file changes.
    let (sok, sout) = run_as_system("schtasks.exe", &format!("/Change /TN \"{path}{name}\" /DISABLE"));
    let now = task_state(path, name);
    if now == "Disabled" {
        record_once(&format!("TASK|{path}{name}"), obj(json!({"Category": cat.id(), "Kind": "Task", "TaskPath": path, "TaskName": name, "State": state, "Method": "System"})));
        log.w_detail(Lvl::Set, format!("{target} : {state} -> Disabled (Windows protects this task, so the app used its built-in SYSTEM helper — this is normal)"), Some(format!("as SYSTEM; Disable-ScheduledTask was refused: {why}")));
    } else {
        log.w(Lvl::Warn, format!("{target} : refused even as SYSTEM (now {now}; {}). Harmless while the update services are Start=4.", if sok { "no change".to_string() } else { sout.lines().last().unwrap_or("").to_string() }));
    }
}

pub(crate) fn enable_task(log: &mut Log, path: &str, name: &str, to: &str) -> bool {
    let target = format!("Task {path}{name}");
    let (ok, out) = sys::ps(&format!("Enable-ScheduledTask -TaskPath {} -TaskName {} | Out-Null", sys::ps_quote(path), sys::ps_quote(name)));
    if ok {
        log.w(Lvl::Set, format!("{target} : Disabled -> {to}"));
        return true;
    }
    let _ = run_as_system("schtasks.exe", &format!("/Change /TN \"{path}{name}\" /ENABLE"));
    if task_state(path, name) != "Disabled" {
        log.w(Lvl::Set, format!("{target} : Disabled -> {to} (as SYSTEM)"));
        true
    } else {
        log.w(Lvl::Warn, format!("{target} : enable failed ({})", out.lines().next().unwrap_or("")));
        false
    }
}

// ---------------- firewall (outbound block per update service) ----------------
fn firewall_block(log: &mut Log) {
    for s in FW_SERVICES {
        if svc_start(s).is_none() {
            log.w(Lvl::Skip, format!("Firewall {FW_PREFIX}{s}: service {s} not present"));
            continue;
        }
        let n = format!("{FW_PREFIX}{s}");
        let (ok, out) = sys::ps(&format!(
            "$r=Get-NetFirewallRule -Name {n} -ErrorAction SilentlyContinue; if($r){{ if(\"$($r.Enabled)\" -ne 'True'){{ Enable-NetFirewallRule -Name {n}; 'enabled' }} else {{ 'exists' }} }} else {{ New-NetFirewallRule -Name {n} -DisplayName {n} -Group 'Win10Tweaks' -Description 'Win10Tweaks: block outbound traffic of update service {s} (delete via Revert / Temporarily allow updates)' -Direction Outbound -Action Block -Profile Any -Service {sq} | Out-Null; 'created' }}",
            n = sys::ps_quote(&n),
            sq = sys::ps_quote(s)
        ));
        match (ok, out.trim()) {
            (true, "exists") => log.w(Lvl::Skip, format!("Firewall rule {n} already present (outbound block, service {s})")),
            (true, "enabled") => log.w(Lvl::Set, format!("Firewall rule {n} : disabled -> enabled")),
            (true, _) => {
                record_once(&format!("FW|{n}"), obj(json!({"Category": "Updates", "Kind": "Firewall", "Name": n})));
                log.w(Lvl::Set, format!("Firewall rule {n} : <absent> -> Block outbound for service {s}"));
            }
            (false, o) => log.w(Lvl::Error, format!("Firewall rule {n} : {}", o.lines().next().unwrap_or(""))),
        }
    }
}

fn firewall_remove(log: &mut Log) {
    let (ok, out) = sys::ps(&format!(
        "Get-NetFirewallRule -PolicyStore PersistentStore -ErrorAction SilentlyContinue | Where-Object {{ $_.Name -like '{FW_PREFIX}*' }} | ForEach-Object {{ $n=$_.Name; Remove-NetFirewallRule -Name $n; $n }}"
    ));
    if !ok {
        log.w(Lvl::Error, format!("Firewall rules {FW_PREFIX}* : remove failed ({})", out.lines().next().unwrap_or("")));
        return;
    }
    let names: Vec<&str> = out.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if names.is_empty() {
        log.w(Lvl::Skip, format!("No {FW_PREFIX}* firewall rules present"));
    }
    for n in names {
        log.w(Lvl::Set, format!("Firewall rule {n} : present -> deleted"));
    }
    let st = load_state();
    for id in st.keys().filter(|k| k.starts_with("FW|")) {
        forget(id);
    }
}

// ---------------- Updates mode + temporary allow ----------------
fn set_mode(policy_only: bool) {
    let mut st = load_state();
    st.insert("MODE|Updates".into(), json!({"Category": "Updates", "Kind": "Mode", "PolicyOnly": policy_only, "RecordedAt": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()}));
    save_state(&st);
}

/// None = Updates never applied by this kit.
pub fn mode_policy_only() -> Option<bool> {
    load_state().get("MODE|Updates").map(|e| e["PolicyOnly"].as_bool().unwrap_or(false))
}

fn temp_path() -> PathBuf {
    root_dir().join("tempallow.json")
}

#[derive(Clone, Debug)]
pub struct TempAllow {
    pub since: String,
    /// None = until "Restore update kill" is clicked.
    pub expires: Option<chrono::NaiveDateTime>,
    pub task: bool,
}

const TS: &str = "%Y-%m-%d %H:%M:%S";

pub fn temp_allow_state() -> Option<TempAllow> {
    let raw = std::fs::read_to_string(temp_path()).ok()?;
    let v: Value = serde_json::from_str(raw.trim_start_matches('\u{feff}')).ok()?;
    if v["Active"].as_bool() != Some(true) {
        return None;
    }
    Some(TempAllow {
        since: v["Since"].as_str().unwrap_or("").to_string(),
        expires: v["Expires"].as_str().and_then(|s| chrono::NaiveDateTime::parse_from_str(s, TS).ok()),
        task: v["Task"].as_bool().unwrap_or(false),
    })
}

pub fn temp_expired(t: &TempAllow) -> bool {
    t.expires.is_some_and(|e| chrono::Local::now().naive_local() >= e)
}

fn norm_win_path(p: &Path) -> String {
    let s = p.to_string_lossy();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    s.replace('/', "\\").to_ascii_lowercase()
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    norm_win_path(a) == norm_win_path(b)
}

/// Official install location (`C:\Program Files\PrimeCrew Software\PrimeCrew Tweakinator 5000\...`).
fn installed_exe() -> PathBuf {
    PathBuf::from(std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into()))
        .join("PrimeCrew Software")
        .join("PrimeCrew Tweakinator 5000")
        .join("PrimeCrewTweakinator5000.exe")
}

/// Staging path used by earlier builds, and by a portable run of this build.
/// Keep this so a timer created by an older exe still fires.
fn legacy_restore_task_exe() -> PathBuf {
    PathBuf::from(std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into()))
        .join("Win10Tweaks")
        .join("win10_tweaks.exe")
}

fn running_from_install() -> bool {
    std::env::current_exe().ok().is_some_and(|p| paths_equal(&p, &installed_exe()))
}

/// Exe the SYSTEM restore task should run.
/// Installed copy: point at the installed exe (Program Files is admin-only write).
/// Portable / other: copy to the legacy staging path so older timers and this flow stay compatible.
fn restore_task_exe() -> PathBuf {
    if running_from_install() {
        installed_exe()
    } else {
        legacy_restore_task_exe()
    }
}

/// One-shot SYSTEM task that runs the restore exe with --restore-wu-kill at `at`.
/// When not already running from the installed (or staging) path, the exe is copied to the
/// legacy Program Files staging folder (admin-only write) so a SYSTEM task never runs a user-writable file.
fn schedule_restore(log: &mut Log, at: chrono::NaiveDateTime) -> bool {
    let dst = restore_task_exe();
    let me = std::env::current_exe().ok();
    let already_there = me.as_deref().is_some_and(|p| paths_equal(p, &dst));
    if !already_there {
        let _ = std::fs::create_dir_all(dst.parent().unwrap());
        if let Some(src) = &me {
            if let Err(e) = std::fs::copy(src, &dst) {
                if !dst.exists() {
                    log.w(Lvl::Warn, format!("Timed restore task not created: copy to {} failed ({e}). Keep this app open or reopen it after the time is up.", dst.display()));
                    return false;
                }
            }
        }
    }
    let (ok, out) = sys::ps(&format!(
        "$a=New-ScheduledTaskAction -Execute {exe} -Argument '{RESTORE_FLAG}'; \
         $t=New-ScheduledTaskTrigger -Once -At ([datetime]::ParseExact('{when}','yyyy-MM-dd HH:mm:ss',[Globalization.CultureInfo]::InvariantCulture)); \
         $p=New-ScheduledTaskPrincipal -UserId 'NT AUTHORITY\\SYSTEM' -LogonType ServiceAccount -RunLevel Highest; \
         $s=New-ScheduledTaskSettingsSet -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Minutes 30); \
         Register-ScheduledTask -TaskName '{RESTORE_TASK_NAME}' -TaskPath '{RESTORE_TASK_PATH}' -Action $a -Trigger $t -Principal $p -Settings $s -Description 'Win10Tweaks: re-apply the Windows Update kill after a temporary allow (one-shot, deletes itself)' -Force | Out-Null",
        exe = sys::ps_quote(&dst.display().to_string()),
        when = at.format(TS)
    ));
    if ok {
        log.w(Lvl::Set, format!("Task {RESTORE_TASK_PATH}{RESTORE_TASK_NAME} : <absent> -> runs once at {} as SYSTEM ({} {RESTORE_FLAG})", at.format(TS), dst.display()));
        true
    } else {
        log.w(Lvl::Warn, format!("Timed restore task not created ({}). Keep this app open or reopen it after the time is up.", out.lines().next().unwrap_or("")));
        false
    }
}

fn delete_restore_task(log: &mut Log) {
    let (_, out) = sys::ps(&format!(
        "$t=Get-ScheduledTask -TaskPath '{RESTORE_TASK_PATH}' -TaskName '{RESTORE_TASK_NAME}' -ErrorAction SilentlyContinue; if($t){{ Unregister-ScheduledTask -TaskPath '{RESTORE_TASK_PATH}' -TaskName '{RESTORE_TASK_NAME}' -Confirm:$false; 'removed' }}"
    ));
    if out.trim().ends_with("removed") {
        log.w(Lvl::Set, format!("Task {RESTORE_TASK_PATH}{RESTORE_TASK_NAME} : registered -> removed"));
    }
    // Never delete the installed app. Only remove the legacy staging copy used when a
    // portable (or older) build scheduled the timer. A running exe can't delete itself.
    let staged = legacy_restore_task_exe();
    let running_staged = std::env::current_exe().ok().is_some_and(|p| paths_equal(&p, &staged));
    if staged.exists() && !running_staged && std::fs::remove_file(&staged).is_ok() {
        log.w(Lvl::Set, format!("{} : present -> deleted", staged.display()));
        let _ = std::fs::remove_dir(staged.parent().unwrap());
    }
}

fn clear_temp_allow(log: &mut Log) {
    if temp_path().exists() {
        let _ = std::fs::remove_file(temp_path());
        log.w(Lvl::Set, "Temporary update allow : active -> ended (kill state is authoritative again)");
    }
    delete_restore_task(log);
}

/// Lifts ONLY what Store / WinGet need: no-connect + dead-WSUS policies, the NoWU firewall rules, and wuauserv + UsoSvc
/// (back to their recorded originals). NoAutoUpdate/AUOptions/NoAutoReboot/SetDisableUXWUAccess, WaaSMedicSvc (key stays locked),
/// uhssvc, the disabled tasks and DO mode are untouched, so Windows does not auto-install OS updates in the window.
pub fn temp_allow(log: &mut Log, minutes: Option<u32>) {
    log.w(Lvl::Info, format!("Temporarily allowing Windows Update ({}).", minutes.map(|m| format!("{m} minutes")).unwrap_or("until you click Restore update kill".into())));
    for it in REG.iter().filter(|i| i.cat == Cat::Updates && TEMP_LIFT_REG.contains(&i.name)) {
        revert_reg_item(log, it);
    }
    firewall_remove(log);
    for (s, def, ddef) in SERVICES.iter().filter(|(s, _, _)| TEMP_LIFT_SERVICES.contains(s)) {
        revert_service(log, s, *def, *ddef);
        if svc_start(s) == Some(4) {
            // Nothing recorded (kill applied by another tool?): use Manual for the window; Restore sets 4 again.
            set_service(log, s, 3, None, false, false);
        }
    }
    for s in TEMP_LIFT_SERVICES {
        let (ok, out) = sys::ps(&format!("$s=Get-Service -Name {n}; if($s.Status -ne 'Running'){{ Start-Service -Name {n}; 'started' }}", n = sys::ps_quote(s)));
        if ok && out.trim() == "started" {
            log.w(Lvl::Set, format!("Service {s} : Stopped -> Running"));
        } else if !ok {
            log.w(Lvl::Warn, format!("Service {s} : could not start ({})", out.lines().next().unwrap_or("")));
        }
    }
    let now = chrono::Local::now().naive_local();
    let exp = minutes.map(|m| now + chrono::Duration::minutes(m as i64));
    let task = exp.is_some_and(|e| schedule_restore(log, e));
    let _ = std::fs::create_dir_all(root_dir());
    let _ = std::fs::write(
        temp_path(),
        serde_json::to_string_pretty(&json!({"Active": true, "Since": now.format(TS).to_string(), "Expires": exp.map(|e| e.format(TS).to_string()), "Task": task})).unwrap_or_default(),
    );
    log.w(Lvl::Set, format!("Temporary update allow : off -> ON{}", exp.map(|e| format!(" until {}", e.format(TS))).unwrap_or_default()));
    log.w(Lvl::Warn, "UPDATES TEMPORARILY ALLOWED: Windows Update can reach the internet. Update your Store / WinGet apps, then click Restore update kill.");
    log.w(Lvl::Info, "Still in force: NoAutoUpdate=1 / AUOptions=1 (no automatic OS installs), WaaSMedicSvc disabled + key locked, update tasks disabled.");
    log.done("Updates temporarily allowed");
}

/// Re-applies the Updates category exactly like Apply (same mode as last Apply; default = full kill). No restore point.
pub fn restore_kill(log: &mut Log) {
    log.w(Lvl::Info, "Restoring the Windows Update kill (same as Apply > Updates).");
    log.w(Lvl::Info, LOG_LEGEND);
    log.w(Lvl::Warn, SECURITY_NOTE);
    apply_updates(log, mode_policy_only().unwrap_or(false));
    let e = log.errors;
    if e == 0 {
        log.w(Lvl::Info, "Restore update kill finished. Everything worked. Reboot, then Verify.");
    } else {
        log.w(Lvl::Warn, format!("Restore update kill finished with {e} real problem(s). Reboot, then Verify."));
    }
    log.emit_summary("Restore update kill finished");
    log.done("Update kill restored");
}

/// Headless entry for the one-shot SYSTEM task (no window).
pub fn headless_restore() {
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut log = Log::new(tx, "RestoreWUKill");
    if temp_allow_state().is_none() {
        log.w(Lvl::Info, "No temporary allow active; nothing to restore.");
        clear_temp_allow(&mut log);
        return;
    }
    restore_kill(&mut log);
}

fn apply_updates(log: &mut Log, policy_only: bool) {
    set_mode(policy_only);
    apply_reg_mode(log, Cat::Updates, policy_only);
    if policy_only {
        log.w(Lvl::Info, "Policy only: services, tasks, service-key ACLs and firewall untouched.");
    } else {
        // Tasks first, so Medic/Orchestrator cannot immediately re-enable the services disabled next.
        for f in UPDATE_TASK_FOLDERS {
            let tasks = list_tasks(f);
            if tasks.is_empty() {
                log.w(Lvl::Skip, format!("Task folder {f} empty / not present"));
            }
            for (p, n, s) in tasks {
                disable_task(log, &p, &n, &s, Cat::Updates, true);
            }
        }
        for (s, _, _) in SERVICES {
            set_service(log, s, 4, None, true, true);
        }
        firewall_block(log);
        log.w(Lvl::Info, "Settings > Windows Update will error/be blank while UsoSvc + wuauserv are disabled (expected). Reboot to finish.");
    }
    clear_temp_allow(log);
}

// ---------------- apply ----------------
pub fn restore_point(log: &mut Log) -> bool {
    log.w(Lvl::Info, "Creating System Restore point...");
    let (ok, out) = sys::ps(
        "Checkpoint-Computer -Description 'Win10Tweaks - before Apply (GUI)' -RestorePointType MODIFY_SETTINGS -WarningVariable w -WarningAction SilentlyContinue; if($w){ 'WARN: ' + ($w -join ' ') } else { 'OK' }",
    );
    if ok && out.trim_end().ends_with("OK") {
        log.w(Lvl::Set, "System Restore point : <none> -> created");
        true
    } else if ok {
        log.w(Lvl::Warn, format!("Restore point not created: {out} (Windows allows one per 24h by default)"));
        true
    } else {
        log.w(Lvl::Error, format!("Restore point failed: {} (System Protection off for C:? Enable it in SystemPropertiesProtection.exe)", out.lines().next().unwrap_or("")));
        false
    }
}

pub fn apply(log: &mut Log, cats: &[Cat], o: Opts) {
    log.w(Lvl::Info, format!("OS: {}", sys::os_summary()));
    log.w(Lvl::Info, LOG_LEGEND);
    if cats.contains(&Cat::Updates) {
        log.w(Lvl::Warn, SECURITY_NOTE);
    }
    if o.restore_point && !restore_point(log) {
        log.w(Lvl::Error, "Aborted: no restore point. Untick 'Create restore point' to proceed without one.");
        log.done("Apply aborted");
        return;
    }
    for c in Cat::ALL {
        if !cats.contains(&c) {
            continue;
        }
        log.w(Lvl::Info, format!("----- {} -----", c.title()));
        match c {
            Cat::Cleanup => cleanup(log, o.force_cleanup),
            Cat::Updates => apply_updates(log, o.policy_only),
            Cat::Notifications => {
                apply_reg(log, c);
                for (p, n, s) in list_tasks(r"\Microsoft\Windows\UpdateOrchestrator\") {
                    if n == "USO_UxBroker" {
                        disable_task(log, &p, &n, &s, c, false);
                    }
                }
            }
            Cat::BlockWin11 => {
                if !sys::os_summary().contains("build 19045.") {
                    log.w(Lvl::Warn, "Not build 19045 (22H2): pinning to 22H2 means WU would offer 22H2 if updates are re-enabled.");
                }
                apply_reg(log, c);
            }
            Cat::Copilot => copilot(log, o.keep_m365),
            Cat::Spotlight => {
                apply_reg(log, c);
                log.w(Lvl::Warn, "Win10 lock-screen Weather/Daily Wonder cards: no documented registry switch. Settings > Personalization > Lock screen: Background = Windows spotlight, status/widget app = None, then Background = Picture.");
                log.w(Lvl::Info, "Sign out or restart Explorer to see taskbar changes.");
            }
            Cat::GodMode => godmode(log),
            Cat::OneDrive => {} // never via Apply: only the OneDrive wizard (Cat::ALL excludes it)
        }
    }
    let e = log.errors;
    if e == 0 {
        log.w(Lvl::Info, "Apply finished. Everything worked. Reboot, then Verify.");
    } else {
        log.w(Lvl::Warn, format!("Apply finished with {e} real problem(s). Reboot, then Verify."));
    }
    log.emit_summary("Apply finished");
    log.done("Apply finished");
}


fn package_not_found(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("0x80073cf1") || m.contains("package was not found")
}

/// List Appx packages that are actually installed for some user, skipping stale catalog leftovers.
fn list_installed_appx(name: &str) -> Vec<String> {
    let (_, out) = sys::ps(&format!(
        "Get-AppxPackage -AllUsers -Name {n} -ErrorAction SilentlyContinue | ForEach-Object {{            $full=$_.PackageFullName; $states=@();            try {{ $states=@($_.PackageUserInformation | ForEach-Object {{ [string]$_.InstallState }}) }} catch {{}};            $ok=$states | Where-Object {{ $_ -match 'Installed' }};            $loc=$_.InstallLocation;            if($ok){{ $full }} elseif($loc -and (Test-Path -LiteralPath $loc)){{ $full }}          }}",
        n = sys::ps_quote(name)
    ));
    out.lines().map(|l| l.trim().to_string()).filter(|l| l.starts_with(name)).collect()
}

fn copilot(log: &mut Log, keep_m365: bool) {
    apply_reg(log, Cat::Copilot);
    for p in COPILOT_PROCS {
        let (ok, _) = sys::run("taskkill.exe", &["/F", "/IM", p]);
        if ok {
            log.w(Lvl::Set, format!("Process {p} : running -> killed"));
        }
    }
    let mut pkgs = vec![PKG_COPILOT];
    if !keep_m365 {
        pkgs.push(PKG_M365);
    }
    for (name, family) in pkgs {
        let full = list_installed_appx(name);
        if full.is_empty() {
            log.w(Lvl::Skip, format!("Appx {name} not installed for any user"));
        }
        for f in full {
            let (ok, o1) = sys::ps(&format!("Remove-AppxPackage -Package {} -AllUsers", sys::ps_quote(&f)));
            if ok {
                log.w(Lvl::Set, format!("Appx {f} : installed -> removed (all users)"));
                continue;
            }
            if package_not_found(&o1) {
                log.w(Lvl::Skip, format!("Appx {f} : old leftover entry, nothing to remove"));
                continue;
            }
            let (ok2, o2) = sys::ps(&format!("Remove-AppxPackage -Package {}", sys::ps_quote(&f)));
            if ok2 {
                log.w_detail(
                    Lvl::Set,
                    format!("Appx {f} : installed -> removed for the current user (could not remove it for every account; that is often normal)"),
                    o1.lines().next().map(|s| s.to_string()),
                );
            } else if package_not_found(&o2) {
                log.w(Lvl::Skip, format!("Appx {f} : old leftover entry, nothing to remove"));
            } else {
                log.w(Lvl::Error, format!("Appx {f} : remove failed ({})", o2.lines().next().unwrap_or("")));
            }
        }
        let (_, out) = sys::ps(&format!(
            "Get-AppxProvisionedPackage -Online | Where-Object {{ $_.DisplayName -eq {} }} | ForEach-Object {{ $_.PackageName }}",
            sys::ps_quote(name)
        ));
        for pn in out.lines().map(str::trim).filter(|l| l.starts_with(name)) {
            let (ok, o) = sys::ps(&format!("Remove-AppxProvisionedPackage -Online -PackageName {} | Out-Null", sys::ps_quote(pn)));
            if ok {
                log.w(Lvl::Set, format!("Provisioned {pn} : present -> removed"));
            } else {
                log.w(Lvl::Error, format!("Provisioned {pn} : remove failed ({})", o.lines().next().unwrap_or("")));
            }
        }
        let k = format!(r"{DEPROV_ROOT}\{family}");
        if sys::key_exists(&k) {
            log.w(Lvl::Skip, format!("{k} already exists"));
        } else {
            record_once(&format!("KEY|{k}"), obj(json!({"Category": "Copilot", "Kind": "RegistryKey", "Path": k, "Existed": false})));
            match sys::reg_create_key(&k) {
                Ok(()) => log.w(Lvl::Set, format!("{k} : <absent> -> key created (blocks reinstall on update)")),
                Err(e) => log.w(Lvl::Error, format!("{k} : {e}")),
            }
        }
    }
    log.w(Lvl::Info, "Restart Explorer or sign out for the taskbar button change.");
}

fn godmode_path() -> PathBuf {
    sys::desktop_dir().join(GODMODE_NAME)
}

fn godmode(log: &mut Log) {
    let p = godmode_path();
    if p.exists() {
        log.w(Lvl::Skip, format!("{} already exists", p.display()));
        return;
    }
    match std::fs::create_dir(&p) {
        Ok(()) => {
            record_once(&format!("GODMODE|{}", p.display()), obj(json!({"Category": "GodMode", "Kind": "Folder", "Path": p.display().to_string(), "Existed": false})));
            log.w(Lvl::Set, format!("{} : <absent> -> created", p.display()));
        }
        Err(e) => log.w(Lvl::Error, format!("{} : {e}", p.display())),
    }
}

fn service_status(name: &str) -> String {
    let (_, out) = sys::ps(&format!("(Get-Service -Name {} -ErrorAction SilentlyContinue).Status", sys::ps_quote(name)));
    out.trim().to_string()
}

fn cleanup(log: &mut Log, force: bool) {
    let mut pending = vec![];
    if sys::key_exists(r"HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired") {
        pending.push("WindowsUpdate RebootRequired");
    }
    if sys::key_exists(r"HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending") {
        pending.push("CBS RebootPending");
    }
    if service_status("TrustedInstaller") == "Running" {
        pending.push("TrustedInstaller running (servicing in progress)");
    }
    if !pending.is_empty() {
        if !force {
            log.w(Lvl::Warn, format!("Cleanup refused: {}. Reboot first (or tick 'Force cleanup').", pending.join("; ")));
            return;
        }
        log.w(Lvl::Warn, format!("Force cleanup despite: {}", pending.join("; ")));
    }
    let mut was_running = vec![];
    for s in ["wuauserv", "bits"] {
        let st = service_status(s);
        if st.is_empty() || st == "Stopped" {
            continue;
        }
        if st == "Running" {
            was_running.push(s);
        }
        let (ok, out) = sys::ps(&format!("Stop-Service -Name {} -Force", sys::ps_quote(s)));
        if ok {
            log.w(Lvl::Set, format!("Service {s} : {st} -> Stopped"));
        } else {
            log.w(Lvl::Error, format!("Service {s} : could not stop ({}). Cleanup aborted.", out.lines().next().unwrap_or("")));
            return;
        }
    }
    let win = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let sd = PathBuf::from(win).join("SoftwareDistribution");
    // The ONLY three targets. Nothing else in SoftwareDistribution is touched.
    for dir in ["Download", "PostRebootEventCache.V2"] {
        let d = sd.join(dir);
        let Ok(rd) = std::fs::read_dir(&d) else {
            log.w(Lvl::Skip, format!("{} not present", d.display()));
            continue;
        };
        let (mut n, mut bytes, mut failed) = (0usize, 0u64, 0usize);
        for e in rd.flatten() {
            let p = e.path();
            bytes += dir_size(&p);
            let r = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
            match r {
                Ok(()) => n += 1,
                Err(err) => {
                    failed += 1;
                    log.w(Lvl::Warn, format!("{} : {err}", p.display()));
                }
            }
        }
        if n == 0 && failed == 0 {
            log.w(Lvl::Skip, format!("{} already empty", d.display()));
        } else {
            log.w(Lvl::Set, format!("{}\\* : {n} items / {:.1} MB -> deleted ({failed} failed)", d.display(), bytes as f64 / 1048576.0));
        }
    }
    let f = sd.join("ReportingEvents.log");
    if let Ok(m) = std::fs::metadata(&f) {
        match std::fs::remove_file(&f) {
            Ok(()) => log.w(Lvl::Set, format!("{} : {} bytes -> deleted", f.display(), m.len())),
            Err(e) => log.w(Lvl::Error, format!("{} : {e}", f.display())),
        }
    } else {
        log.w(Lvl::Skip, format!("{} not present", f.display()));
    }
    for s in ["wuauserv", "bits"] {
        if was_running.contains(&s) {
            let (ok, out) = sys::ps(&format!("Start-Service -Name {}", sys::ps_quote(s)));
            if ok {
                log.w(Lvl::Set, format!("Service {s} : Stopped -> Running"));
            } else {
                log.w(Lvl::Warn, format!("Service {s} : could not restart ({})", out.lines().next().unwrap_or("")));
            }
        } else {
            log.w(Lvl::Skip, format!("Service {s} was not running before; left stopped"));
        }
    }
}

fn dir_size(p: &std::path::Path) -> u64 {
    if p.is_file() {
        return std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    }
    std::fs::read_dir(p).map(|rd| rd.flatten().map(|e| dir_size(&e.path())).sum()).unwrap_or(0)
}

// ---------------- verify (read-only) ----------------
pub fn verify(log: &mut Log) {
    let os = sys::os_summary();
    log.w(Lvl::Info, format!("OS: {os}"));
    if os.contains("| 22H2 |") {
        log.w(Lvl::Pass, "OS version 22H2");
    } else {
        log.warn_v("OS is not 22H2; kit targets Windows 10 22H2");
    }
    let mode = mode_policy_only();
    let hard = mode != Some(true);
    let temp = temp_allow_state();
    match (&temp, mode) {
        (Some(t), _) => log.warn_v(format!(
            "[Updates] UPDATES TEMPORARILY ALLOWED since {} ({}) - lifted items below are expected off; click Restore update kill",
            t.since,
            t.expires.map(|e| format!("auto-restore {}", e.format(TS))).unwrap_or("until Restore".into())
        )),
        (None, Some(true)) => log.w(Lvl::Info, "[Updates] Last apply used 'policy only': hard-kill items are reported as SKIP"),
        (None, None) => log.w(Lvl::Info, "[Updates] No Updates apply recorded by this kit"),
        _ => {}
    }
    let od_applied = crate::onedrive::removal_recorded();
    for it in REG {
        let cur = sys::reg_get(it.path, it.name);
        let t = format!("[{}] {}\\{}", it.cat.id(), it.path, it.name);
        if it.min_build > 0 && sys::os_build() < it.min_build {
            log.w(Lvl::Skip, format!("{t} is for Windows 11; not checked on this PC (build {})", sys::os_build()));
            continue;
        }
        if it.covered_by.is_some() {
            // The covering policy is verified as its own catalog item. This per-user value is often locked by Windows.
            log.w(Lvl::Skip, format!("{t} not checked (Windows may lock this value; the matching policy is the one that matters)"));
            continue;
        }
        if it.level == Level::Hard && !hard {
            log.w(Lvl::Skip, format!("{t} (hard kill; not used in policy-only mode) now {}", cur.show()));
            continue;
        }
        if it.cat == Cat::OneDrive && !od_applied {
            continue; // reported by the OneDrive section below
        }
        let lifted = temp.is_some() && it.cat == Cat::Updates && TEMP_LIFT_REG.contains(&it.name);
        if matches(&cur, it.val) {
            if it.level == Level::BestEffort {
                log.warn_v(format!("{t} = {} (set; Microsoft scopes this to another edition/OS, may be ignored)", it.val.text()));
            } else {
                log.w(Lvl::Pass, format!("{t} = {}", it.val.text()));
            }
        } else if lifted {
            log.warn_v(format!("{t} lifted by temporary allow (now {})", cur.show()));
        } else {
            log.w(Lvl::Fail, format!("{t} expected {}, found {}", it.val.text(), cur.show()));
        }
    }
    for (s, _, _) in SERVICES {
        let lifted = temp.is_some() && TEMP_LIFT_SERVICES.contains(s);
        match svc_start(s) {
            None if *s == "uhssvc" => log.w(Lvl::Pass, format!("[Updates] Service {s} not installed (nothing to disable)")),
            None => log.warn_v(format!("[Updates] Service {s} not present")),
            Some(4) => log.w(Lvl::Pass, format!("[Updates] Service {s} Start=4 (Disabled), status={}", service_status(s))),
            Some(v) if !hard => log.w(Lvl::Skip, format!("[Updates] Service {s} Start={v} (policy-only mode)")),
            Some(v) if lifted => log.warn_v(format!("[Updates] Service {s} Start={v}, status={} (temporarily allowed)", service_status(s))),
            Some(v) => log.w(Lvl::Fail, format!("[Updates] Service {s} Start={v}, status={} (expected 4; re-run Apply > Updates)", service_status(s))),
        }
    }
    if hard {
        for name in LOCK_SERVICES {
            let k = svc_key(name);
            match sys::key_sddl(&k) {
                Ok(sd) if sd.starts_with("O:BA") && sd.contains(LOCK_DENY_ACE) => log.w(Lvl::Pass, format!("[Updates] {k} locked (owner Administrators, SYSTEM may not set values)")),
                Ok(sd) => log.w(Lvl::Fail, format!("[Updates] {k} not locked: {sd}")),
                Err(e) if svc_start(name).is_none() => log.warn_v(format!("[Updates] {k}: {e}")),
                Err(e) => log.w(Lvl::Fail, format!("[Updates] {k}: {e}")),
            }
        }
        let (_, out) = sys::ps(&format!(
            "Get-NetFirewallRule -PolicyStore PersistentStore -ErrorAction SilentlyContinue | Where-Object {{ $_.Name -like '{FW_PREFIX}*' }} | ForEach-Object {{ \"$($_.Name)|$($_.Enabled)|$($_.Direction)|$($_.Action)\" }}"
        ));
        for s in FW_SERVICES {
            let n = format!("{FW_PREFIX}{s}");
            if svc_start(s).is_none() {
                continue;
            }
            let row = out.lines().map(str::trim).find(|l| l.split('|').next() == Some(n.as_str()));
            match row {
                Some(r) if r.ends_with("|True|Outbound|Block") => log.w(Lvl::Pass, format!("[Updates] Firewall {n}: outbound block enabled")),
                Some(r) => log.w(Lvl::Fail, format!("[Updates] Firewall {n}: {r}")),
                None if temp.is_some() => log.warn_v(format!("[Updates] Firewall {n}: absent (temporarily allowed)")),
                None => log.w(Lvl::Fail, format!("[Updates] Firewall {n}: missing")),
            }
            // Service-scoped firewall rules match the service SID; a service with ServiceSidType 0 has none.
            if !matches!(sys::reg_get(&svc_key(s), "ServiceSidType"), Cur::D(1) | Cur::D(3)) {
                log.warn_v(format!("[Updates] {s} has no service SID (ServiceSidType 0): its firewall rule may not match; disabled service + no-connect policies still apply"));
            }
        }
    }
    for f in UPDATE_TASK_FOLDERS {
        for (p, n, s) in list_tasks(f) {
            if s == "Disabled" {
                log.w(Lvl::Pass, format!("[Updates] Task {p}{n} Disabled"));
            } else {
                log.warn_v(format!("[Updates] Task {p}{n} {s} (refused even as SYSTEM, or policy-only; harmless while the update services are Start=4)"));
            }
        }
    }
    let (ok, nl) = sys::ps("(New-Object -ComObject Microsoft.Update.AutoUpdate).Settings.NotificationLevel");
    match (ok, nl.trim()) {
        (true, "1") => log.w(Lvl::Pass, "[Updates] WU agent NotificationLevel = 1 (automatic updates disabled)"),
        (true, v) => log.w(Lvl::Fail, format!("[Updates] WU agent NotificationLevel = {v} (1 = disabled)")),
        _ => log.warn_v("[Updates] WU agent NotificationLevel: could not query"),
    }
    let (_, out) = sys::ps("Get-AppxPackage -AllUsers -Name '*Copilot*' | ForEach-Object { $_.PackageFullName }");
    let pk: Vec<&str> = out.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if pk.is_empty() {
        log.w(Lvl::Pass, "[Copilot] Get-AppxPackage -AllUsers *Copilot*: none");
    } else {
        log.w(Lvl::Fail, format!("[Copilot] Installed: {}", pk.join(", ")));
    }
    let (_, out) = sys::ps("Get-AppxPackage -AllUsers -Name 'Microsoft.MicrosoftOfficeHub' | ForEach-Object { $_.PackageFullName }");
    if out.trim().is_empty() {
        log.w(Lvl::Pass, "[Copilot] Microsoft.MicrosoftOfficeHub not installed");
    } else {
        log.warn_v("[Copilot] Microsoft.MicrosoftOfficeHub installed (expected if 'keep Microsoft 365 app')");
    }
    let (_, out) = sys::ps("Get-AppxProvisionedPackage -Online | Where-Object { $_.DisplayName -like '*Copilot*' } | ForEach-Object { $_.PackageName }");
    if out.trim().is_empty() {
        log.w(Lvl::Pass, "[Copilot] No provisioned *Copilot* package");
    } else {
        log.w(Lvl::Fail, format!("[Copilot] Provisioned: {}", out.trim()));
    }
    for (_, fam) in [PKG_COPILOT, PKG_M365] {
        let k = format!(r"{DEPROV_ROOT}\{fam}");
        if sys::key_exists(&k) {
            log.w(Lvl::Pass, format!("[Copilot] {k} present"));
        } else if fam.contains("OfficeHub") {
            log.warn_v(format!("[Copilot] {k} absent (expected if 'keep Microsoft 365 app')"));
        } else {
            log.w(Lvl::Fail, format!("[Copilot] {k} absent"));
        }
    }
    let (_, out) = sys::run("tasklist.exe", &["/FO", "CSV", "/NH"]);
    let running: Vec<&&str> = COPILOT_PROCS.iter().filter(|p| out.to_ascii_lowercase().contains(&format!("\"{}\"", p.to_ascii_lowercase()))).collect();
    if running.is_empty() {
        log.w(Lvl::Pass, "[Copilot] No Copilot process running");
    } else {
        log.w(Lvl::Fail, format!("[Copilot] Running: {running:?}"));
    }
    let g = godmode_path();
    if g.exists() {
        log.w(Lvl::Pass, format!("[GodMode] {} exists", g.display()));
    } else {
        log.w(Lvl::Fail, format!("[GodMode] {} missing", g.display()));
    }
    crate::onedrive::verify_status(log, false);
    let (_, hf) = sys::ps("Get-HotFix | Sort-Object InstalledOn -Descending | Select-Object -First 5 | ForEach-Object { \"$($_.HotFixID)  $($_.Description)  $($_.InstalledOn)\" }");
    log.w(Lvl::Info, "Last 5 updates (Get-HotFix):");
    for l in hf.lines() {
        log.w(Lvl::Info, format!("  {}", l.trim()));
    }
    log.w(Lvl::Info, "gpresult /r lists GPOs only; registry-written local policy will not appear there as an applied GPO.");
    let [p, f, w] = log.counts;
    log.w(Lvl::Info, format!("PASS {p}  FAIL {f}  WARN {w}"));
    log.done(format!("Verify: PASS {p} FAIL {f} WARN {w}"));
}

// ---------------- revert ----------------
pub(crate) fn num(v: &Value) -> Option<u32> {
    v.as_u64().map(|x| x as u32).or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn revert_service(log: &mut Log, s: &str, def: u32, ddef: Option<u32>) {
    let id = format!("SVC|{s}");
    let st = load_state();
    let (start, delayed) = match st.get(&id) {
        Some(e) => (num(&e["Start"]).unwrap_or(def), num(&e["DelayedAutostart"])),
        None => {
            if svc_start(s) == Some(4) {
                log.w(Lvl::Warn, format!("No recorded original for {s}; using stock default Start={def}"));
                (def, ddef)
            } else {
                log.w(Lvl::Skip, format!("Service {s} not disabled by this app; left alone"));
                return;
            }
        }
    };
    set_service(log, s, start, delayed, false, false);
    if svc_start(s) == Some(start) {
        forget(&id);
    }
}

pub(crate) fn revert_reg_item(log: &mut Log, it: &RegItem) {
    let id = format!("REG|{}|{}", it.path, it.name);
    let target = format!("{}\\{}", it.path, it.name);
    let cur = sys::reg_get(it.path, it.name);
    let st = load_state();
    match st.get(&id) {
        Some(e) if e["Existed"].as_bool() == Some(true) => {
            let r = match e["ValueKind"].as_str() {
                Some("DWord") => num(&e["Value"]).ok_or("bad value".to_string()).and_then(|v| sys::reg_set_dword(it.path, it.name, v)),
                Some("String") => sys::reg_set_string(it.path, it.name, e["Value"].as_str().unwrap_or("")),
                k => Err(format!("original type {k:?} not restorable by the app")),
            };
            match r {
                Ok(()) => {
                    log.w(Lvl::Set, format!("{target} : {} -> {} [original restored]", cur.show(), e["Value"]));
                    forget(&id);
                }
                Err(x) => log.w(Lvl::Warn, format!("{target} : {x}")),
            }
        }
        Some(_) => {
            if cur != Cur::Absent {
                match sys::reg_delete_value(it.path, it.name) {
                    Ok(()) => log.w(Lvl::Set, format!("{target} : {} -> <absent> [was not set before]", cur.show())),
                    Err(x) => log.w(Lvl::Error, format!("{target} : {x}")),
                }
            }
            forget(&id);
        }
        None => {
            if matches(&cur, it.val) {
                match sys::reg_delete_value(it.path, it.name) {
                    Ok(()) => log.w(Lvl::Set, format!("{target} : {} -> <absent> [no recorded original; Windows default applies]", cur.show())),
                    Err(x) => log.w(Lvl::Error, format!("{target} : {x}")),
                }
            } else {
                log.w(Lvl::Skip, format!("{target} not set by this kit (now {}) - left alone", cur.show()));
            }
        }
    }
}

pub fn revert(log: &mut Log, cats: &[Cat]) {
    log.w(Lvl::Info, format!("Reverting: {}", cats.iter().map(|c| c.id()).collect::<Vec<_>>().join(", ")));
    if cats.contains(&Cat::Updates) {
        // Stop the timed restore first so it can't re-kill mid-revert.
        clear_temp_allow(log);
        firewall_remove(log);
        for (s, def, ddef) in SERVICES {
            revert_service(log, s, *def, *ddef);
        }
        // After Start is back (we still hold the key), hand owner + permissions back.
        restore_acls(log);
        let st = load_state();
        // OneDrive tasks belong to the OneDrive wizard's own Undo, not to Revert > Updates.
        for (id, e) in st.iter().filter(|(k, e)| k.starts_with("TASK|") && e["Category"].as_str() != Some("OneDrive")) {
            let (p, n, s) = (e["TaskPath"].as_str().unwrap_or(""), e["TaskName"].as_str().unwrap_or(""), e["State"].as_str().unwrap_or("Ready"));
            if s == "Disabled" {
                forget(id);
                continue;
            }
            if enable_task(log, p, n, s) {
                forget(id);
            }
        }
        forget("MODE|Updates");
    }
    for it in REG.iter().filter(|i| cats.contains(&i.cat)) {
        revert_reg_item(log, it);
    }
    if cats.contains(&Cat::Copilot) {
        for (_, fam) in [PKG_COPILOT, PKG_M365] {
            let k = format!(r"{DEPROV_ROOT}\{fam}");
            if sys::key_exists(&k) {
                match sys::reg_delete_key(&k) {
                    Ok(()) => log.w(Lvl::Set, format!("{k} : present -> deleted")),
                    Err(e) => log.w(Lvl::Error, format!("{k} : {e}")),
                }
            }
            forget(&format!("KEY|{k}"));
        }
        log.w(Lvl::Info, "Removed apps are not reinstalled. Get Copilot / Microsoft 365 Copilot from the Microsoft Store if wanted (needs Windows Update service enabled).");
    }
    if cats.contains(&Cat::GodMode) {
        let g = godmode_path();
        if g.exists() {
            match std::fs::remove_dir_all(&g) {
                Ok(()) => log.w(Lvl::Set, format!("{} : present -> removed", g.display())),
                Err(e) => log.w(Lvl::Error, format!("{} : {e}", g.display())),
            }
        }
        forget(&format!("GODMODE|{}", g.display()));
    }
    log.w(Lvl::Info, "Revert finished. Reboot. Revert cannot recreate an unknown earlier state perfectly.");
    log.done("Revert finished");
}
