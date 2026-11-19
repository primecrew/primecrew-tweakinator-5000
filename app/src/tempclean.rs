//! Clean temp files: a read-only scan/preview, then (after an explicit confirm) delete old,
//! unlocked files strictly inside the temp folders. Files in use are skipped and can optionally
//! be handed to Windows' own delete-at-restart list (MoveFileExW + MOVEFILE_DELAY_UNTIL_REBOOT).
//!
//! Safety rules (enforced by the walker, covered by unit tests):
//! * only folders whose last component is `Temp` / `Tmp` are ever used as roots;
//! * every path touched is built from a root plus directory-entry names and is checked to be
//!   strictly inside that root (the root itself is never deleted);
//! * symlinks, junctions and any other reparse point are never followed and never deleted;
//! * files and folders changed (or created) more recently than the age limit are left alone;
//! * the folder this app is running from is never touched;
//! * no Recycle Bin and no undo: the confirm dialog says so.
#![allow(dead_code)]

use crate::ops::{Log, Lvl};
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const DEFAULT_MIN_AGE_HOURS: u32 = 24;
/// Age limits offered in Expert mode (hours). Beginner mode always uses 24 hours.
pub const AGE_CHOICES: [u32; 4] = [1, 24, 72, 168];
/// Per-file lines written to the log for files that could not be deleted (the rest are counted).
pub const MAX_DETAIL_LINES: usize = 200;
const MAX_DEPTH: usize = 64;

pub fn age_label(h: u32) -> String {
    match h {
        1 => "1 hour".into(),
        h if h % 24 == 0 && h >= 48 => format!("{} days", h / 24),
        h => format!("{h} hours"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RootKind {
    CurrentUser,
    Windows,
    OtherUser,
}

#[derive(Clone, Debug)]
pub struct Root {
    pub kind: RootKind,
    pub label: String,
    pub path: PathBuf,
}

impl Root {
    pub fn shown(&self) -> String {
        display_path(&self.path)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CleanOpts {
    pub include_windows: bool,
    pub include_other_users: bool,
    pub min_age_hours: u32,
}

impl Default for CleanOpts {
    fn default() -> Self {
        CleanOpts { include_windows: true, include_other_users: false, min_age_hours: DEFAULT_MIN_AGE_HOURS }
    }
}

impl CleanOpts {
    /// Beginner mode always uses the defaults: your own %TMP% / %TEMP% (standard location only) plus
    /// C:\Windows\Temp, 24 hours. Other accounts' temp folders are Expert-only.
    pub fn effective(self, expert: bool) -> Self {
        if expert {
            CleanOpts { min_age_hours: self.min_age_hours.max(1), ..self }
        } else {
            CleanOpts::default()
        }
    }
}

#[derive(Clone, Debug)]
pub struct Locked {
    pub path: PathBuf,
    pub root: PathBuf,
    pub bytes: u64,
    pub is_dir: bool,
    pub reason: String,
}

/// Counts for one root. A scan fills `eligible_*`, `recent_*`, `links`, `unreadable`, `excluded`;
/// a clean also fills `deleted_*` and `locked`.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub eligible_files: usize,
    pub eligible_bytes: u64,
    pub deleted_files: usize,
    pub deleted_bytes: u64,
    pub deleted_dirs: usize,
    pub recent_files: usize,
    pub recent_bytes: u64,
    pub links: usize,
    pub unreadable: usize,
    pub excluded: usize,
    pub locked: Vec<Locked>,
    pub locked_bytes: u64,
}

impl Stats {
    fn add(&mut self, o: &Stats) {
        self.eligible_files += o.eligible_files;
        self.eligible_bytes += o.eligible_bytes;
        self.deleted_files += o.deleted_files;
        self.deleted_bytes += o.deleted_bytes;
        self.deleted_dirs += o.deleted_dirs;
        self.recent_files += o.recent_files;
        self.recent_bytes += o.recent_bytes;
        self.links += o.links;
        self.unreadable += o.unreadable;
        self.excluded += o.excluded;
        self.locked.extend(o.locked.iter().cloned());
        self.locked_bytes += o.locked_bytes;
    }
    pub fn locked_files(&self) -> usize {
        self.locked.iter().filter(|l| !l.is_dir).count()
    }
}

#[derive(Clone, Debug)]
pub struct RootResult {
    pub root: Root,
    pub stats: Stats,
}

/// Result of a scan (`cleaned == false`) or a clean (`cleaned == true`).
#[derive(Clone, Debug)]
pub struct Report {
    pub cleaned: bool,
    pub opts: CleanOpts,
    pub when: String,
    pub roots: Vec<RootResult>,
    /// Folders that were found but refused as roots (label, reason).
    pub refused: Vec<(String, String)>,
    pub preview_only: bool,
}

impl Report {
    pub fn total(&self) -> Stats {
        let mut t = Stats::default();
        for r in &self.roots {
            t.add(&r.stats);
        }
        t
    }
}

// ---------------------------------------------------------------- helpers

pub fn fmt_n(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
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
    } else if f >= 1024.0 {
        format!("{:.0} KB", f / 1024.0)
    } else {
        format!("{b} bytes")
    }
}

/// `\\?\C:\x` -> `C:\x` for display (canonicalized paths carry the verbatim prefix on Windows).
pub fn display_path(p: &Path) -> String {
    let s = p.display().to_string();
    if let Some(r) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{r}")
    } else if let Some(r) = s.strip_prefix(r"\\?\") {
        r.to_string()
    } else {
        s
    }
}

/// Normalized key for comparing a path with an entry of PendingFileRenameOperations:
/// strip `\??\` / `\\?\`, use backslashes, lowercase.
pub fn norm_key(s: &str) -> String {
    let s = s.replace('/', "\\");
    let s = s.strip_prefix(r"\??\").or_else(|| s.strip_prefix(r"\\?\")).unwrap_or(&s);
    s.trim_end_matches('\\').to_lowercase()
}

/// A folder may be used as a cleanup root only if it is absolute, at least two folders deep,
/// and its own name is Temp or Tmp. This blocks disasters such as TEMP=C:\ or TEMP=C:\Users\me.
pub fn root_ok(p: &Path) -> Result<(), String> {
    if !p.is_absolute() {
        return Err("not an absolute path".into());
    }
    let normals: Vec<String> = p
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_string()),
            _ => None,
        })
        .collect();
    if p.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
        return Err("path contains . or ..".into());
    }
    if normals.len() < 2 {
        return Err("too close to the top of the drive".into());
    }
    let last = normals.last().map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    if last != "temp" && last != "tmp" {
        return Err(format!("folder is named \"{}\", not Temp", normals.last().cloned().unwrap_or_default()));
    }
    Ok(())
}

fn strictly_inside(p: &Path, root: &Path) -> bool {
    p != root && p.starts_with(root) && !p.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir))
}

fn is_link(md: &std::fs::Metadata) -> bool {
    if md.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

/// Newest of modified and created. Installers often unpack files with old modified dates,
/// so the created time matters too. Unknown time counts as recent (left alone).
fn newest(md: &std::fs::Metadata) -> Option<SystemTime> {
    match (md.modified().ok(), md.created().ok()) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

fn is_recent(md: &std::fs::Metadata, cutoff: SystemTime) -> bool {
    match newest(md) {
        Some(t) => t > cutoff,
        None => true,
    }
}

fn reason(e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(32) => "in use by another program (sharing violation, os error 32)".into(),
        Some(33) => "part of the file is locked by another program (os error 33)".into(),
        Some(5) => "access denied (os error 5)".into(),
        _ => e.to_string(),
    }
}

fn remove_file(p: &Path, md: &std::fs::Metadata) -> std::io::Result<()> {
    match std::fs::remove_file(p) {
        Ok(()) => Ok(()),
        #[cfg(windows)]
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && md.permissions().readonly() => {
            // A read-only temp file: clear the read-only flag (not a link; checked by the caller), then retry.
            let mut perm = md.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perm.set_readonly(false);
            std::fs::set_permissions(p, perm).map_err(|_| e)?;
            std::fs::remove_file(p)
        }
        Err(e) => {
            let _ = md;
            Err(e)
        }
    }
}

// ---------------------------------------------------------------- the walker

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum DirState {
    /// Nothing left that we would keep (empty, or emptied).
    Empty,
    /// Only files / folders we could not delete because they are in use are left.
    OnlyLocked,
    /// Something we deliberately keep is in here (recent, link, excluded, unreadable).
    Keep,
}

struct Walk<'a> {
    root: &'a Path,
    cutoff: SystemTime,
    exclude: &'a [PathBuf],
    delete: bool,
}

impl Walk<'_> {
    fn excluded(&self, p: &Path) -> bool {
        self.exclude.iter().any(|e| p.starts_with(e))
    }
    fn holds_excluded(&self, p: &Path) -> bool {
        self.exclude.iter().any(|e| e.starts_with(p))
    }

    fn dir(&self, dir: &Path, depth: usize, st: &mut Stats) -> DirState {
        if depth > MAX_DEPTH {
            st.unreadable += 1;
            return DirState::Keep;
        }
        let Ok(rd) = std::fs::read_dir(dir) else {
            st.unreadable += 1;
            return DirState::Keep;
        };
        let mut state = DirState::Empty;
        let worse = |s: &mut DirState, n: DirState| {
            *s = match (*s, n) {
                (DirState::Keep, _) | (_, DirState::Keep) => DirState::Keep,
                (DirState::OnlyLocked, _) | (_, DirState::OnlyLocked) => DirState::OnlyLocked,
                _ => DirState::Empty,
            }
        };
        for ent in rd {
            let Ok(ent) = ent else {
                st.unreadable += 1;
                worse(&mut state, DirState::Keep);
                continue;
            };
            let p = dir.join(ent.file_name());
            if !strictly_inside(&p, self.root) {
                worse(&mut state, DirState::Keep);
                continue;
            }
            // symlink_metadata never follows a link / junction.
            let Ok(md) = std::fs::symlink_metadata(&p) else {
                st.unreadable += 1;
                worse(&mut state, DirState::Keep);
                continue;
            };
            if is_link(&md) {
                st.links += 1;
                worse(&mut state, DirState::Keep);
                continue;
            }
            if self.excluded(&p) {
                st.excluded += 1;
                worse(&mut state, DirState::Keep);
                continue;
            }
            if md.is_dir() {
                let recent = is_recent(&md, self.cutoff);
                let sub = self.dir(&p, depth + 1, st);
                let keep_dir = recent || self.holds_excluded(&p);
                match sub {
                    DirState::Empty if !keep_dir => {
                        if self.delete {
                            match std::fs::remove_dir(&p) {
                                Ok(()) => st.deleted_dirs += 1,
                                Err(e) => {
                                    st.locked.push(Locked { path: p.clone(), root: self.root.to_path_buf(), bytes: 0, is_dir: true, reason: reason(&e) });
                                    worse(&mut state, DirState::OnlyLocked);
                                }
                            }
                        }
                    }
                    DirState::OnlyLocked if !keep_dir => {
                        // Deepest first: children were pushed before this folder.
                        if self.delete {
                            st.locked.push(Locked { path: p.clone(), root: self.root.to_path_buf(), bytes: 0, is_dir: true, reason: "contains files in use".into() });
                        }
                        worse(&mut state, DirState::OnlyLocked);
                    }
                    _ => worse(&mut state, DirState::Keep),
                }
                continue;
            }
            let len = md.len();
            if is_recent(&md, self.cutoff) {
                st.recent_files += 1;
                st.recent_bytes += len;
                worse(&mut state, DirState::Keep);
                continue;
            }
            st.eligible_files += 1;
            st.eligible_bytes += len;
            if !self.delete {
                continue;
            }
            match remove_file(&p, &md) {
                Ok(()) => {
                    st.deleted_files += 1;
                    st.deleted_bytes += len;
                }
                Err(e) => {
                    st.locked_bytes += len;
                    st.locked.push(Locked { path: p, root: self.root.to_path_buf(), bytes: len, is_dir: false, reason: reason(&e) });
                    worse(&mut state, DirState::OnlyLocked);
                }
            }
        }
        state
    }
}

/// Scan (`delete == false`) or clean (`delete == true`) one root. The root itself is never removed.
pub fn walk_root(root: &Path, min_age_hours: u32, now: SystemTime, exclude: &[PathBuf], delete: bool) -> Result<Stats, String> {
    root_ok(root)?;
    let md = std::fs::symlink_metadata(root).map_err(|e| e.to_string())?;
    if !md.is_dir() {
        return Err("not a folder".into());
    }
    // Only exclusions strictly inside this root matter (the app's folder, or the exe itself if it sits
    // directly in the root). An app folder above the root does not stop the root being cleaned.
    let exclude: Vec<PathBuf> = exclude.iter().filter(|e| strictly_inside(e, root)).cloned().collect();
    let exclude = &exclude[..];
    let cutoff = now.checked_sub(Duration::from_secs(u64::from(min_age_hours.max(1)) * 3600)).unwrap_or(SystemTime::UNIX_EPOCH);
    let mut st = Stats::default();
    Walk { root, cutoff, exclude, delete }.dir(root, 0, &mut st);
    Ok(st)
}

// ---------------------------------------------------------------- roots

fn canon(p: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(p).ok()
}

/// The exe and the folder the app runs from (never cleaned, e.g. an exe opened straight from a zip in Explorer).
fn exclusions() -> Vec<PathBuf> {
    let Some(exe) = std::env::current_exe().ok().and_then(|p| canon(&p)) else { return vec![] };
    let mut v = vec![exe.clone()];
    if let Some(d) = exe.parent() {
        v.push(d.to_path_buf());
    }
    v
}

pub fn current_user_temp() -> Option<PathBuf> {
    canon(&std::env::temp_dir())
}

/// The user's temp folder is cleaned only when it is the standard `<profile>\AppData\Local\Temp`.
/// Both paths should already be canonical.
pub fn is_standard_user_temp(candidate: &Path, profile: &Path) -> bool {
    let std_temp = profile.join("AppData").join("Local").join("Temp");
    let std_temp = canon(&std_temp).unwrap_or(std_temp);
    norm_key(&candidate.to_string_lossy()) == norm_key(&std_temp.to_string_lossy())
}

pub const CUSTOM_TEMP_NOTE: &str = "points to a custom folder, so it is left alone. Only the standard AppData\\Local\\Temp is cleaned.";

/// The roots for these options plus any folder refused by `root_ok`.
pub fn roots(o: CleanOpts) -> (Vec<Root>, Vec<(String, String)>) {
    let mut out: Vec<Root> = vec![];
    let mut refused = vec![];
    let push = |kind: RootKind, label: String, p: PathBuf, out: &mut Vec<Root>, refused: &mut Vec<(String, String)>| {
        let Some(c) = canon(&p) else { return };
        if out.iter().any(|r| norm_key(&r.path.to_string_lossy()) == norm_key(&c.to_string_lossy())) {
            return;
        }
        match root_ok(&c) {
            Ok(()) => out.push(Root { kind, label, path: c }),
            Err(e) => refused.push((format!("{label} ({})", display_path(&c)), e)),
        }
    };
    #[cfg(windows)]
    {
        // %TMP% and %TEMP% (Windows uses TMP first). Only the standard AppData\Local\Temp is cleaned.
        let profile = std::env::var_os("USERPROFILE").map(PathBuf::from).and_then(|p| canon(&p));
        for var in ["TMP", "TEMP"] {
            let Some(raw) = std::env::var_os(var).map(PathBuf::from) else { continue };
            let Some(c) = canon(&raw) else { continue };
            if out.iter().any(|r| norm_key(&r.path.to_string_lossy()) == norm_key(&c.to_string_lossy())) {
                continue;
            }
            if profile.as_deref().is_some_and(|p| is_standard_user_temp(&c, p)) {
                push(RootKind::CurrentUser, "Your temp folder (%TMP% / %TEMP%)".into(), raw, &mut out, &mut refused);
            } else {
                let label = format!("Your %{var}% ({})", display_path(&c));
                if !refused.iter().any(|(l, _)| *l == label) {
                    refused.push((label, CUSTOM_TEMP_NOTE.into()));
                }
            }
        }
    }
    #[cfg(not(windows))]
    push(RootKind::CurrentUser, "Your temp folder (%TMP% / %TEMP%)".into(), std::env::temp_dir(), &mut out, &mut refused);
    #[cfg(windows)]
    {
        if o.include_windows {
            let win = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
            push(RootKind::Windows, "Windows temp folder".into(), PathBuf::from(win).join("Temp"), &mut out, &mut refused);
        }
        if o.include_other_users {
            for (name, p) in crate::sys::profile_temp_dirs() {
                push(RootKind::OtherUser, format!("Temp folder of {name}"), p, &mut out, &mut refused);
            }
        }
    }
    #[cfg(not(windows))]
    let _ = o;
    (out, refused)
}

fn run(log: &mut Log, o: CleanOpts, delete: bool, expert: bool) -> Report {
    let (roots, refused) = roots(o);
    let exclude = exclusions();
    let now = SystemTime::now();
    let mut results = vec![];
    for (label, why) in &refused {
        log.w(Lvl::Skip, format!("{label} : not used — {why}"));
    }
    if roots.is_empty() {
        log.w(Lvl::Warn, "No temp folder found to clean.");
    }
    let preview_only = delete && !cfg!(windows);
    for r in roots {
        let started = std::time::Instant::now();
        match walk_root(&r.path, o.min_age_hours, now, &exclude, delete && !preview_only) {
            Ok(st) => {
                let shown = r.shown();
                if delete && !preview_only {
                    log.w(Lvl::Set, format!("{} [{shown}] : {} files ({}) -> deleted, {} empty folders removed", r.label, fmt_n(st.deleted_files), fmt_bytes(st.deleted_bytes), fmt_n(st.deleted_dirs)));
                    let lf = st.locked_files();
                    if lf > 0 {
                        log.w(Lvl::Skip, format!("{} : {} files in use ({}) left alone", r.label, fmt_n(lf), fmt_bytes(st.locked_bytes)));
                    }
                    for (i, l) in st.locked.iter().enumerate() {
                        if i >= MAX_DETAIL_LINES {
                            log.file_only(Lvl::Skip, format!("… and {} more items not listed", st.locked.len() - MAX_DETAIL_LINES));
                            break;
                        }
                        let line = format!("in use : {}", display_path(&l.path));
                        if expert {
                            log.w_detail(Lvl::Skip, line, Some(l.reason.clone()));
                        } else {
                            log.file_only(Lvl::Skip, format!("{line}  [{}]", l.reason));
                        }
                    }
                } else {
                    log.w(Lvl::Info, format!("{} [{shown}] : {} files ({}) older than {}", r.label, fmt_n(st.eligible_files), fmt_bytes(st.eligible_bytes), age_label(o.min_age_hours)));
                }
                if st.recent_files > 0 {
                    log.w(Lvl::Skip, format!("{} : {} newer files ({}) changed in the last {} — left alone", r.label, fmt_n(st.recent_files), fmt_bytes(st.recent_bytes), age_label(o.min_age_hours)));
                }
                if st.links > 0 {
                    log.w(Lvl::Skip, format!("{} : {} links / junctions not followed", r.label, fmt_n(st.links)));
                }
                if st.excluded > 0 {
                    log.w(Lvl::Skip, format!("{} : {} items left alone because this app is running from there", r.label, fmt_n(st.excluded)));
                }
                if st.unreadable > 0 {
                    let m = format!("{} : {} folders or items could not be read", r.label, fmt_n(st.unreadable));
                    if expert { log.w(Lvl::Skip, m) } else { log.file_only(Lvl::Skip, m) }
                }
                log.file_only(Lvl::Info, format!("{} took {:.1}s", r.label, started.elapsed().as_secs_f32()));
                results.push(RootResult { root: r, stats: st });
            }
            Err(e) => log.w(Lvl::Warn, format!("{} [{}] : skipped — {e}", r.label, r.shown())),
        }
    }
    if preview_only {
        log.w(Lvl::Info, "UI preview on Linux: nothing was deleted.");
    }
    Report { cleaned: delete, opts: o, when: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(), roots: results, refused, preview_only }
}

/// Read-only. Counts what a clean would delete.
pub fn scan(log: &mut Log, o: CleanOpts, expert: bool) -> Report {
    log.w(Lvl::Info, format!("Scanning temp folders (read-only). Files changed in the last {} are left alone.", age_label(o.min_age_hours)));
    let r = run(log, o, false, expert);
    let t = r.total();
    log.w(Lvl::Info, format!("Scan: {} files ({}) can be cleaned.", fmt_n(t.eligible_files), fmt_bytes(t.eligible_bytes)));
    log.done(format!("Temp scan: {} files, {}", fmt_n(t.eligible_files), fmt_bytes(t.eligible_bytes)));
    r
}

/// Deletes (no Recycle Bin, no undo). Walks the folders again; the earlier scan is only a preview.
pub fn clean(log: &mut Log, o: CleanOpts, expert: bool) -> Report {
    log.w(Lvl::Info, format!("Cleaning temp folders. Files changed in the last {} and files in use are left alone.", age_label(o.min_age_hours)));
    let r = run(log, o, true, expert);
    let t = r.total();
    let lf = t.locked_files();
    log.w(Lvl::Info, format!("Clean temp files: {} files deleted ({} freed), {} in use and left alone ({}).", fmt_n(t.deleted_files), fmt_bytes(t.deleted_bytes), fmt_n(lf), fmt_bytes(t.locked_bytes)));
    log.done(format!("Temp cleanup: {} freed", fmt_bytes(t.deleted_bytes)));
    r
}

// ---------------------------------------------------------------- PendingFileRenameOperations

/// REG_MULTI_SZ bytes -> strings, keeping the empty strings that PendingFileRenameOperations uses
/// as "delete" destinations.
pub fn parse_multi_sz(bytes: &[u8]) -> Vec<String> {
    let w: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    let mut out: Vec<String> = w.split(|c| *c == 0).map(String::from_utf16_lossy).collect();
    // The data ends with "\0\0"; splitting leaves trailing empties that are not entries.
    while out.len() % 2 == 1 || (out.len() >= 2 && out[out.len() - 2].is_empty() && out[out.len() - 1].is_empty()) {
        if out.pop().is_none() {
            break;
        }
    }
    out
}

/// Strings -> (source, destination) pairs. Stops at the first empty source.
pub fn to_pairs(list: &[String]) -> Vec<(String, String)> {
    let mut v = vec![];
    let mut it = list.chunks(2);
    for c in &mut it {
        if c[0].is_empty() {
            break;
        }
        v.push((c[0].clone(), c.get(1).cloned().unwrap_or_default()));
    }
    v
}

pub fn encode_pairs(pairs: &[(String, String)]) -> Vec<u8> {
    let mut w: Vec<u16> = vec![];
    for (s, d) in pairs {
        w.extend(s.encode_utf16());
        w.push(0);
        w.extend(d.encode_utf16());
        w.push(0);
    }
    w.push(0);
    w.iter().flat_map(|c| c.to_le_bytes()).collect()
}

/// Remove only delete-at-restart entries whose source is one of `ours`. Returns (kept, removed count).
pub fn without_ours(pairs: &[(String, String)], ours: &std::collections::HashSet<String>) -> (Vec<(String, String)>, usize) {
    let mut kept = vec![];
    let mut removed = 0;
    for (s, d) in pairs {
        if d.is_empty() && ours.contains(&norm_key(s)) {
            removed += 1;
        } else {
            kept.push((s.clone(), d.clone()));
        }
    }
    (kept, removed)
}

// ---------------------------------------------------------------- scheduled-at-restart record

fn pending_path() -> PathBuf {
    crate::ops::root_dir().join("tempclean-pending.json")
}

fn last_path() -> PathBuf {
    crate::ops::root_dir().join("tempclean-last.json")
}

#[derive(Clone, Debug, Default)]
pub struct PendingInfo {
    pub scheduled_at: String,
    pub items: usize,
    pub files: usize,
    pub bytes: u64,
    /// None of our entries are left in Windows' delete-at-restart list: the restart has happened.
    pub ran: bool,
    pub cleared_files: usize,
    pub cleared_bytes: u64,
    pub still_there: usize,
}

struct Rec {
    path: String,
    bytes: u64,
    dir: bool,
}

fn load_record() -> Option<(String, Vec<Rec>)> {
    let raw = std::fs::read_to_string(pending_path()).ok()?;
    let v: Value = serde_json::from_str(raw.trim_start_matches('\u{feff}')).ok()?;
    let at = v.get("scheduled_at").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let items = v
        .get("items")?
        .as_array()?
        .iter()
        .filter_map(|i| {
            Some(Rec {
                path: i.get("path")?.as_str()?.to_string(),
                bytes: i.get("bytes").and_then(|b| b.as_u64()).unwrap_or(0),
                dir: i.get("dir").and_then(|b| b.as_bool()).unwrap_or(false),
            })
        })
        .collect();
    Some((at, items))
}

fn save_record(at: &str, items: &[Rec]) -> std::io::Result<()> {
    let _ = std::fs::create_dir_all(crate::ops::root_dir());
    let total: u64 = items.iter().map(|r| r.bytes).sum();
    let v = json!({
        "scheduled_at": at,
        "note": "Temp files that PrimeCrew Tweakinator 5000 asked Windows to delete at the next restart (MoveFileEx delay-until-reboot).",
        "bytes": total,
        "items": items.iter().map(|r| json!({"path": r.path, "bytes": r.bytes, "dir": r.dir})).collect::<Vec<_>>(),
    });
    std::fs::write(pending_path(), serde_json::to_string_pretty(&v).unwrap_or_default())
}

#[cfg(windows)]
const SM_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager";

#[cfg(windows)]
fn read_pfro() -> Result<Vec<(String, String)>, String> {
    use winreg::enums::*;
    let k = winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(SM_KEY, KEY_READ).map_err(|e| e.to_string())?;
    match k.get_raw_value("PendingFileRenameOperations") {
        Ok(v) => Ok(to_pairs(&parse_multi_sz(&v.bytes))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(not(windows))]
fn read_pfro() -> Result<Vec<(String, String)>, String> {
    Ok(vec![])
}

/// What was scheduled for the next restart, and (after the restart) what got cleared.
pub fn pending_status() -> Option<PendingInfo> {
    let (at, items) = load_record()?;
    if items.is_empty() {
        return None;
    }
    let pfro: std::collections::HashSet<String> = read_pfro().ok()?.into_iter().filter(|(_, d)| d.is_empty()).map(|(s, _)| norm_key(&s)).collect();
    let ran = !items.iter().any(|r| pfro.contains(&norm_key(&r.path)));
    let mut info = PendingInfo {
        scheduled_at: at,
        items: items.len(),
        files: items.iter().filter(|r| !r.dir).count(),
        bytes: items.iter().map(|r| r.bytes).sum(),
        ran,
        ..Default::default()
    };
    if ran {
        for r in items.iter().filter(|r| !r.dir) {
            if std::fs::symlink_metadata(&r.path).is_ok() {
                info.still_there += 1;
            } else {
                info.cleared_files += 1;
                info.cleared_bytes += r.bytes;
            }
        }
    }
    Some(info)
}

/// After the restart: keep the record as tempclean-last.json and stop showing it.
pub fn dismiss_pending() {
    let _ = std::fs::rename(pending_path(), last_path());
}

#[cfg(windows)]
fn delay_delete(p: &Path) -> Result<(), String> {
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};
    let w: Vec<u16> = p.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    let ok = unsafe { MoveFileExW(w.as_ptr(), std::ptr::null(), MOVEFILE_DELAY_UNTIL_REBOOT) };
    if ok != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

/// Hands the files that were in use (and folders that only held them) to Windows' delete-at-restart
/// list. Each path is checked again first: still there, not a link, strictly inside its temp root.
pub fn schedule(log: &mut Log, locked: &[Locked], expert: bool) {
    // A finished restart cleanup that was not dismissed yet: keep it as the "last" record and start fresh.
    if pending_status().is_some_and(|p| p.ran) {
        dismiss_pending();
    }
    let mut recs: Vec<Rec> = load_record().map(|(_, r)| r).unwrap_or_default();
    let mut known: std::collections::HashSet<String> = recs.iter().map(|r| norm_key(&r.path)).collect();
    let (mut ok, mut skipped, mut failed, mut bytes) = (0usize, 0usize, 0usize, 0u64);
    for l in locked {
        let valid = root_ok(&l.root).is_ok()
            && strictly_inside(&l.path, &l.root)
            && std::fs::symlink_metadata(&l.path).map(|m| !is_link(&m) && m.is_dir() == l.is_dir).unwrap_or(false);
        if !valid {
            skipped += 1;
            continue;
        }
        if known.contains(&norm_key(&l.path.to_string_lossy())) {
            continue;
        }
        #[cfg(windows)]
        let res = delay_delete(&l.path);
        #[cfg(not(windows))]
        let res: Result<(), String> = Err("UI preview on Linux".into());
        match res {
            Ok(()) => {
                ok += 1;
                bytes += l.bytes;
                known.insert(norm_key(&l.path.to_string_lossy()));
                recs.push(Rec { path: display_path(&l.path), bytes: l.bytes, dir: l.is_dir });
                log.file_only(Lvl::Set, format!("delete at restart : {}", display_path(&l.path)));
            }
            Err(e) => {
                failed += 1;
                let line = format!("could not schedule : {}", display_path(&l.path));
                if expert { log.w_detail(Lvl::Skip, line, Some(e)) } else { log.file_only(Lvl::Skip, format!("{line}  [{e}]")) }
            }
        }
    }
    if ok > 0 {
        let at = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        if let Err(e) = save_record(&at, &recs) {
            log.w(Lvl::Warn, format!("Could not save {}: {e}", pending_path().display()));
        }
        log.w(Lvl::Set, format!("Finish cleanup at next restart : {} items ({}) -> added to Windows' delete-at-restart list", fmt_n(ok), fmt_bytes(bytes)));
        log.w(Lvl::Info, "Nothing else happens until you restart. Windows deletes these early in startup, before programs start.");
    }
    if skipped > 0 {
        log.w(Lvl::Skip, format!("{} items were already gone or changed since the cleanup — not scheduled", fmt_n(skipped)));
    }
    if failed > 0 {
        log.w(Lvl::Skip, format!("{} items could not be scheduled", fmt_n(failed)));
    }
    if !cfg!(windows) {
        log.w(Lvl::Info, "UI preview on Linux: nothing was scheduled.");
    }
    log.done(format!("Scheduled {} items for the next restart", fmt_n(ok)));
}

/// Removes only our delete-at-restart entries from PendingFileRenameOperations, then forgets the record.
pub fn cancel_pending(log: &mut Log) {
    let Some((_, recs)) = load_record() else {
        log.w(Lvl::Skip, "No scheduled temp cleanup to cancel.");
        log.done("Nothing to cancel");
        return;
    };
    let ours: std::collections::HashSet<String> = recs.iter().map(|r| norm_key(&r.path)).collect();
    #[cfg(windows)]
    {
        use winreg::enums::*;
        let k = match winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(SM_KEY, KEY_READ | KEY_WRITE) {
            Ok(k) => k,
            Err(e) => {
                log.w(Lvl::Error, format!("Cancel scheduled cleanup: could not open the Session Manager key: {e}"));
                log.done("Cancel failed");
                return;
            }
        };
        let raw = match k.get_raw_value("PendingFileRenameOperations") {
            Ok(v) => Some(v),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                log.w(Lvl::Error, format!("Cancel scheduled cleanup: could not read PendingFileRenameOperations: {e}"));
                log.done("Cancel failed");
                return;
            }
        };
        if let Some(v) = raw {
            let list = parse_multi_sz(&v.bytes);
            let pairs = to_pairs(&list);
            if v.vtype != REG_MULTI_SZ || pairs.len() * 2 != list.len() {
                log.w(Lvl::Error, "Cancel scheduled cleanup: Windows' delete-at-restart list is in an unexpected format, so it was left untouched. Restarting will still run the cleanup.");
                log.done("Cancel failed");
                return;
            }
            let (kept, removed) = without_ours(&pairs, &ours);
            if removed > 0 {
                let res = if kept.is_empty() {
                    k.delete_value("PendingFileRenameOperations")
                } else {
                    k.set_raw_value("PendingFileRenameOperations", &winreg::RegValue { vtype: REG_MULTI_SZ, bytes: encode_pairs(&kept) })
                };
                if let Err(e) = res {
                    log.w(Lvl::Error, format!("Cancel scheduled cleanup: could not update PendingFileRenameOperations: {e}"));
                    log.done("Cancel failed");
                    return;
                }
            }
            log.w(Lvl::Set, format!("Delete-at-restart list : {} of our entries -> removed ({} entries from Windows or other programs left as they were)", removed, kept.len()));
        } else {
            log.w(Lvl::Skip, "Delete-at-restart list is already empty.");
        }
    }
    #[cfg(not(windows))]
    {
        let _ = &ours;
        log.w(Lvl::Info, "UI preview on Linux: nothing was changed.");
    }
    let _ = std::fs::remove_file(pending_path());
    log.done("Scheduled temp cleanup cancelled");
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmpdir(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("tc-test-{name}-{}", std::process::id())).join("Temp");
        let _ = fs::remove_dir_all(base.parent().unwrap());
        fs::create_dir_all(&base).unwrap();
        base.canonicalize().unwrap()
    }

    /// Sets the modified time (files or folders). Tests move "now" forward instead of back-dating
    /// files, because the created time cannot be changed on Linux.
    fn touch_at(p: &Path, t: SystemTime) {
        let f = if p.is_dir() { fs::File::open(p).unwrap() } else { fs::File::options().write(true).open(p).unwrap() };
        f.set_modified(t).unwrap();
    }

    #[test]
    fn root_guard() {
        assert!(root_ok(Path::new("/")).is_err());
        assert!(root_ok(Path::new("/tmp")).is_err(), "one level is too shallow");
        assert!(root_ok(Path::new("/home/me")).is_err());
        assert!(root_ok(Path::new("relative/Temp")).is_err());
        assert!(root_ok(Path::new("/home/me/../Temp")).is_err());
        #[cfg(unix)]
        {
            assert!(root_ok(Path::new("/home/AppData/Local/Temp")).is_ok());
            assert!(root_ok(Path::new("/var/TMP")).is_ok());
        }
        #[cfg(windows)]
        {
            assert!(root_ok(Path::new(r"C:\Users\A\AppData\Local\Temp")).is_ok());
            assert!(root_ok(Path::new(r"\\?\C:\Windows\Temp")).is_ok());
            assert!(root_ok(Path::new(r"C:\Temp")).is_err());
            assert!(root_ok(Path::new(r"C:\")).is_err());
        }
    }

    #[test]
    fn keys_and_display() {
        assert_eq!(norm_key(r"\??\C:\Users\A\AppData\Local\Temp\x.tmp"), r"c:\users\a\appdata\local\temp\x.tmp");
        assert_eq!(norm_key(r"\\?\C:\Windows\Temp\Y\"), r"c:\windows\temp\y");
        assert_eq!(display_path(Path::new(r"\\?\C:\Windows\Temp")), r"C:\Windows\Temp");
        assert_eq!(fmt_bytes(2_469_606_195), "2.3 GB");
        assert_eq!(fmt_n(4812), "4,812");
        assert_eq!(age_label(24), "24 hours");
        assert_eq!(age_label(168), "7 days");
        assert_eq!(CleanOpts { include_windows: false, include_other_users: true, min_age_hours: 1 }.effective(false), CleanOpts::default());
        let d = CleanOpts::default();
        assert!(d.include_windows && !d.include_other_users && d.min_age_hours == 24, "Windows\\Temp is cleaned by default; other accounts are Expert-only");
        assert!(is_standard_user_temp(Path::new("/nonexistent/Users/A/AppData/Local/Temp"), Path::new("/nonexistent/Users/A")));
        assert!(is_standard_user_temp(Path::new("/nonexistent/Users/A/appdata/local/TEMP"), Path::new("/nonexistent/Users/A")));
        assert!(!is_standard_user_temp(Path::new("/nonexistent/Scratch/Temp"), Path::new("/nonexistent/Users/A")));
        assert!(!is_standard_user_temp(Path::new("/nonexistent/Users/B/AppData/Local/Temp"), Path::new("/nonexistent/Users/A")));
    }

    #[test]
    fn multi_sz_roundtrip_and_cancel_only_ours() {
        let pairs = vec![
            (r"\??\C:\Other\setup.tmp".to_string(), String::new()),
            (r"\??\C:\Users\A\AppData\Local\Temp\lock.dat".to_string(), String::new()),
            (r"\??\C:\Prog\new.dll".to_string(), r"!\??\C:\Prog\old.dll".to_string()),
            (r"\??\C:\Users\A\AppData\Local\Temp\Sub".to_string(), String::new()),
        ];
        let bytes = encode_pairs(&pairs);
        let back = to_pairs(&parse_multi_sz(&bytes));
        assert_eq!(back, pairs);
        let ours: std::collections::HashSet<String> = [r"C:\Users\A\AppData\Local\Temp\lock.dat", r"C:\Users\A\AppData\Local\Temp\Sub"].iter().map(|s| norm_key(s)).collect();
        let (kept, removed) = without_ours(&back, &ours);
        assert_eq!(removed, 2);
        assert_eq!(kept, vec![pairs[0].clone(), pairs[2].clone()]);
        // A rename entry pointing at one of our paths is never touched (destination not empty).
        let ren = vec![(r"\??\C:\Users\A\AppData\Local\Temp\lock.dat".to_string(), r"\??\C:\x".to_string())];
        assert_eq!(without_ours(&ren, &ours).1, 0);
        assert!(to_pairs(&parse_multi_sz(&encode_pairs(&[]))).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn walker_is_conservative() {
        let root = tmpdir("walk");
        let now = SystemTime::now();
        let later = now + Duration::from_secs(48 * 3600); // everything created now is 48 h old "later"
        let recent = later - Duration::from_secs(3600);
        let outside = root.parent().unwrap().join("outside");
        fs::create_dir_all(outside.join("deep")).unwrap();
        fs::write(outside.join("keep.txt"), b"precious").unwrap();
        fs::write(outside.join("deep/keep2.txt"), b"precious").unwrap();

        fs::write(root.join("old.tmp"), vec![0u8; 1000]).unwrap();
        fs::write(root.join("new.tmp"), vec![0u8; 10]).unwrap();
        touch_at(&root.join("new.tmp"), recent);
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::write(root.join("a/b/old2.tmp"), vec![0u8; 500]).unwrap();
        fs::create_dir_all(root.join("fresh")).unwrap();
        fs::write(root.join("fresh/n.tmp"), b"x").unwrap();
        touch_at(&root.join("fresh/n.tmp"), recent);
        fs::create_dir_all(root.join("fresh2")).unwrap();
        fs::write(root.join("fresh2/x.tmp"), b"y").unwrap();
        touch_at(&root.join("fresh2"), recent);
        std::os::unix::fs::symlink(&outside, root.join("link-dir")).unwrap();
        std::os::unix::fs::symlink(outside.join("keep.txt"), root.join("link-file")).unwrap();
        fs::create_dir_all(root.join("app")).unwrap();
        fs::write(root.join("app/me.exe"), b"exe").unwrap();
        let exclude = vec![root.join("app")];

        let s = walk_root(&root, 24, later, &exclude, false).unwrap();
        assert_eq!(s.eligible_files, 3);
        assert_eq!(s.eligible_bytes, 1501);
        assert_eq!(s.recent_files, 2);
        assert_eq!(s.links, 2);
        assert_eq!(s.excluded, 1);
        assert!(root.join("old.tmp").exists(), "scan must not delete");

        // Nothing is old yet "now": a clean right away deletes nothing.
        let c = walk_root(&root, 24, now, &exclude, true).unwrap();
        assert_eq!(c.deleted_files, 0);
        assert!(root.join("old.tmp").exists());

        let c = walk_root(&root, 24, later, &exclude, true).unwrap();
        assert_eq!(c.deleted_files, 3);
        assert_eq!(c.deleted_bytes, 1501);
        assert!(c.locked.is_empty());
        assert!(!root.join("old.tmp").exists());
        assert!(!root.join("a").exists(), "emptied old subfolders are removed");
        assert_eq!(c.deleted_dirs, 2);
        assert!(root.join("fresh2").exists(), "a recently changed folder stays even when emptied");
        assert!(root.join("new.tmp").exists());
        assert!(root.join("fresh/n.tmp").exists());
        assert!(root.join("app/me.exe").exists(), "the folder the app runs from is never touched");
        assert!(fs::symlink_metadata(root.join("link-dir")).is_ok(), "links are not deleted");
        assert!(fs::symlink_metadata(root.join("link-file")).is_ok(), "links are not deleted");
        assert!(outside.join("keep.txt").exists() && outside.join("deep/keep2.txt").exists(), "never follows links");
        assert!(outside.join("deep").exists());
        assert!(root.exists(), "root itself is never removed");
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_bad_roots_and_running_folder() {
        let root = tmpdir("refuse");
        let now = SystemTime::now();
        assert!(walk_root(root.parent().unwrap(), 24, now, &[], false).is_err());
        // The app folder above or equal to the root does not block cleaning; the exe itself in the root is kept.
        fs::write(root.join("PrimeCrewTweakinator5000.exe"), b"exe").unwrap();
        let later = now + Duration::from_secs(48 * 3600);
        let s = walk_root(&root, 24, later, &[root.join("PrimeCrewTweakinator5000.exe"), root.clone()], true).unwrap();
        assert_eq!(s.excluded, 1);
        assert!(root.join("PrimeCrewTweakinator5000.exe").exists());
        fs::write(root.join("x.tmp"), b"x").unwrap();
        let s = walk_root(&root, 24, later, &[root.parent().unwrap().to_path_buf()], true).unwrap();
        assert_eq!(s.deleted_files, 2);
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn windows_walker_and_restart_list() {
        use std::os::windows::fs::FileTimesExt;
        let root = std::env::temp_dir().join(format!("tc-win-{}", std::process::id())).join("Temp");
        let _ = fs::remove_dir_all(root.parent().unwrap());
        fs::create_dir_all(root.join("sub")).unwrap();
        let root = root.canonicalize().unwrap();
        let outside = root.parent().unwrap().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep.txt"), b"precious").unwrap();
        let old = SystemTime::now() - Duration::from_secs(72 * 3600);
        let mk_old = |p: &Path| {
            let f = fs::File::options().write(true).open(p).unwrap();
            f.set_times(fs::FileTimes::new().set_modified(old).set_created(old)).unwrap();
        };
        fs::write(root.join("old.tmp"), vec![0u8; 100]).unwrap();
        mk_old(&root.join("old.tmp"));
        fs::write(root.join("sub/ro.tmp"), vec![0u8; 50]).unwrap();
        mk_old(&root.join("sub/ro.tmp"));
        let mut perm = fs::metadata(root.join("sub/ro.tmp")).unwrap().permissions();
        perm.set_readonly(true);
        fs::set_permissions(root.join("sub/ro.tmp"), perm).unwrap();
        fs::write(root.join("new.tmp"), b"n").unwrap();
        // Prefer a junction (mklink /J needs no special privilege); fall back to a directory symlink.
        let _ = std::process::Command::new("cmd").args(["/c", "mklink", "/J"]).arg(root.join("link")).arg(&outside).output();
        if fs::symlink_metadata(root.join("link")).is_err() {
            let _ = std::os::windows::fs::symlink_dir(&outside, root.join("link"));
        }
        let linked = fs::symlink_metadata(root.join("link")).is_ok();
        eprintln!("link created: {linked}");

        let s = walk_root(&root, 24, SystemTime::now(), &[], false).unwrap();
        assert_eq!(s.eligible_files, 2);
        assert_eq!(s.recent_files, 1);
        let c = walk_root(&root, 24, SystemTime::now(), &[], true).unwrap();
        assert_eq!(c.deleted_files, 2, "old and read-only old files deleted: {:?}", c.locked);
        assert!(!root.join("old.tmp").exists() && !root.join("sub/ro.tmp").exists());
        assert!(root.join("new.tmp").exists());
        assert!(outside.join("keep.txt").exists());
        if linked {
            assert_eq!(c.links, 1);
            assert!(fs::symlink_metadata(root.join("link")).is_ok());
        }

        // Delete-at-restart round trip: schedule our file, then cancel removes only ours.
        let target = root.join("locked.tmp");
        fs::write(&target, b"x").unwrap();
        delay_delete(&target).unwrap();
        let ours: std::collections::HashSet<String> = [norm_key(&target.to_string_lossy())].into_iter().collect();
        let pairs = read_pfro().unwrap();
        assert!(pairs.iter().any(|(s, d)| d.is_empty() && ours.contains(&norm_key(s))), "{pairs:?}");
        let (kept, removed) = without_ours(&pairs, &ours);
        assert_eq!(removed, 1);
        use winreg::enums::*;
        let k = winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(SM_KEY, KEY_READ | KEY_WRITE).unwrap();
        if kept.is_empty() {
            k.delete_value("PendingFileRenameOperations").unwrap();
        } else {
            k.set_raw_value("PendingFileRenameOperations", &winreg::RegValue { vtype: REG_MULTI_SZ, bytes: encode_pairs(&kept) }).unwrap();
        }
        assert!(!read_pfro().unwrap().iter().any(|(s, _)| ours.contains(&norm_key(s))));
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }
}
