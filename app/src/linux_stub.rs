//! Minimal stand-ins so the GUI can be built and screenshotted on Linux.
//! These never change a real system; actions only write a log line.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use crate::catalog::Cat;

pub mod ops {
    use super::*;
    use serde_json::{Map, Value};
    use std::io::Write;

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
        Line(Lvl, String),
        Done(String),
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
        pub counts: [usize; 3],
    }

    pub fn root_dir() -> PathBuf {
        std::env::temp_dir().join("Win10Tweaks")
    }

    impl Log {
        pub fn new(tx: Sender<Msg>, name: &str) -> Self {
            let dir = root_dir().join("Logs");
            let _ = std::fs::create_dir_all(&dir);
            let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
            let file = std::fs::File::create(dir.join(format!("GUI-{name}-{ts}.log"))).ok();
            Log { tx, file, errors: 0, counts: [0; 3] }
        }
        pub fn w(&mut self, l: Lvl, s: impl Into<String>) {
            let s = s.into();
            if let Some(f) = self.file.as_mut() {
                let _ = writeln!(f, "{} [{}] {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), l.tag(), s);
            }
            let _ = self.tx.send(Msg::Line(l, s));
        }
        pub fn done(&self, s: impl Into<String>) {
            let _ = self.tx.send(Msg::Done(s.into()));
        }
    }

    pub fn load_state() -> Map<String, Value> {
        Map::new()
    }

    #[derive(Clone, Debug)]
    pub struct TempAllow {
        pub since: String,
        pub expires: Option<chrono::NaiveDateTime>,
        pub task: bool,
    }

    pub fn temp_allow_state() -> Option<TempAllow> {
        None
    }

    pub fn temp_expired(_t: &TempAllow) -> bool {
        false
    }

    fn preview(log: &mut Log, name: &str) {
        log.w(Lvl::Info, format!("UI preview on Linux: {name} does not change this computer."));
        log.done(format!("Preview: {name} skipped"));
    }

    pub fn temp_allow(log: &mut Log, _minutes: Option<u32>) {
        preview(log, "Temporary allow");
    }
    pub fn restore_kill(log: &mut Log) {
        preview(log, "Restore update kill");
    }
    pub fn apply(log: &mut Log, _cats: &[Cat], _o: Opts) {
        preview(log, "Apply");
    }
    pub fn verify(log: &mut Log) {
        preview(log, "Verify");
    }
    pub fn revert(log: &mut Log, _cats: &[Cat]) {
        preview(log, "Revert");
    }
}

pub mod sys {
    use super::*;

    pub fn is_admin() -> bool {
        true
    }
    pub fn relaunch_elevated() -> bool {
        false
    }
    pub fn desktop_dir() -> PathBuf {
        dirs_fallback()
    }
    fn dirs_fallback() -> PathBuf {
        std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp")).join("Desktop")
    }
    pub fn user_mismatch() -> Option<(String, String)> {
        None
    }
    pub fn os_summary() -> String {
        "Windows 10 Pro 22H2".into()
    }
}

pub mod onedrive {
    use super::*;
    use super::ops::{Log, Lvl};

    #[derive(Clone, Debug, Default)]
    pub struct KnownF {
        pub label: &'static str,
        pub path: String,
        pub in_od: bool,
        pub cloud: usize,
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
        pub fn blockers(&self) -> (Vec<String>, Vec<String>) {
            (vec![], vec![])
        }
    }

    pub fn fmt_n(n: usize) -> String {
        n.to_string()
    }

    pub fn fmt_bytes(b: u64) -> String {
        format!("{b} bytes")
    }

    pub fn removal_recorded() -> bool {
        false
    }

    pub fn scan(log: &mut Log, _expert: bool) -> Scan {
        log.w(Lvl::Info, "UI preview: OneDrive scan is not available on Linux.");
        log.done("Preview: scan skipped");
        Scan { at: "preview".into(), ..Default::default() }
    }

    pub fn open_onedrive(log: &mut Log) {
        log.w(Lvl::Info, "UI preview: Open OneDrive skipped.");
        log.done("Preview");
    }

    #[derive(Default, Clone, Copy)]
    pub struct Progress {
        pub total: usize,
        pub left: usize,
        pub active: bool,
    }

    pub fn download_all(log: &mut Log, _prog: Arc<Mutex<Progress>>, _cancel: Arc<AtomicBool>) {
        log.w(Lvl::Info, "UI preview: download skipped.");
        log.done("Preview");
    }
    pub fn move_back(log: &mut Log) {
        log.w(Lvl::Info, "UI preview: copy folders skipped.");
        log.done("Preview");
    }
    pub fn stop_sync(log: &mut Log, _restore_point: bool) {
        log.w(Lvl::Info, "UI preview: stop sync skipped.");
        log.done("Preview");
    }

    pub struct RemoveOpts {
        pub restore_point: bool,
        pub nav_pane: bool,
        pub new_users: bool,
    }

    pub fn remove(log: &mut Log, _o: RemoveOpts) {
        log.w(Lvl::Info, "UI preview: remove skipped.");
        log.done("Preview");
    }
    pub fn undo(log: &mut Log) {
        log.w(Lvl::Info, "UI preview: undo skipped.");
        log.done("Preview");
    }
    pub fn verify_status(log: &mut Log, _standalone: bool) {
        log.w(Lvl::Info, "UI preview: verify skipped.");
        log.done("Preview");
    }
    pub fn restart(log: &mut Log) {
        log.w(Lvl::Info, "UI preview: restart skipped.");
        log.done("Preview");
    }

    // Silence unused import in this stub.
    #[allow(dead_code)]
    fn _keep(p: &Path) -> bool {
        p.exists()
    }
}
