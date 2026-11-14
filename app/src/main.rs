//! PrimeCrew Tweakinator 5000 (Windows 10 tweaks) - portable, offline (no network code at all), admin-only GUI.
//! The UI also builds on Linux (with stubs) so screenshots can be taken under Xvfb.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod catalog;
mod help;
mod psgen;
mod theme;

#[cfg(windows)]
mod onedrive;
#[cfg(windows)]
mod ops;
#[cfg(windows)]
mod sys;

#[cfg(not(windows))]
mod linux_stub;
#[cfg(not(windows))]
mod onedrive {
    pub use crate::linux_stub::onedrive::*;
}
#[cfg(not(windows))]
mod ops {
    pub use crate::linux_stub::ops::*;
}
#[cfg(not(windows))]
mod sys {
    pub use crate::linux_stub::sys::*;
}

fn screenshot_dir() -> Option<std::path::PathBuf> {
    let mut args = std::env::args();
    while let Some(a) = args.next() {
        if a == "--screenshot" {
            return args.next().map(std::path::PathBuf::from);
        }
    }
    None
}

fn main() -> eframe::Result<()> {
    #[cfg(windows)]
    {
        // Headless one-shot used by the SYSTEM task Win10Tweaks-RestoreWUKill: re-apply the Updates kill, no window.
        if std::env::args().any(|a| a == catalog::RESTORE_FLAG) {
            if sys::is_admin() {
                ops::headless_restore();
            }
            return Ok(());
        }
    }
    let shot = screenshot_dir();
    let admin = sys::is_admin();
    // The manifest already requests admin; this is the runtime fallback (e.g. manifest stripped).
    #[cfg(windows)]
    {
        if !admin && std::env::args().all(|a| a != "--no-elevate") && sys::relaunch_elevated() {
            return Ok(());
        }
    }
    let title = gui::APP_TITLE;
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(title)
            .with_icon(std::sync::Arc::new(eframe::egui::IconData { rgba: gui::ICON_RGBA.to_vec(), width: 128, height: 128 }))
            .with_inner_size([1100.0, 800.0])
            .with_min_inner_size([820.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(title, options, Box::new(move |cc| Ok(Box::new(gui::App::new(cc, admin, shot)))))
}

mod gui {

    use crate::catalog::*;
    use crate::ops::{self, ApplySummary, Log, Lvl, Msg, Opts, LOG_LEGEND};
    use crate::help;
    use crate::theme::{self, Look, Prefs};
    use crate::{psgen, sys};
    use eframe::egui::{self, RichText};
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc::{channel, Receiver};
    use std::sync::{Arc, Mutex};

    mod wizard {
        include!("wizard.rs");
    }
    use wizard::{After, OdAct, Step};

    pub const APP_NAME: &str = "PrimeCrew Tweakinator 5000";
    pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
    pub const APP_TITLE: &str = concat!("PrimeCrew Tweakinator 5000 v", env!("CARGO_PKG_VERSION"));
    /// 128x128 RGBA (generated from assets/icon_src.png); embedded, never fetched at runtime.
    pub const ICON_RGBA: &[u8] = include_bytes!("../assets/icon_128.rgba");

    enum Pending {
        Apply { cats: Vec<Cat>, all: bool },
        Revert { cats: Vec<Cat> },
        TempAllow,
        Od(OdAct),
    }

    /// Duration presets for "Temporarily allow updates" (minutes; 0 = until Restore is clicked).
    const TEMP_PRESETS: [u32; 4] = [0, 30, 60, 120];

    fn preset_label(m: u32) -> String {
        if m == 0 { "until I click Restore".into() } else { format!("{m} minutes") }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum ShotPhase {
        Main,
        Help,
        Tip,
        About,
        Preflight,
        Summary,
        Done,
    }

    struct Shot {
        dir: std::path::PathBuf,
        phase: ShotPhase,
        frames: u32,
        waiting: bool,
    }

    pub struct App {
        admin: bool,
        os: String,
        sel: [bool; 7],
        opts: Opts,
        log: Vec<(Lvl, String, Option<String>)>,
        rx: Option<Receiver<Msg>>,
        status: String,
        confirm: Option<Pending>,
        preflight: sys::Preflight,
        summary: Option<ApplySummary>,
        ack_updates: bool,
        ack_all: bool,
        prefs: Prefs,
        prefs_path: std::path::PathBuf,
        wheel: f32,
        temp: Option<ops::TempAllow>,
        temp_minutes: u32,
        icon: egui::TextureHandle,
        mismatch: Option<(String, String)>,
        od_open: bool,
        od_step: Step,
        od_scan: Arc<Mutex<Option<crate::onedrive::Scan>>>,
        od_prog: Arc<Mutex<crate::onedrive::Progress>>,
        od_cancel: Arc<AtomicBool>,
        od_override: bool,
        od_ack: bool,
        od_nav: bool,
        od_new_users: bool,
        od_after: Option<After>,
        help_open: bool,
        help_topic: &'static str,
        about_open: bool,
        shot_tip: bool,
        shot: Option<Shot>,
    }

    const SHORT_NOTE: &str = "Blocking Windows Update also blocks security / ESU patches";

    fn lvl_color(l: Lvl) -> egui::Color32 {
        let k = theme::t();
        match l {
            Lvl::Set | Lvl::Pass => k.ok,
            Lvl::Skip => k.muted,
            Lvl::Warn => k.warn,
            Lvl::Error | Lvl::Fail => k.err,
            Lvl::Info => k.info,
        }
    }

    impl App {
        pub fn new(cc: &eframe::CreationContext<'_>, admin: bool, shot_dir: Option<std::path::PathBuf>) -> Self {
            let prefs_path = ops::root_dir().join("ui.json");
            let prefs = Prefs::load(&prefs_path);
            theme::install_fonts(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx, &prefs);
            let mut a = App {
                admin,
                os: sys::os_summary(),
                sel: [true; 7],
                opts: Opts { restore_point: true, ..Default::default() },
                log: vec![],
                rx: None,
                status: "Idle".into(),
                confirm: None,
                preflight: sys::preflight(),
                summary: None,
                ack_updates: false,
                ack_all: false,
                prefs,
                prefs_path,
                wheel: 0.0,
                temp: ops::temp_allow_state(),
                temp_minutes: 0,
                icon: cc.egui_ctx.load_texture("app-icon", egui::ColorImage::from_rgba_unmultiplied([128, 128], ICON_RGBA), egui::TextureOptions::LINEAR),
                mismatch: sys::user_mismatch(),
                od_open: false,
                od_step: Step::Welcome,
                od_scan: Arc::new(Mutex::new(None)),
                od_prog: Arc::new(Mutex::new(Default::default())),
                od_cancel: Arc::new(AtomicBool::new(false)),
                od_override: false,
                od_ack: false,
                od_nav: true,
                od_new_users: true,
                od_after: None,
                help_open: false,
                help_topic: help::ID_START,
                about_open: false,
                shot_tip: false,
                shot: shot_dir.map(|dir| Shot { dir, phase: ShotPhase::Main, frames: 0, waiting: false }),
            };
            a.sel[0] = false; // cleanup is opt-in
            a.push(Lvl::Info, format!("OS: {}", a.os));
            a.push(Lvl::Info, format!("Elevated: {}. State/logs: {}", admin, ops::root_dir().display()));
            a.push(Lvl::Info, "Nothing changes until you confirm. Verify is read-only. This app makes no network connections.");
            if let Some((me, who)) = a.mismatch.clone() {
                a.push(Lvl::Warn, format!("This app runs as {me}, but {who} is signed in to Windows. Per-user tweaks (HKCU, Desktop) and the OneDrive wizard would affect {me}, not {who}. The OneDrive wizard is locked; sign in as {who} with an admin account to use it."));
            }
            if !admin {
                a.push(Lvl::Error, "Not running as Administrator: Apply/Revert disabled. Right-click the exe > Run as administrator.");
            }
            if let Some(t) = &a.temp {
                a.push(Lvl::Warn, format!("UPDATES TEMPORARILY ALLOWED since {}. Click Restore update kill when done.", t.since));
            }
            a
        }

        fn push(&mut self, l: Lvl, s: impl Into<String>) {
            self.log.push((l, s.into(), None));
        }

        fn busy(&self) -> bool {
            self.rx.is_some()
        }

        fn selected(&self) -> Vec<Cat> {
            Cat::ALL.iter().zip(self.sel.iter()).filter(|(_, s)| **s).map(|(c, _)| *c).collect()
        }

        fn set_prefs(&mut self, ctx: &egui::Context, p: Prefs) {
            if p != self.prefs {
                self.prefs = p;
                theme::apply(ctx, &self.prefs);
                self.prefs.save(&self.prefs_path);
            }
        }

        fn shortcuts(&mut self, ctx: &egui::Context) {
            let mut p = self.prefs;
            ctx.input(|i| {
                if i.key_pressed(egui::Key::F1) {
                    // handled below; flag via help::open so we don't need a second borrow
                    help::open(help::ID_START);
                }
                if i.modifiers.command {
                    if i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals) {
                        p.scale = theme::step(p.scale, true);
                    }
                    if i.key_pressed(egui::Key::Minus) {
                        p.scale = theme::step(p.scale, false);
                    }
                    if i.key_pressed(egui::Key::Num0) {
                        p.scale = 1.0;
                    }
                }
            });
            let zd = ctx.input(|i| i.zoom_delta());
            if (zd - 1.0).abs() > f32::EPSILON {
                self.wheel += zd.ln();
                if self.wheel.abs() >= 0.09 {
                    p.scale = theme::step(p.scale, self.wheel > 0.0);
                    self.wheel = 0.0;
                }
            }
            self.set_prefs(ctx, p);
        }

        fn open_logs(&self) {
            let d = ops::root_dir().join("Logs");
            let _ = std::fs::create_dir_all(&d);
            #[cfg(windows)]
            let _ = std::process::Command::new("explorer.exe").arg(&d).spawn();
            #[cfg(not(windows))]
            let _ = std::process::Command::new("xdg-open").arg(&d).spawn();
        }

        fn copy_powershell(&mut self, ctx: &egui::Context) {
            let s = psgen::generate(&self.selected(), self.opts);
            let n = s.lines().count();
            ctx.copy_text(s);
            self.push(Lvl::Info, format!("Copied generated PowerShell ({n} lines) for: {}. Paste into an elevated PowerShell.", self.selected().iter().map(|c| c.id()).collect::<Vec<_>>().join(", ")));
        }

        fn menu_bar(&mut self, ctx: &egui::Context) {
            let k = theme::t();
            egui::TopBottomPanel::top("menu")
                .frame(egui::Frame::new().fill(k.bar_fill).inner_margin(egui::Margin::symmetric(8, 2)))
                .show(ctx, |ui| {
                    egui::menu::bar(ui, |ui| {
                        ui.menu_button("File", |ui| {
                            if ui.button("Copy PowerShell script").clicked() {
                                self.copy_powershell(ctx);
                                ui.close_menu();
                            }
                            if ui.button("Open logs folder").clicked() {
                                self.open_logs();
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("Exit").clicked() {
                                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                                ui.close_menu();
                            }
                        });
                        ui.menu_button("View", |ui| {
                            ui.menu_button("Theme", |ui| {
                                let mut p = self.prefs;
                                for l in Look::ALL {
                                    if ui.selectable_label(p.look == l, l.label()).clicked() {
                                        p.look = l;
                                        self.set_prefs(ctx, p);
                                        ui.close_menu();
                                    }
                                }
                            });
                        });
                        ui.menu_button("Options", |ui| {
                            let mut p = self.prefs;
                            if ui.checkbox(&mut p.expert, "Expert mode").changed() {
                                self.set_prefs(ctx, p);
                            }
                            ui.label(egui::RichText::new("Off by default. Safety checks and confirmations stay on.").small().color(k.muted));
                        });
                        ui.menu_button("Help", |ui| {
                            if ui.button("Help contents\tF1").clicked() {
                                self.help_topic = help::ID_START;
                                self.help_open = true;
                                ui.close_menu();
                            }
                            if ui.button("About PrimeCrew Tweakinator 5000…").clicked() {
                                self.about_open = true;
                                ui.close_menu();
                            }
                        });
                    });
                });
        }

        fn draw_shot_tip(&self, ctx: &egui::Context) {
            if !self.shot_tip {
                return;
            }
            let k = theme::t();
            // A tooltip-style bubble used only for the v1.3.0 screenshot of a circled-? tip.
            egui::Area::new(egui::Id::new("shot-tip"))
                .fixed_pos(egui::pos2(255.0, 168.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style())
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            ui.set_max_width(320.0);
                            ui.add(egui::Label::new(egui::RichText::new(help::tip_for(Cat::Updates)).color(k.info)).wrap());
                        });
                });
        }

        fn tick_shot(&mut self, ctx: &egui::Context) {
            let Some(shot) = self.shot.as_mut() else { return };
            if shot.phase == ShotPhase::Done {
                return;
            }
            match shot.phase {
                ShotPhase::Main => {
                    self.help_open = false;
                    self.about_open = false;
                    self.shot_tip = false;
                    self.confirm = None;
                    self.summary = None;
                }
                ShotPhase::Help => {
                    self.help_open = true;
                    self.about_open = false;
                    self.shot_tip = false;
                    self.help_topic = help::ID_START;
                }
                ShotPhase::Tip => {
                    self.help_open = false;
                    self.about_open = false;
                    self.shot_tip = true;
                }
                ShotPhase::About => {
                    self.help_open = false;
                    self.about_open = true;
                    self.shot_tip = false;
                    self.confirm = None;
                    self.summary = None;
                }
                ShotPhase::Preflight => {
                    self.help_open = false;
                    self.about_open = false;
                    self.shot_tip = false;
                    self.summary = None;
                    if self.confirm.is_none() {
                        self.preflight = sys::preflight();
                        self.mismatch = self.preflight.mismatch.clone();
                        self.admin = self.preflight.elevated;
                        self.confirm = Some(Pending::Apply { cats: Cat::ALL.iter().copied().filter(|c| *c != Cat::Cleanup).collect(), all: false });
                    }
                }
                ShotPhase::Summary => {
                    self.help_open = false;
                    self.about_open = false;
                    self.shot_tip = false;
                    self.confirm = None;
                    if self.summary.is_none() {
                        self.summary = Some(ApplySummary {
                            title: "Apply finished".into(),
                            set: 47,
                            skip: 8,
                            errors: 0,
                            problems: vec![],
                        });
                    }
                }
                ShotPhase::Done => {}
            }
            let mut got = None;
            ctx.input(|i| {
                for ev in &i.events {
                    if let egui::Event::Screenshot { image, .. } = ev {
                        got = Some(image.clone());
                    }
                }
            });
            if let Some(image) = got {
                let name = match shot.phase {
                    ShotPhase::Main => "01-main-menu-bar.png",
                    ShotPhase::Help => "02-help-window.png",
                    ShotPhase::Tip => "03-help-tip.png",
                    ShotPhase::About => "04-about-popup.png",
                    ShotPhase::Preflight => "05-preflight-apply.png",
                    ShotPhase::Summary => "06-apply-summary.png",
                    ShotPhase::Done => "done.png",
                };
                let path = shot.dir.join(name);
                if let Err(e) = save_png(&path, &image) {
                    eprintln!("screenshot {}: {e}", path.display());
                } else {
                    eprintln!("wrote {}", path.display());
                }
                shot.phase = match shot.phase {
                    ShotPhase::Main => ShotPhase::Help,
                    ShotPhase::Help => ShotPhase::Tip,
                    ShotPhase::Tip => ShotPhase::About,
                    ShotPhase::About => ShotPhase::Preflight,
                    ShotPhase::Preflight => ShotPhase::Summary,
                    ShotPhase::Summary | ShotPhase::Done => ShotPhase::Done,
                };
                shot.frames = 0;
                shot.waiting = false;
                if shot.phase == ShotPhase::Done {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                ctx.request_repaint();
                return;
            }
            shot.frames = shot.frames.saturating_add(1);
            let need = match shot.phase {
                ShotPhase::Preflight | ShotPhase::Summary => 18,
                _ => 10,
            };
            if !shot.waiting && shot.frames >= need {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                shot.waiting = true;
                shot.frames = 0;
            }
            ctx.request_repaint();
        }

        fn start<F>(&mut self, ctx: &egui::Context, name: &'static str, f: F)
        where
            F: FnOnce(&mut Log) + Send + 'static,
        {
            let (tx, rx) = channel();
            self.rx = Some(rx);
            self.status = format!("{name}: running...");
            self.push(Lvl::Info, format!("===== {name} ====="));
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let mut log = Log::new(tx, name);
                f(&mut log);
                ctx.request_repaint();
            });
        }

        fn poll(&mut self) {
            let mut finished = None;
            if let Some(rx) = &self.rx {
                while let Ok(m) = rx.try_recv() {
                    match m {
                        Msg::Line(l, s, d) => self.log.push((l, s, d)),
                        Msg::Done(s) => finished = Some(s),
                        Msg::Summary(sum) => {
                            if !self.prefs.expert {
                                self.summary = Some(sum);
                            }
                        }
                    }
                }
            }
            if let Some(s) = finished {
                self.status = s;
                self.rx = None;
                self.temp = ops::temp_allow_state();
            }
        }

        fn describe(cats: &[Cat], o: Opts) -> Vec<String> {
            let mut v = vec![];
            for c in cats {
                let n = REG.iter().filter(|i| i.cat == *c).count();
                let extra = match c {
                    Cat::Cleanup => "stop wuauserv/bits; delete Download\\*, PostRebootEventCache.V2\\*, ReportingEvents.log; restart what was running".to_string(),
                    Cat::Updates if o.policy_only => "policy values only".into(),
                    Cat::Updates => format!("+ no-phone-home policies (DoNotConnect…, DisableWindowsUpdateAccess, WU pointed at {DEAD_WSUS}, DO mode 99); disable WaaSMedic/UpdateOrchestrator/WindowsUpdate tasks (using a built-in SYSTEM helper if Windows protects them); WaaSMedicSvc, uhssvc, UsoSvc, wuauserv -> Disabled + stopped; take ownership of the WaaSMedicSvc service registry KEY (not files) and deny SYSTEM writes to it; firewall outbound block rules {FW_PREFIX}*"),
                    Cat::Notifications => "disable task USO_UxBroker".into(),
                    Cat::Copilot => format!("kill Copilot processes; remove Appx Microsoft.Copilot{} (all users + provisioned); add Deprovisioned keys", if o.keep_m365 { "" } else { " + Microsoft.MicrosoftOfficeHub" }),
                    Cat::GodMode => format!("create {}\\{}", sys::desktop_dir().display(), GODMODE_NAME),
                    _ => String::new(),
                };
                let mut line = format!("• {}", c.title());
                if n > 0 {
                    line.push_str(&format!("  [{n} registry values]"));
                }
                if !extra.is_empty() {
                    line.push_str(&format!("  [{extra}]"));
                }
                v.push(line);
            }
            v
        }


        fn refresh_preflight(&mut self) {
            self.preflight = sys::preflight();
            self.mismatch = self.preflight.mismatch.clone();
            self.admin = self.preflight.elevated;
        }

        fn preflight_rows(&self, ui: &mut egui::Ui, offer_relaunch: bool) {
            let k = theme::t();
            let pf = &self.preflight;
            let row = |ui: &mut egui::Ui, color: egui::Color32, title: &str, body: &str, topic: &'static str| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(title).strong().color(color));
                    help::link(ui, topic);
                });
                if !body.is_empty() {
                    ui.add(egui::Label::new(RichText::new(body).small().color(k.muted)).wrap());
                }
            };

            if pf.elevated {
                row(ui, k.ok, "Running as administrator", "This app has the rights it needs to apply or revert tweaks.", help::ID_PREFLIGHT);
            } else {
                row(ui, k.err, "Not running as administrator", "Apply, Revert, Temporary allow, and the OneDrive wizard need administrator rights. The app usually asks for this when it starts.", help::ID_PREFLIGHT);
                if offer_relaunch {
                    if ui.add(theme::primary_button("Restart as administrator")).on_hover_text("Closes this window and asks Windows for administrator permission (UAC).").clicked() {
                        if sys::relaunch_elevated() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                }
            }

            match pf.session_is_admin {
                Some(true) => row(ui, k.ok, "Signed-in account is an administrator", "The person signed in to Windows can approve the UAC prompt with their own account.", help::ID_PREFLIGHT),
                Some(false) => row(ui, k.warn, "Signed-in account is a standard user", "At the UAC prompt you will need the password of an administrator account. Per-user tweaks would then go to that administrator, not to the person sitting at the PC.", help::ID_PREFLIGHT),
                None => row(ui, k.info, "Could not check whether the signed-in account is an administrator", "", help::ID_PREFLIGHT),
            }

            row(ui, k.info, &pf.uac_label(), sys::Preflight::uac_summary(), help::ID_PREFLIGHT);

            match pf.tamper_on {
                Some(true) => row(ui, k.info, "Microsoft Defender Tamper Protection is on", "This does not block the tweaks this app makes.", help::ID_PREFLIGHT),
                Some(false) => row(ui, k.info, "Microsoft Defender Tamper Protection is off", "This does not affect what the app can do. The app never changes this setting.", help::ID_PREFLIGHT),
                None => row(ui, k.info, "Microsoft Defender Tamper Protection could not be read", "Usually this just means Defender is not installed, or the value was not present.", help::ID_PREFLIGHT),
            }
            if pf.third_party_av.is_empty() {
                row(ui, k.info, "No third-party antivirus detected", "Microsoft Defender is fine. A third-party product can sometimes block or quarantine this app.", help::ID_PREFLIGHT);
            } else {
                let names = pf.third_party_av.join(", ");
                row(ui, k.warn, &format!("Third-party antivirus detected: {names}"), "It may block or quarantine this app or the changes it writes. Consider adding PrimeCrewTweakinator5000.exe to that product's allow list (exclusions / whitelist) before you Apply. The app never changes your antivirus settings.", help::ID_PREFLIGHT);
            }

            if let Some((me, who)) = &pf.mismatch {
                row(ui, k.err, &format!("Running as {me}, not the signed-in user ({who})"), "Per-user tweaks (and the OneDrive wizard) would affect the elevated account, not the person sitting at the PC. The OneDrive wizard is locked. Close the app and start it again while signed in as that person, using an administrator account for them.", help::ID_PREFLIGHT);
            } else {
                row(ui, k.ok, "Elevated account matches the signed-in user", "Per-user tweaks will apply to the person sitting at the PC.", help::ID_PREFLIGHT);
            }

            if pf.reboot_pending.is_empty() {
                row(ui, k.ok, "No reboot is pending", "", help::ID_PREFLIGHT);
            } else {
                row(ui, k.warn, "A reboot is pending", &format!("{}. Finish the reboot first, then run this app again.", pf.reboot_pending.join("; ")), help::ID_PREFLIGHT);
            }

            if pf.windows11 {
                row(ui, k.warn, &format!("This PC is running Windows 11 (build {})", pf.build), "This toolkit is built for Windows 10. You can continue, but some settings differ, and Windows 11-only values are used only there.", help::ID_PREFLIGHT);
            } else {
                row(ui, k.ok, &format!("This is Windows 10 (build {})", pf.build), "This toolkit is built for Windows 10, especially 22H2.", help::ID_PREFLIGHT);
            }
        }


        fn preflight_alert_count(&self) -> usize {
            let p = &self.preflight;
            let mut n = 0;
            if !p.elevated { n += 1; }
            if p.session_is_admin == Some(false) { n += 1; }
            if p.mismatch.is_some() { n += 1; }
            if !p.reboot_pending.is_empty() { n += 1; }
            if p.windows11 { n += 1; }
            if !p.third_party_av.is_empty() { n += 1; }
            n
        }

        fn preflight_compact(&self, ui: &mut egui::Ui) {
            let k = theme::t();
            let p = &self.preflight;
            let n = self.preflight_alert_count();
            if n == 0 {
                ui.label(RichText::new("All checks look good.").strong().color(k.ok));
                ui.add(egui::Label::new(RichText::new("Running as administrator on Windows 10, no pending reboot. Click Apply selected… to see the full check before anything is written.").small().color(k.muted)).wrap());
            } else {
                ui.label(RichText::new(format!("{n} item(s) need your attention before you Apply.")).strong().color(k.warn));
                if !p.elevated {
                    ui.label(RichText::new("• Not running as administrator.").color(k.err));
                    if ui.add(theme::primary_button("Restart as administrator")).clicked() {
                        if sys::relaunch_elevated() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                }
                if p.session_is_admin == Some(false) {
                    ui.add(egui::Label::new(RichText::new("• The signed-in account is a standard user. You will need an administrator password at the UAC prompt.").color(k.warn)).wrap());
                }
                if let Some((me, who)) = &p.mismatch {
                    ui.add(egui::Label::new(RichText::new(format!("• Running as {me}, not the signed-in user ({who}). Per-user tweaks would go to the wrong account.")).color(k.err)).wrap());
                }
                if !p.reboot_pending.is_empty() {
                    ui.add(egui::Label::new(RichText::new(format!("• A reboot is pending: {}.", p.reboot_pending.join("; "))).color(k.warn)).wrap());
                }
                if p.windows11 {
                    ui.add(egui::Label::new(RichText::new(format!("• This PC is running Windows 11 (build {}). This toolkit is built for Windows 10.", p.build)).color(k.warn)).wrap());
                }
                if !p.third_party_av.is_empty() {
                    ui.add(egui::Label::new(RichText::new(format!("• Third-party antivirus detected ({}). It may block or quarantine this app — consider adding it to that product's allow list.", p.third_party_av.join(", "))).color(k.warn)).wrap());
                }
                ui.add(egui::Label::new(RichText::new("Click Apply selected… to see the full check. Nothing is changed automatically.").small().color(k.muted)).wrap());
            }
        }

        fn summary_window(&mut self, ctx: &egui::Context) {
            let Some(sum) = self.summary.clone() else { return };
            let k = theme::t();
            let mut close = false;
            let mut stay = true;
            egui::Window::new(&sum.title)
                .open(&mut stay)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .default_width(560.0)
                .show(ctx, |ui| {
                    if sum.errors == 0 {
                        ui.label(RichText::new("Everything worked.").heading().strong().color(k.ok));
                    } else {
                        ui.label(RichText::new(format!("{} real problem(s) need your attention.", sum.errors)).heading().strong().color(k.err));
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("{} changes made", sum.set)).color(k.ok));
                    ui.label(RichText::new(format!("{} already done (skipped)", sum.skip)).color(k.muted));
                    ui.label(RichText::new(format!("{} real problem{}", sum.errors, if sum.errors == 1 { "" } else { "s" })).color(if sum.errors == 0 { k.ok } else { k.err }));
                    if !sum.problems.is_empty() {
                        ui.add_space(6.0);
                        ui.label(RichText::new("What went wrong").strong());
                        egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                            for (tech, plain) in &sum.problems {
                                ui.add(egui::Label::new(RichText::new(format!("• {plain}")).color(k.err)).wrap());
                                if self.prefs.expert {
                                    ui.add(egui::Label::new(RichText::new(tech).small().monospace().color(k.muted)).wrap());
                                }
                            }
                        });
                    }
                    ui.add_space(8.0);
                    ui.add(egui::Label::new(RichText::new("Next: reboot this PC, then click Verify (read-only) to confirm everything stuck.").color(k.info)).wrap());
                    help::link(ui, help::ID_LOG_GUIDE);
                    ui.add_space(10.0);
                    if ui.add(theme::primary_button("Close")).clicked() {
                        close = true;
                    }
                });
            if close || !stay {
                self.summary = None;
            }
        }

        fn confirm_window(&mut self, ctx: &egui::Context) {
            let Some(p) = &self.confirm else { return };
            let (title, cats, all, is_apply) = match p {
                Pending::Apply { cats, all } => ("Confirm apply", cats.clone(), *all, true),
                Pending::Revert { cats } => ("Confirm revert", cats.clone(), false, false),
                Pending::TempAllow => {
                    self.temp_confirm_window(ctx);
                    return;
                }
                Pending::Od(a) => {
                    let a = *a;
                    self.od_confirm(ctx, a);
                    return;
                }
            };
            let k = theme::t();
            let mut close = false;
            let mut go = false;
            egui::Window::new(title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .default_width(760.0)
                .show(ctx, |ui| {
                    let mut ok = true;
                    egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
                    if is_apply {
                        ui.label(RichText::new("Pre-flight check (read-only — nothing here is changed)").strong());
                        help::link(ui, help::ID_PREFLIGHT);
                        ui.add_space(4.0);
                        theme::card(ui, "This PC", |ui| {
                            self.preflight_rows(ui, true);
                        });
                        ui.add_space(8.0);
                    }
                    ui.label(if is_apply { "These changes will be made (each one is logged old → new):" } else { "These tweaks will be reverted to recorded originals (or Windows defaults):" });
                    theme::card(ui, "", |ui| {
                        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                            for l in Self::describe(&cats, self.opts) {
                                ui.add(egui::Label::new(RichText::new(l).monospace()).wrap());
                            }
                        });
                    });
                    if is_apply {
                        ui.label(RichText::new(if self.opts.restore_point { "✔ A System Restore point is created first." } else { "No restore point will be created (checkbox is off)." }).color(if self.opts.restore_point { k.ok } else { k.warn }));
                        if cats.contains(&Cat::Updates) {
                            ui.add(egui::Label::new(RichText::new(format!("⚠ {SECURITY_NOTE}")).color(k.warn)).wrap());
                            ui.checkbox(&mut self.ack_updates, "I understand Windows Update (including security/ESU patches) will be blocked");
                            ok &= self.ack_updates;
                        }
                        if all {
                            ui.checkbox(&mut self.ack_all, "Apply ALL tweaks listed above");
                            ok &= self.ack_all;
                        }
                    }
                    });
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        let b = if is_apply { theme::primary_button("Confirm apply") } else {
                            egui::Button::new(RichText::new("Confirm revert").strong().color(k.err)).stroke(egui::Stroke::new(1.5_f32, k.err)).min_size(egui::vec2(160.0_f32, 30.0_f32))
                        };
                        if ui.add_enabled(ok && self.admin, b).on_hover_text(if self.admin { "Apply these changes" } else { "Restart as administrator first (see the pre-flight check above)" }).clicked() {
                            go = true;
                        }
                        if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(90.0, 30.0))).clicked() {
                            close = true;
                        }
                    });
                });
            if go {
                let o = self.opts;
                if is_apply {
                    self.start(ctx, "Apply", move |log| ops::apply(log, &cats, o));
                } else {
                    self.start(ctx, "Revert", move |log| ops::revert(log, &cats));
                }
                close = true;
            }
            if close {
                self.confirm = None;
                self.ack_updates = false;
                self.ack_all = false;
            }
        }

        fn temp_confirm_window(&mut self, ctx: &egui::Context) {
            let k = theme::t();
            let (mut go, mut close) = (false, false);
            egui::Window::new("Temporarily allow updates")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .default_width(560.0)
                .show(ctx, |ui| {
                    ui.add(egui::Label::new(RichText::new(format!("Windows Update will be able to reach the internet until you restore the kill ({}).", preset_label(self.temp_minutes))).strong().color(k.warn)).wrap());
                    ui.add(egui::Label::new(RichText::new("Lifts only: no-connect / intranet-server policies, the Win10Tweaks-NoWU-* firewall rules, and wuauserv + UsoSvc. Automatic OS updates stay off (NoAutoUpdate=1), WaaSMedic stays disabled. Copilot / Spotlight / Win11 block are not touched.").color(k.muted)).wrap());
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui.add(theme::primary_button("Allow updates now")).clicked() {
                            go = true;
                        }
                        if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(90.0, 30.0))).clicked() {
                            close = true;
                        }
                    });
                });
            if go {
                let m = (self.temp_minutes > 0).then_some(self.temp_minutes);
                self.start(ctx, "TempAllow", move |log| ops::temp_allow(log, m));
                close = true;
            }
            if close {
                self.confirm = None;
            }
        }

        fn restore_kill(&mut self, ctx: &egui::Context) {
            self.start(ctx, "RestoreKill", ops::restore_kill);
        }

        fn temp_left(&self) -> String {
            match self.temp.as_ref().and_then(|t| t.expires) {
                Some(e) => {
                    let secs = (e - chrono::Local::now().naive_local()).num_seconds().max(0);
                    let how = if self.temp.as_ref().is_some_and(|t| t.task) { "" } else { " - keep this app open" };
                    format!("auto-restore in {}:{:02}{how}", secs / 60, secs % 60)
                }
                None => "until you click Restore update kill".into(),
            }
        }

        fn header(&mut self, ctx: &egui::Context) {
            let k = theme::t();
            egui::TopBottomPanel::top("top")
                .frame(egui::Frame::new().fill(k.bar_fill).inner_margin(egui::Margin::symmetric(14, 8)))
                .show(ctx, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let h = ui.text_style_height(&egui::TextStyle::Heading).max(28.0);
                        ui.add(egui::Image::new(&self.icon).fit_to_exact_size(egui::vec2(h + 4.0, h + 4.0))).on_hover_text(APP_TITLE);
                        ui.label(RichText::new(APP_NAME).heading().strong());
                        ui.label(RichText::new(format!("v{APP_VERSION}")).color(k.muted));
                        if self.admin {
                            theme::pill(ui, "ADMIN", k.pill_ok, "Running elevated (Administrator)");
                        } else {
                            theme::pill(ui, "NOT ELEVATED", k.pill_err, "Apply / Revert need Administrator. Right-click the exe > Run as administrator.");
                        }
                        if let Some((me, who)) = &self.mismatch {
                            theme::pill(ui, &format!("⚠ RUNNING AS {me}, NOT {who}"), k.pill_err, "The elevated account differs from the signed-in user. Per-user tweaks go to the elevated account; the OneDrive wizard is locked.");
                        }
                        if self.temp.is_some() {
                            let left = self.temp_left();
                            theme::pill(ui, &format!("UPDATES TEMPORARILY ALLOWED · {left}"), k.pill_err, "Windows Update can reach the internet. Click Restore update kill when your Store / WinGet updates are done.");
                        } else {
                            theme::pill(ui, "⚠ No updates = no security/ESU patches", k.pill_warn, SECURITY_NOTE);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let mut p = self.prefs;
                            let r = ui.button("+").on_hover_text("Larger text (Ctrl +)");
                            if r.clicked() {
                                p.scale = theme::step(p.scale, true);
                            }
                            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Larger text"));
                            let r = ui.button(format!("{:.0} %", p.scale * 100.0)).on_hover_text("Reset text size (Ctrl 0)");
                            if r.clicked() {
                                p.scale = 1.0;
                            }
                            let r = ui.button("−").on_hover_text("Smaller text (Ctrl −)");
                            if r.clicked() {
                                p.scale = theme::step(p.scale, false);
                            }
                            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Smaller text"));
                            ui.label(RichText::new("Text").color(k.muted));
                            self.set_prefs(ctx, p);
                        });
                    });
                    ui.label(RichText::new(&self.os).small().color(k.muted));
                });
        }

        fn status_bar(&mut self, ctx: &egui::Context) {
            let k = theme::t();
            egui::TopBottomPanel::bottom("status")
                .frame(egui::Frame::new().fill(k.bar_fill).inner_margin(egui::Margin::symmetric(14, 5)))
                .show(ctx, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if self.busy() {
                            ui.spinner();
                        }
                        ui.label(&self.status);
                        ui.label(RichText::new(format!("·  offline · no telemetry · {APP_TITLE} · logs in C:\\ProgramData\\Win10Tweaks\\Logs")).color(k.muted));
                    });
                });
        }

        fn log_panel(&mut self, ctx: &egui::Context) {
            let k = theme::t();
            let busy = self.busy();
            egui::TopBottomPanel::bottom("log")
                .resizable(true)
                .default_height(220.0)
                .min_height(120.0)
                .show(ctx, |ui| {
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Log").strong().size(14.5));
                        ui.label(RichText::new("every key / service / task touched: old → new").color(k.muted));
                        help::link(ui, help::ID_LOG_GUIDE);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add_enabled(!busy, egui::Button::new("Clear")).clicked() {
                                self.log.clear();
                            }
                            if ui.button("Copy").clicked() {
                                let t: String = self.log.iter().map(|(l, s, d)| match d {
                                    Some(det) => format!("[{}] {} ({})\n", l.tag(), s, det),
                                    None => format!("[{}] {}\n", l.tag(), s),
                                }).collect();
                                ui.ctx().copy_text(t);
                            }
                            if ui.button("Open logs folder").clicked() {
                                self.open_logs();
                            }
                            let mut p = self.prefs;
                            egui::ComboBox::from_id_salt("mono")
                                .selected_text(format!("Log {:.0} %", p.mono * 100.0))
                                .show_ui(ui, |ui| {
                                    for m in theme::MONO_STEPS {
                                        ui.selectable_value(&mut p.mono, m, format!("{:.0} %", m * 100.0));
                                    }
                                })
                                .response
                                .on_hover_text("Log text size (relative to body text)");
                            self.set_prefs(ui.ctx(), p);
                        });
                    });
                    if !self.prefs.expert {
                        ui.add(egui::Label::new(RichText::new(LOG_LEGEND).small().color(k.muted)).wrap());
                    }
                    let expert = self.prefs.expert;
                    egui::Frame::new()
                        .fill(k.log_fill)
                        .stroke(egui::Stroke::new(1.0_f32, k.card_stroke))
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            egui::ScrollArea::both().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                                for (l, s, d) in &self.log {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(RichText::new(format!("[{:<5}] {}", l.tag(), s)).monospace().color(lvl_color(*l)));
                                        if expert {
                                            if let Some(det) = d {
                                                ui.label(RichText::new(det).monospace().color(k.muted));
                                            }
                                        }
                                    });
                                }
                            });
                        });
                });
        }
    }

    impl eframe::App for App {
        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            theme::begin_frame(ctx, &self.prefs);
            self.shortcuts(ctx);
            self.poll();
            if self.busy() || self.od_after.is_some() {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            if let Some(t) = &self.temp {
                if t.expires.is_some() {
                    ctx.request_repaint_after(std::time::Duration::from_secs(1));
                }
                if ops::temp_expired(t) && self.admin && !self.busy() && self.confirm.is_none() {
                    self.push(Lvl::Warn, "Temporary update allow expired: restoring the update kill now.");
                    self.restore_kill(ctx);
                }
            }
            if let Some(id) = help::take() {
                self.help_topic = id;
                self.help_open = true;
            }
            let busy = self.busy();
            let can_change = self.admin && !busy && self.confirm.is_none();
            let k = theme::t();
            let expert = self.prefs.expert;

            self.menu_bar(ctx);
            self.header(ctx);
            self.status_bar(ctx);
            self.log_panel(ctx);

            egui::SidePanel::left("left").resizable(true).default_width(470.0).min_width(320.0).show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(6.0);
                    theme::card(ui, "Tweaks", |ui| {
                        for (i, c) in Cat::ALL.iter().enumerate() {
                            ui.horizontal_wrapped(|ui| {
                                ui.add_enabled(!busy, egui::Checkbox::new(&mut self.sel[i], c.title()));
                                help::mark(ui, help::tip_for(*c), help::id_for(*c), expert);
                                help::link(ui, help::id_for(*c));
                            });
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui.add_enabled(!busy, egui::Button::new("Select all").small()).clicked() {
                                self.sel = [true; 7];
                            }
                            if ui.add_enabled(!busy, egui::Button::new("None").small()).clicked() {
                                self.sel = [false; 7];
                            }
                        });
                    });
                    theme::card(ui, "Apply options", |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.checkbox(&mut self.opts.restore_point, "Create System Restore point before Apply (recommended)");
                            help::mark(ui, "Creates a Windows System Restore point first. If this is ticked and the restore point cannot be created, Apply stops and nothing else is written.", help::ID_OPTIONS, expert);
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.checkbox(&mut self.opts.policy_only, "Updates: policy values only (leave services/tasks)");
                            help::mark(ui, "A lighter update kill: writes the basic policies only, and leaves services, tasks, the firewall and the no-phone-home policies alone.", help::ID_OPTIONS, expert);
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.checkbox(&mut self.opts.keep_m365, "Copilot: keep Microsoft 365 (OfficeHub) app");
                            help::mark(ui, "By default, removing Copilot also removes the Microsoft 365 (OfficeHub) app. Tick this to leave that app installed.", help::ID_OPTIONS, expert);
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.checkbox(&mut self.opts.force_cleanup, "Cleanup: run even if a reboot is pending (not recommended)");
                            help::mark(ui, "SoftwareDistribution cleanup normally refuses to run if a reboot is pending or TrustedInstaller is busy. Leave this off unless you know why you need it.", help::ID_OPTIONS, expert);
                        });
                    });
                });
            });

            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(6.0);
                    let sel = self.selected();
                    theme::card(ui, "This PC (pre-flight, read-only)", |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.add(egui::Label::new(RichText::new("A quick look at this computer before you Apply. Nothing here is changed.").color(k.muted)).wrap());
                            help::mark(ui, "Read-only checks: administrator, UAC, antivirus, the signed-in account, a pending reboot, and the Windows version. The full list appears when you click Apply.", help::ID_PREFLIGHT, expert);
                            help::link(ui, help::ID_PREFLIGHT);
                            if ui.small_button("Refresh").on_hover_text("Read the checks again. Does not change any settings.").clicked() {
                                self.refresh_preflight();
                            }
                        });
                        ui.add_space(4.0);
                        self.preflight_compact(ui);
                    });
                    theme::card(ui, "Actions", |ui| {
                        ui.horizontal_wrapped(|ui| {
                            if ui.add_enabled(!busy && self.confirm.is_none() && !sel.is_empty(), theme::primary_button("Apply selected…")).on_hover_text("Review the exact changes and a pre-flight check, then confirm").clicked() {
                                self.refresh_preflight();
                                self.confirm = Some(Pending::Apply { cats: sel.clone(), all: false });
                            }
                            help::mark(ui, "Shows a list of the exact changes and asks you to confirm before anything is written.", help::ID_ACTIONS, expert);
                            if ui.add_enabled(!busy && self.confirm.is_none(), egui::Button::new("Apply all…").min_size(egui::vec2(120.0, 32.0))).clicked() {
                                self.refresh_preflight();
                                self.confirm = Some(Pending::Apply { cats: Cat::ALL.to_vec(), all: true });
                            }
                            if ui.add_enabled(!busy, egui::Button::new(RichText::new("Verify (read-only)").color(k.info)).min_size(egui::vec2(140.0, 32.0))).on_hover_text("PASS / FAIL / WARN for every setting. Changes nothing.").clicked() {
                                self.start(ctx, "Verify", ops::verify);
                            }
                            help::mark(ui, "Read-only check. Reports PASS, FAIL or WARN for every setting. Changes nothing.", help::ID_ACTIONS, expert);
                        });
                        ui.add_space(2.0);
                        ui.horizontal_wrapped(|ui| {
                            let rsel: Vec<Cat> = sel.iter().copied().filter(|c| *c != Cat::Cleanup).collect();
                            if ui.add_enabled(can_change && !rsel.is_empty(), egui::Button::new(RichText::new("Revert selected…").color(k.err))).on_hover_text("Restore recorded originals for the ticked tweaks").clicked() {
                                self.confirm = Some(Pending::Revert { cats: rsel });
                            }
                            help::mark(ui, "Puts the recorded original settings back for the ticked tweaks. Cleanup cannot be reverted this way.", help::ID_ACTIONS, expert);
                            if ui.add_enabled(can_change, egui::Button::new(RichText::new("Revert all…").color(k.err))).clicked() {
                                self.confirm = Some(Pending::Revert { cats: Cat::ALL.iter().copied().filter(|c| *c != Cat::Cleanup).collect() });
                            }
                        });
                        ui.add_space(4.0);
                        ui.separator();
                        if self.temp.is_some() {
                            ui.add(egui::Label::new(RichText::new(format!("⚠ UPDATES TEMPORARILY ALLOWED ({}). Update your Store / WinGet apps, then restore the kill.", self.temp_left())).strong().color(k.err)).wrap());
                            if ui.add_enabled(can_change, theme::primary_button("Restore update kill")).on_hover_text("Re-applies Updates exactly like Apply: policies, forced services incl. WaaSMedic, tasks, firewall, no-phone-home").clicked() {
                                self.restore_kill(ctx);
                            }
                            help::mark(ui, "Turns the hard Windows Update kill back on, and cancels a Temporary allow timer.", help::ID_TEMP, expert);
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                if ui.add_enabled(can_change, egui::Button::new(RichText::new("Temporarily allow updates…").color(k.warn)).min_size(egui::vec2(200.0, 30.0))).on_hover_text("Lift only the Updates kill so Microsoft Store / WinGet can update apps (PowerToys, ChatGPT…). Copilot / Spotlight / Win11 block stay.").clicked() {
                                    self.confirm = Some(Pending::TempAllow);
                                }
                                help::mark(ui, "Lifts just enough of the update kill so the Microsoft Store can update apps. Automatic OS updates stay off. Click for details.", help::ID_TEMP, expert);
                                egui::ComboBox::from_id_salt("tempdur")
                                    .selected_text(preset_label(self.temp_minutes))
                                    .show_ui(ui, |ui| {
                                        for m in TEMP_PRESETS {
                                            ui.selectable_value(&mut self.temp_minutes, m, preset_label(m));
                                        }
                                    })
                                    .response
                                    .on_hover_text("Timed allows restore the kill automatically (one-shot SYSTEM task, even if this app is closed)");
                            });
                        }
                        if !self.admin {
                            ui.label(RichText::new("Apply / Revert need Administrator.").color(k.err));
                        }
                    });
                    theme::card(ui, "Selected changes", |ui| {
                        if sel.is_empty() {
                            ui.label(RichText::new("Nothing ticked.").color(k.muted));
                        }
                        for l in Self::describe(&sel, self.opts) {
                            ui.add(egui::Label::new(RichText::new(l).color(k.muted)).wrap());
                        }
                        if sel.contains(&Cat::Updates) {
                            ui.add(egui::Label::new(RichText::new(format!("⚠ {SHORT_NOTE}.")).color(k.warn)).wrap()).on_hover_text(SECURITY_NOTE);
                        }
                    });
                    theme::card(ui, "OneDrive removal wizard", |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.add(egui::Label::new(RichText::new("Step by step: make sure your files are on this PC, stop OneDrive syncing, then remove it. Your files are never deleted. Not part of Apply all.").color(k.muted)).wrap());
                            help::mark(ui, "A guided wizard. Your files are never deleted. The wizard is not part of Apply all.", help::ID_ONEDRIVE, expert);
                            help::link(ui, help::ID_ONEDRIVE);
                        });
                        ui.horizontal_wrapped(|ui| {
                            if ui.add_enabled(!self.od_open, theme::primary_button("Open OneDrive wizard…")).clicked() {
                                self.od_open = true;
                                if self.od_step == Step::Done {
                                    self.od_step = Step::Check;
                                }
                            }
                            if self.mismatch.is_some() {
                                ui.label(RichText::new("Locked: wrong account (see header).").color(k.err));
                            }
                        });
                    });
                    theme::card(ui, "PowerShell fallback", |ui| {
                        ui.horizontal_wrapped(|ui| {
                            help::mark(ui, "Puts an equivalent script on the clipboard, or writes the full script kit next to the exe. The pasted script does not record originals; use Revert in the app to undo.", help::ID_PS, expert);
                            help::link(ui, help::ID_PS);
                            if ui.button("Copy PowerShell (selected)").clicked() {
                                self.copy_powershell(ctx);
                            }
                            if ui.button("Copy Apply-All-Tweaks.ps1").clicked() {
                                ctx.copy_text(psgen::KIT[1].1.to_string());
                                self.push(Lvl::Info, "Copied Apply-All-Tweaks.ps1 (needs the scripts\\ folder: use Export PS kit).");
                            }
                            if ui.button("Export PS kit next to exe").clicked() {
                                let dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_else(|| ".".into());
                                match psgen::export_kit(&dir) {
                                    Ok(p) => self.push(Lvl::Set, format!("PowerShell kit : <absent> -> written to {}", p.display())),
                                    Err(e) => self.push(Lvl::Error, format!("Export failed: {e}")),
                                }
                            }
                        });
                    });
                });
            });

            self.od_after_job(ctx);
            self.od_window(ctx);
            self.confirm_window(ctx);
            self.summary_window(ctx);
            help::show_window(ctx, &mut self.help_open, &mut self.help_topic);
            help::show_about(ctx, &self.icon, &mut self.about_open);
            self.draw_shot_tip(ctx);
            self.tick_shot(ctx);
            theme::focus_ring(ctx);
        }
    }

    fn save_png(path: &std::path::Path, image: &egui::ColorImage) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let raw = path.with_extension("rgba");
        let mut buf = Vec::with_capacity(image.pixels.len() * 4);
        for px in &image.pixels {
            buf.extend_from_slice(&[px.r(), px.g(), px.b(), px.a()]);
        }
        std::fs::write(&raw, &buf).map_err(|e| e.to_string())?;
        let script = path.with_extension("py");
        std::fs::write(&script, include_str!("../../scripts/write_png.py")).map_err(|e| e.to_string())?;
        let st = std::process::Command::new("python3")
            .arg(&script)
            .arg(path)
            .arg(image.width().to_string())
            .arg(image.height().to_string())
            .arg(&raw)
            .status()
            .map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&raw);
        let _ = std::fs::remove_file(&script);
        if st.success() {
            Ok(())
        } else {
            Err(format!("python3 write_png failed: {st}"))
        }
    }
}
