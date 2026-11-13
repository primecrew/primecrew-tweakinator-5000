// OneDrive removal wizard window (child of the gui module). Beginner mode by default; Expert mode is compact
// and technical. Every change still goes through a confirm dialog; the files gate stays in both modes.
use super::{App, Pending};
use crate::catalog::*;
use crate::help;
use crate::onedrive::{self as od, fmt_bytes, fmt_n, RemoveOpts, Scan};
use crate::ops::Lvl;
use crate::theme;
use eframe::egui::{self, RichText};
use std::sync::atomic::Ordering;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Welcome,
    Check,
    Stop,
    Remove,
    Done,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OdAct {
    Download,
    MoveBack,
    Stop,
    Remove,
    Undo,
    Restart,
    Override,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum After {
    Rescan,
    Done,
}

fn status(ui: &mut egui::Ui, l: Lvl, text: &str, tip: Option<&str>, topic: &'static str, expert: bool) {
    let k = theme::t();
    let (tag, c) = match l {
        Lvl::Pass => ("OK", k.ok),
        Lvl::Fail => ("NOT YET", k.err),
        Lvl::Warn => ("CHECK", k.warn),
        _ => ("INFO", k.info),
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("{tag:<7}")).monospace().strong().color(c));
        ui.add(egui::Label::new(RichText::new(text).color(if l == Lvl::Info { k.muted } else { ui.visuals().text_color() })).wrap());
        if let Some(t) = tip {
            help::mark(ui, t, topic, expert);
        }
    });
}

fn para(ui: &mut egui::Ui, t: &str) {
    ui.add(egui::Label::new(t).wrap());
}

fn steps_list(ui: &mut egui::Ui, items: &[&str]) {
    for (i, s) in items.iter().enumerate() {
        ui.add(egui::Label::new(format!("{}. {s}", i + 1)).wrap());
    }
}

impl App {
    fn od_scan_now(&self) -> Option<Scan> {
        self.od_scan.lock().ok().and_then(|g| g.clone())
    }

    pub(super) fn od_start_scan(&mut self, ctx: &egui::Context) {
        let slot = self.od_scan.clone();
        let expert = self.prefs.expert;
        self.start(ctx, "OneDriveCheck", move |log| {
            let s = od::scan(log, expert);
            if let Ok(mut g) = slot.lock() {
                *g = Some(s);
            }
        });
    }

    /// Hard blockers (wrong account) can never be overridden; file blockers can, with an explicit tick.
    fn od_gate(&self) -> bool {
        match self.od_scan_now() {
            Some(s) => {
                let (h, so) = s.blockers();
                h.is_empty() && (so.is_empty() || self.od_override)
            }
            None => false,
        }
    }

    fn od_can_change(&self) -> bool {
        self.admin && !self.busy() && self.confirm.is_none() && self.mismatch.is_none()
    }

    pub(super) fn od_after_job(&mut self, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        if let Some(a) = self.od_after.take() {
            if a == After::Done {
                self.od_step = Step::Done;
            }
            self.od_start_scan(ctx);
        }
    }

    pub(super) fn od_window(&mut self, ctx: &egui::Context) {
        if !self.od_open {
            return;
        }
        let mut open = true;
        let expert = self.prefs.expert;
        if expert && self.od_step == Step::Welcome {
            self.od_step = Step::Check;
        }
        egui::Window::new("OneDrive removal wizard")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(780.0)
            .default_height(600.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    self.od_header(ui);
                    match self.od_step {
                        Step::Welcome => self.od_welcome(ui),
                        Step::Check => self.od_check(ui, ctx),
                        Step::Stop => self.od_stop(ui, ctx),
                        Step::Remove => self.od_remove(ui),
                        Step::Done => self.od_done(ui),
                    }
                    if self.od_step != Step::Welcome {
                        ui.add_space(6.0);
                        ui.separator();
                        self.od_footer(ui, ctx);
                    }
                });
            });
        if !open {
            self.od_open = false;
            self.od_cancel.store(true, Ordering::Relaxed);
        }
    }

    fn od_header(&mut self, ui: &mut egui::Ui) {
        let k = theme::t();
        if let Some((me, s)) = &self.mismatch {
            theme::card(ui, "", |ui| {
                ui.add(egui::Label::new(RichText::new(format!("Wrong account. This app runs as {me}, but {s} is signed in. The wizard would look at the wrong person's OneDrive, so it is locked. Close the app and start it again while signed in as {s}, using an administrator account for {s}.")).strong().color(k.err)).wrap());
            });
        }
        let names = ["Check files", "Stop sync", "Remove"];
        let cur = match self.od_step {
            Step::Check => 0,
            Step::Stop => 1,
            Step::Remove => 2,
            _ => 9,
        };
        if cur < 9 {
            ui.horizontal_wrapped(|ui| {
                for (i, n) in names.iter().enumerate() {
                    let t = RichText::new(format!("{} {n}", i + 1));
                    ui.label(if i == cur { t.strong().color(k.info).underline() } else { t.color(k.muted) });
                    if i < 2 {
                        ui.label(RichText::new("›").color(k.muted));
                    }
                }
                if !self.prefs.expert {
                    ui.label(RichText::new(format!("   Step {} of 3", cur + 1)).color(k.muted));
                }
                let topic = match self.od_step {
                    Step::Check => help::ID_OD_CHECK,
                    Step::Stop => help::ID_OD_STOP,
                    Step::Remove | Step::Done => help::ID_OD_REMOVE,
                    Step::Welcome => help::ID_ONEDRIVE,
                };
                help::mark(ui, "Open the Help topic for this step.", topic, self.prefs.expert);
                help::link(ui, topic);
            });
            ui.add_space(4.0);
        }
    }

    fn od_welcome(&mut self, ui: &mut egui::Ui) {
        let k = theme::t();
        theme::card(ui, "Remove OneDrive from this PC", |ui| {
            ui.horizontal_wrapped(|ui| {
                help::mark(ui, "A guided, three-step removal. Your files are never deleted. Nothing changes until you confirm.", help::ID_ONEDRIVE, false);
                help::link(ui, help::ID_ONEDRIVE);
            });
            para(ui, "This wizard helps you stop using OneDrive and take it off this PC, one safe step at a time.");
            ui.add_space(4.0);
            steps_list(ui, &[
                "Check files: we make sure all your files are on this PC, not only in the cloud.",
                "Stop sync: you unlink your account and we stop OneDrive from starting.",
                "Remove: we uninstall OneDrive and stop it from coming back.",
            ]);
            ui.add_space(4.0);
            para(ui, "It usually takes 10 to 30 minutes. If many files need downloading, it can take longer.");
            para(ui, "You can stop at any time. Nothing changes until you press a button and say yes.");
            ui.add(egui::Label::new(RichText::new("Your files are never deleted. The OneDrive folder stays on your PC.").strong().color(k.ok)).wrap());
            ui.add(egui::Label::new(RichText::new("Before you start: make sure you know your Microsoft account password. You will need it to see your files on OneDrive.com later, or to use OneDrive again.").color(k.warn)).wrap());
            ui.add_space(8.0);
            if ui.add(theme::primary_button("Start")).clicked() {
                self.od_step = Step::Check;
            }
        });
    }

    fn od_check(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let k = theme::t();
        let expert = self.prefs.expert;
        let scan = self.od_scan_now();
        let busy = self.busy();
        let can = self.od_can_change();
        if !expert {
            theme::card(ui, "", |ui| {
                para(ui, "Step 1: make sure your files are on this PC.");
                para(ui, "Some files may only live in the cloud. Your Desktop or Documents may also be backed up to OneDrive. We check both. Checking changes nothing.");
            });
        }
        ui.horizontal_wrapped(|ui| {
            let r = ui.add_enabled(!busy && self.mismatch.is_none(), theme::primary_button(if scan.is_some() { "Check again" } else { "Check my files" }));
            if r.clicked() {
                self.od_start_scan(ctx);
            }
            help::mark(ui, "Looks through your OneDrive folder and your Desktop, Documents and Pictures. It only reads; nothing is changed or downloaded.", help::ID_OD_CHECK, expert);
            if let Some(s) = &scan {
                ui.label(RichText::new(format!("last checked {}", s.at)).color(k.muted));
            }
        });
        let prog = *self.od_prog.lock().unwrap();
        if prog.active {
            theme::card(ui, "", |ui| {
                let done = prog.total.saturating_sub(prog.left);
                ui.label(RichText::new(format!("Downloading {} of {} files…", fmt_n(done), fmt_n(prog.total))).strong());
                ui.add(egui::ProgressBar::new(if prog.total == 0 { 1.0 } else { done as f32 / prog.total as f32 }).show_percentage());
                para(ui, "You can leave this running. OneDrive does the downloading; this just keeps count every 10 seconds.");
                if ui.button("Stop watching").clicked() {
                    self.od_cancel.store(true, Ordering::Relaxed);
                }
            });
        }
        let Some(s) = scan else {
            if busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Checking…");
                });
            }
            return;
        };
        theme::card(ui, "What we found", |ui| {
            if s.mismatch.is_some() {
                status(ui, Lvl::Fail, "Wrong Windows account (see the red note above).", Some("The person signed in to Windows must also be the one running this app, or we'd check someone else's files."), help::ID_OD_CHECK, expert);
            }
            if s.cloud_files == 0 && s.cloud_dirs == 0 {
                status(ui, Lvl::Pass, &format!("All {} files in OneDrive are on this PC.", fmt_n(s.files)), Some("Good. Every file has a real copy on this computer."), help::ID_OD_CHECK, expert);
            } else {
                let t = format!("{} files ({}){} only live in the cloud.", fmt_n(s.cloud_files), fmt_bytes(s.cloud_bytes), if s.cloud_dirs > 0 { format!(" and {} folders", fmt_n(s.cloud_dirs)) } else { String::new() });
                status(ui, Lvl::Fail, &t, Some("These are in your OneDrive online but not stored on this PC yet. Click 'Download all files to this PC' and wait until this says everything is on this PC."), help::ID_OD_CHECK, expert);
            }
            for kf in &s.known {
                if kf.in_od {
                    let t = if expert { format!("{} -> {} (inside OneDrive; {} cloud-only)", kf.label, kf.path, fmt_n(kf.cloud)) } else { format!("Your {} is being backed up to OneDrive.", kf.label) };
                    status(ui, Lvl::Fail, &t, Some("Turn off backup for this folder in OneDrive (steps below) and choose to keep the files on this PC. Or use 'Copy my folders back'."), help::ID_OD_CHECK, expert);
                } else {
                    let t = if expert { format!("{} -> {}", kf.label, kf.path) } else { format!("Your {} is on this PC.", kf.label) };
                    status(ui, Lvl::Pass, &t, Some("This folder is a normal folder on this computer."), help::ID_OD_CHECK, expert);
                }
            }
            for a in &s.accounts {
                status(ui, Lvl::Warn, &format!("Signed in to OneDrive: {} {}", a.kind, a.email), Some("You'll unlink this in Step 2. Unlinking keeps your files."), help::ID_OD_CHECK, expert);
            }
            if s.url_files > 0 {
                status(ui, Lvl::Warn, &format!("{} web-link files found. OneNote notebooks look like this and live only online.", fmt_n(s.url_files)), Some("If you use OneNote, open each notebook in OneNote and use File > Export to save a copy on this PC."), help::ID_OD_CHECK, expert);
            }
            if s.errors > 0 {
                status(ui, Lvl::Warn, &format!("{} items could not be read.", fmt_n(s.errors)), Some("Usually a file in use or a folder you don't have permission for. Close other apps and check again."), help::ID_OD_CHECK, expert);
            }
            status(ui, Lvl::Info, &format!("OneDrive app: {}{}", if s.installed { "installed" } else { "not installed" }, if s.running { ", running" } else { "" }), None, help::ID_OD_CHECK, expert);
            if expert {
                for r in &s.roots {
                    status(ui, Lvl::Info, &format!("root {}", r.display()), None, help::ID_OD_CHECK, expert);
                }
                for w in &s.install_where {
                    status(ui, Lvl::Info, &format!("install {w}"), None, help::ID_OD_CHECK, expert);
                }
                status(ui, Lvl::Info, &format!("{} files, {} total", fmt_n(s.files), fmt_bytes(s.bytes)), None, help::ID_OD_CHECK, expert);
            }
        });

        theme::card(ui, "Help me get my files onto this PC", |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(can && s.installed, egui::Button::new("Open OneDrive")).on_hover_text("Starts OneDrive (as you, not as admin). Then click the cloud icon near the clock > gear > Settings.").clicked() {
                    self.start(ctx, "OpenOneDrive", od::open_onedrive);
                }
                if ui.add_enabled(!s.roots.is_empty(), egui::Button::new("Open OneDrive folder")).on_hover_text("Shows your OneDrive folder in File Explorer.").clicked() {
                    for r in &s.roots {
                        let _ = std::process::Command::new("explorer.exe").arg(r).spawn();
                    }
                }
                let need_dl = s.cloud_files > 0 || s.cloud_dirs > 0;
                if ui.add_enabled(can && need_dl && !prog.active, theme::primary_button("Download all files to this PC…")).on_hover_text(if expert { "attrib +P -U /S /D on each OneDrive root (Always keep on this device); then polls the cloud-only count." } else { "Asks OneDrive to keep a copy of every file on this PC. OneDrive must be running and you need internet." }).clicked() {
                    self.confirm = Some(Pending::Od(OdAct::Download));
                }
                let need_mv = s.known.iter().any(|k| k.in_od);
                if ui.add_enabled(can && need_mv, egui::Button::new("Copy my folders back…")).on_hover_text(if expert { "Copies OneDrive\\<Folder> to %USERPROFILE%\\<Folder> (skip existing names), then SHSetKnownFolderPath. Never moves or deletes." } else { "Copies your Desktop / Documents / Pictures out of OneDrive into normal folders on this PC. Nothing is deleted." }).clicked() {
                    self.confirm = Some(Pending::Od(OdAct::MoveBack));
                }
            });
            if !expert && s.known.iter().any(|k| k.in_od) {
                ui.add_space(4.0);
                ui.label(RichText::new("Best way: turn off folder backup in OneDrive").strong());
                steps_list(ui, &[
                    "Click the OneDrive cloud icon near the clock (bottom right).",
                    "Click the gear, then Settings.",
                    "Open 'Sync and backup' and click 'Manage backup'.",
                    "Switch off Desktop, Documents and Pictures. When asked, choose to keep the files on this PC.",
                    "Come back here and click 'Check again'.",
                ]);
                para(ui, "If OneDrive says some files couldn't be moved, click 'Download all files to this PC' first, wait, then try again.");
            }
        });

        let (hard, soft) = s.blockers();
        theme::card(ui, "", |ui| {
            if hard.is_empty() && soft.is_empty() {
                ui.add(egui::Label::new(RichText::new("Your files are safe on this PC. You can go on.").strong().color(k.ok)).wrap());
            } else {
                ui.label(RichText::new("Not ready yet:").strong().color(k.err));
                for b in hard.iter().chain(soft.iter()) {
                    ui.add(egui::Label::new(RichText::new(format!("• {b}")).color(k.err)).wrap());
                }
                if hard.is_empty() {
                    if expert {
                        ui.checkbox(&mut self.od_override, RichText::new("I've checked, my files are safe (skip the files check)").color(k.warn));
                    } else if self.od_override {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new("You chose to skip the files check.").color(k.warn));
                            if ui.small_button("Undo that").clicked() {
                                self.od_override = false;
                            }
                        });
                    } else if ui.add(egui::Button::new(RichText::new("My files are safe another way…").small().color(k.warn)).frame(false)).clicked() {
                        self.confirm = Some(Pending::Od(OdAct::Override));
                    }
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            if !expert && ui.button("Back").clicked() {
                self.od_step = Step::Welcome;
            }
            if ui.add_enabled(self.od_gate(), theme::primary_button("Next: Stop sync")).clicked() {
                self.od_step = Step::Stop;
            }
        });
    }

    fn od_stop(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let k = theme::t();
        let expert = self.prefs.expert;
        let can = self.od_can_change() && self.od_gate();
        let scan = self.od_scan_now().unwrap_or_default();
        if !expert {
            theme::card(ui, "", |ui| {
                para(ui, "Step 2: stop OneDrive.");
                para(ui, "First you unlink this PC from your OneDrive account. Then we close OneDrive and stop it from starting by itself. Your files stay exactly where they are.");
            });
        }
        theme::card(ui, "Unlink this PC (you do this in OneDrive)", |ui| {
            steps_list(ui, &[
                "Click the OneDrive cloud icon near the clock (or press 'Open OneDrive').",
                "Click the gear, then Settings.",
                "Open the 'Account' tab and click 'Unlink this PC', then 'Unlink account'.",
                "Close the OneDrive sign-in window that appears. Don't sign in again.",
            ]);
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(self.od_can_change() && scan.installed, egui::Button::new("Open OneDrive")).clicked() {
                    self.start(ctx, "OpenOneDrive", od::open_onedrive);
                }
                if ui.add_enabled(!self.busy() && self.mismatch.is_none(), egui::Button::new("Check again")).clicked() {
                    self.od_start_scan(ctx);
                }
                help::mark(ui, "Unlinking only disconnects this PC. Your files stay on this PC and online at OneDrive.com.", help::ID_OD_STOP, expert);
            });
            if scan.accounts.is_empty() {
                status(ui, Lvl::Pass, "No account is linked any more.", None, help::ID_OD_STOP, expert);
            } else {
                for a in &scan.accounts {
                    status(ui, Lvl::Warn, &format!("Still linked: {} {}", a.kind, a.email), Some("Follow the steps above, then click Check again. You can also go on; removing OneDrive works either way."), help::ID_OD_STOP, expert);
                }
            }
        });
        theme::card(ui, "Stop OneDrive", |ui| {
            if expert {
                para(ui, &format!("taskkill OneDrive.exe; delete {OD_RUN_CU}\\OneDrive (recorded); disable Get-ScheduledTask 'OneDrive*' (recorded)."));
            } else {
                para(ui, "Closes OneDrive and stops it starting when you sign in. Undo can turn it back on.");
            }
            ui.checkbox(&mut self.opts.restore_point, "Create a restore point first (recommended)");
            if !expert {
                ui.label(RichText::new("A restore point lets you roll Windows settings back if something goes wrong. It doesn't touch your files.").color(k.muted));
            }
            if ui.add_enabled(can, theme::primary_button("Stop OneDrive now…")).clicked() {
                self.confirm = Some(Pending::Od(OdAct::Stop));
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Back").clicked() {
                self.od_step = Step::Check;
            }
            if ui.add_enabled(self.od_gate(), theme::primary_button("Next: Remove")).clicked() {
                self.od_step = Step::Remove;
            }
        });
    }

    pub(super) fn od_remove_lines(&self) -> Vec<String> {
        let e = self.prefs.expert;
        let mut v = vec![];
        if self.opts.restore_point {
            v.push("Create a System Restore point first.".into());
        }
        v.push(if e { format!("Close OneDrive.exe; delete {OD_RUN_CU}\\OneDrive; disable OneDrive* tasks") } else { "Close OneDrive and stop it starting by itself.".into() });
        v.push(if e { "Uninstall: UninstallString (HKCU / HKLM), else %LOCALAPPDATA%\\Microsoft\\OneDrive\\<ver>\\OneDriveSetup.exe /uninstall, Program Files … /uninstall /allusers, %SystemRoot%\\System32|SysWOW64\\OneDriveSetup.exe /uninstall, then winget".into() } else { "Uninstall the OneDrive app.".into() });
        v.push(if e { format!("{OD_WINPOL}\\DisableFileSyncNGSC=1; {OD_POL_CU}\\DisablePersonalSync=1; {OD_POL_LM}\\KFMBlockOptIn=1, PreventNetworkTrafficPreUserSignIn=1") } else { "Turn on Windows settings that stop OneDrive syncing or backing up your folders again.".into() });
        v.push(if e { format!("delete {OD_RUN_CU}\\OneDriveSetup if present") } else { "Stop OneDrive setting itself up again when you sign in.".into() });
        if self.od_new_users {
            v.push(if e { "Default user hive (C:\\Users\\Default\\NTUSER.DAT) Run\\OneDriveSetup -> removed".into() } else { "Don't install OneDrive for new user accounts on this PC.".into() });
        }
        if self.od_nav {
            v.push(if e { format!("{OD_NAV_CLSID}\\{OD_NAV_VALUE}=0 (HKCU/HKLM, 64/32-bit, where present)") } else { "Hide OneDrive from the File Explorer side bar.".into() });
        }
        v.push("Your OneDrive folder and the files in it are NOT deleted.".into());
        v.push("Every change is saved so Undo can put it back.".into());
        v
    }

    fn od_remove(&mut self, ui: &mut egui::Ui) {
        let k = theme::t();
        let expert = self.prefs.expert;
        if !expert {
            theme::card(ui, "", |ui| {
                para(ui, "Step 3: remove OneDrive.");
                para(ui, "We uninstall the OneDrive app and switch on Windows settings that stop it syncing or coming back.");
                ui.add(egui::Label::new(RichText::new("Your files in the OneDrive folder are NOT deleted.").strong().color(k.ok)).wrap());
            });
        }
        theme::card(ui, "Options", |ui| {
            ui.checkbox(&mut self.opts.restore_point, "Create a restore point first (recommended)");
            if !expert {
                ui.label(RichText::new("A restore point lets you roll Windows settings back if something goes wrong. It doesn't touch your files.").color(k.muted));
            }
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut self.od_nav, "Hide OneDrive from the File Explorer side bar");
                help::mark(ui, "Removes the OneDrive entry on the left side of File Explorer. Undo puts it back.", help::ID_OD_REMOVE, expert);
            });
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut self.od_new_users, "Don't install OneDrive for new user accounts on this PC");
                help::mark(ui, "Windows normally installs OneDrive for every new account. This stops that. Undo puts it back.", help::ID_OD_REMOVE, expert);
            });
        });
        theme::card(ui, "What will happen", |ui| {
            for l in self.od_remove_lines() {
                ui.add(egui::Label::new(RichText::new(format!("• {l}")).color(k.muted)).wrap());
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Back").clicked() {
                self.od_step = Step::Stop;
            }
            if ui.add_enabled(self.od_can_change() && self.od_gate(), theme::danger_button("Remove OneDrive…")).clicked() {
                self.od_ack = false;
                self.confirm = Some(Pending::Od(OdAct::Remove));
            }
        });
    }

    fn od_done(&mut self, ui: &mut egui::Ui) {
        let k = theme::t();
        let s = self.od_scan_now().unwrap_or_default();
        theme::card(ui, "OneDrive is removed", |ui| {
            ui.add(egui::Label::new(RichText::new(if s.installed { "Some parts could not be removed. See the log below; the blocks still stop OneDrive syncing." } else { "Done. OneDrive is no longer installed on this PC." }).strong().color(if s.installed { k.warn } else { k.ok })).wrap());
            ui.add_space(4.0);
            ui.label(RichText::new("What changed").strong());
            for l in self.od_remove_lines().iter().filter(|l| !l.starts_with("Your OneDrive folder") && !l.starts_with("Every change")) {
                ui.add(egui::Label::new(format!("• {l}")).wrap());
            }
            ui.add_space(4.0);
            ui.label(RichText::new("Where your files are").strong());
            if s.roots.is_empty() {
                para(ui, "In your normal folders (Desktop, Documents, Pictures).");
            }
            for r in &s.roots {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("• {}", r.display()));
                    if ui.small_button("Open").clicked() {
                        let _ = std::process::Command::new("explorer.exe").arg(r).spawn();
                    }
                });
            }
            para(ui, "That folder is now an ordinary folder. You can delete it yourself later, once you're sure you have everything.");
            ui.add_space(4.0);
            ui.label(RichText::new("Please restart the PC to finish.").strong().color(k.warn));
            ui.add_space(4.0);
            ui.label(RichText::new("How to undo").strong());
            para(ui, "Open this wizard and click 'Undo OneDrive removal'. To use OneDrive again later, install it from Microsoft (microsoft.com/onedrive/download, or 'winget install Microsoft.OneDrive') and sign in.");
            ui.add_space(6.0);
            if ui.add_enabled(self.od_can_change(), theme::primary_button("Restart now…")).clicked() {
                self.confirm = Some(Pending::Od(OdAct::Restart));
            }
        });
    }

    fn od_footer(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let expert = self.prefs.expert;
        let can = self.od_can_change();
        let has_state = od::removal_recorded() || crate::ops::load_state().values().any(|e| e["Category"].as_str() == Some("OneDrive"));
        ui.horizontal_wrapped(|ui| {
            let k = theme::t();
            help::mark(ui, "Read about this wizard in Help.", help::ID_ONEDRIVE, expert);
            help::link(ui, help::ID_ONEDRIVE);
            if ui.add_enabled(!self.busy(), egui::Button::new(RichText::new("Verify (read-only)").color(k.info))).on_hover_text("Shows the OneDrive state in the log. Changes nothing.").clicked() {
                self.start(ctx, "OneDriveVerify", |log| od::verify_status(log, true));
            }
            if ui.add_enabled(can && has_state, theme::danger_button("Undo OneDrive removal…")).on_hover_text("Puts back everything this wizard changed (settings, startup, tasks). It does not reinstall OneDrive.").clicked() {
                self.confirm = Some(Pending::Od(OdAct::Undo));
            }
            if expert && ui.button("Copy Remove-OneDrive.ps1").clicked() {
                let t = crate::psgen::KIT.iter().find(|(n, _)| n.ends_with("Remove-OneDrive.ps1")).map(|(_, t)| *t).unwrap_or("");
                ctx.copy_text(t.to_string());
                self.push(Lvl::Info, "Copied scripts\\Remove-OneDrive.ps1 (needs scripts\\TweakCommon.ps1 next to it: use Export PS kit).");
            }
        });
    }

    /// Confirm dialogs for the wizard. Destructive steps always confirm, in both modes.
    pub(super) fn od_confirm(&mut self, ctx: &egui::Context, act: OdAct) {
        let k = theme::t();
        let expert = self.prefs.expert;
        let s = self.od_scan_now().unwrap_or_default();
        let (title, lines, btn, danger): (&str, Vec<String>, &str, bool) = match act {
            OdAct::Download => (
                "Download all files to this PC",
                {
                    let mut v = vec![
                        "OneDrive will keep a copy of every file on this PC ('Always keep on this device').".to_string(),
                        format!("About {} will be downloaded. You need internet and that much free disk space.", fmt_bytes(s.cloud_bytes)),
                        "OneDrive must stay running. You can keep using the PC meanwhile.".into(),
                    ];
                    for r in &s.roots {
                        v.push(if expert { format!("attrib +P -U \"{0}\" && attrib +P -U /S /D \"{0}\\*\"", r.display()) } else { format!("Folder: {}", r.display()) });
                    }
                    v
                },
                "Start downloading",
                false,
            ),
            OdAct::MoveBack => (
                "Copy my folders back to this PC",
                {
                    let home = std::env::var("USERPROFILE").unwrap_or_default();
                    let mut v = vec!["OneDrive is closed first.".to_string()];
                    for kf in s.known.iter().filter(|k| k.in_od) {
                        v.push(format!("{}: copy files from {} to {}\\{}, then use that as your {} folder.", kf.label, kf.path, home, kf.label, kf.label));
                    }
                    v.push("Files with the same name already there are skipped, not overwritten.".into());
                    v.push("Nothing is moved or deleted from OneDrive.".into());
                    v.push("Folders that still have cloud-only files are skipped. Download those first.".into());
                    v
                },
                "Copy my folders",
                false,
            ),
            OdAct::Stop => (
                "Stop OneDrive",
                vec![
                    if self.opts.restore_point { "Create a System Restore point first.".to_string() } else { "No restore point (the box is off).".into() },
                    "Close OneDrive now.".into(),
                    if expert { format!("Delete {OD_RUN_CU}\\OneDrive (saved for Undo).") } else { "Stop OneDrive starting when you sign in (Undo can turn it back on).".into() },
                    if expert { "Disable scheduled tasks matching 'OneDrive*' (saved for Undo).".into() } else { "Turn off OneDrive's background update tasks (Undo can turn them back on).".into() },
                    "Your files are not touched.".into(),
                ],
                "Stop OneDrive",
                false,
            ),
            OdAct::Remove => ("Remove OneDrive from this PC", self.od_remove_lines(), "Remove OneDrive", true),
            OdAct::Undo => (
                "Undo OneDrive removal",
                vec![
                    "Turn the OneDrive blocking settings back to how they were.".into(),
                    "Put back OneDrive starting at sign-in and its tasks (if OneDrive is installed).".into(),
                    "Put back OneDrive setup for new accounts, and the File Explorer side bar entry.".into(),
                    "Your folders stay where they are now. OneDrive itself is not reinstalled; the log says how.".into(),
                ],
                "Undo",
                true,
            ),
            OdAct::Restart => ("Restart the PC", vec!["The PC restarts in 20 seconds. Save your work first.".into(), "To cancel, run: shutdown /a".into()], "Restart", true),
            OdAct::Override => (
                "Skip the files check?",
                vec![
                    "The check says some files are not on this PC yet, or a folder is still in OneDrive.".into(),
                    "If you go on anyway, files that live only in the cloud will NOT be on this PC after OneDrive is removed.".into(),
                    "They stay safe online at OneDrive.com, but you'd need your Microsoft account to get them.".into(),
                    "Only do this if you know your files are safe somewhere else.".into(),
                ],
                "Skip the check",
                true,
            ),
        };
        let needs_tick = matches!(act, OdAct::Remove | OdAct::Override);
        let mut go = false;
        let mut close = false;
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(640.0)
            .show(ctx, |ui| {
                if act == OdAct::Remove {
                    theme::card(ui, "", |ui| {
                        ui.add(egui::Label::new(RichText::new("Make sure your files are safe.").heading().strong().color(k.warn)).wrap());
                        ui.add(egui::Label::new(RichText::new("This removes OneDrive from this PC. Your files in the OneDrive folder are NOT deleted.").strong()).wrap());
                    });
                }
                theme::card(ui, "", |ui| {
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for l in &lines {
                            ui.add(egui::Label::new(if expert { RichText::new(format!("• {l}")).monospace() } else { RichText::new(format!("• {l}")) }).wrap());
                        }
                    });
                });
                let mut ok = true;
                if needs_tick {
                    let label = if act == OdAct::Remove { "I understand. My files are safe." } else { "I understand the risk. My files are safe." };
                    ui.checkbox(&mut self.od_ack, RichText::new(label).strong());
                    ok = self.od_ack;
                }
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    let b = if danger { theme::danger_button(btn) } else { theme::primary_button(btn) };
                    if ui.add_enabled(ok, b).clicked() {
                        go = true;
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(90.0, 30.0))).clicked() {
                        close = true;
                    }
                });
            });
        if go {
            close = true;
            let rp = self.opts.restore_point;
            match act {
                OdAct::Download => {
                    self.od_cancel.store(false, Ordering::Relaxed);
                    let (p, c) = (self.od_prog.clone(), self.od_cancel.clone());
                    self.od_after = Some(After::Rescan);
                    self.start(ctx, "OneDriveDownload", move |log| od::download_all(log, p, c));
                }
                OdAct::MoveBack => {
                    self.od_after = Some(After::Rescan);
                    self.start(ctx, "OneDriveMoveBack", od::move_back);
                }
                OdAct::Stop => {
                    self.od_after = Some(After::Rescan);
                    self.start(ctx, "OneDriveStopSync", move |log| od::stop_sync(log, rp));
                }
                OdAct::Remove => {
                    let o = RemoveOpts { restore_point: rp, nav_pane: self.od_nav, new_users: self.od_new_users };
                    self.od_after = Some(After::Done);
                    self.start(ctx, "OneDriveRemove", move |log| od::remove(log, o));
                }
                OdAct::Undo => {
                    self.od_after = Some(After::Rescan);
                    self.start(ctx, "OneDriveUndo", od::undo);
                }
                OdAct::Restart => self.start(ctx, "Restart", od::restart),
                OdAct::Override => {
                    self.od_override = true;
                    self.push(Lvl::Warn, "Files check skipped by the user (\"my files are safe\").");
                }
            }
        }
        if close {
            self.confirm = None;
            self.od_ack = false;
        }
    }
}
