//! PrimeCrew Tweakinator 5000 (Windows 10 tweaks) - portable, offline (no network code at all), admin-only GUI.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod catalog;
#[cfg(windows)]
mod onedrive;
#[cfg(windows)]
mod ops;
#[cfg(windows)]
mod psgen;
#[cfg(windows)]
mod sys;
#[cfg(windows)]
mod theme;

#[cfg(not(windows))]
fn main() {
    eprintln!("PrimeCrew Tweakinator 5000 runs on Windows only.");
}

#[cfg(windows)]
fn main() -> eframe::Result<()> {
    // Headless one-shot used by the SYSTEM task Win10Tweaks-RestoreWUKill: re-apply the Updates kill, no window.
    if std::env::args().any(|a| a == catalog::RESTORE_FLAG) {
        if sys::is_admin() {
            ops::headless_restore();
        }
        return Ok(());
    }
    let admin = sys::is_admin();
    // The manifest already requests admin; this is the runtime fallback (e.g. manifest stripped).
    if !admin && std::env::args().all(|a| a != "--no-elevate") && sys::relaunch_elevated() {
        return Ok(());
    }
    let title = gui::APP_TITLE;
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(title)
            .with_icon(std::sync::Arc::new(eframe::egui::IconData { rgba: gui::ICON_RGBA.to_vec(), width: 128, height: 128 }))
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([820.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(title, options, Box::new(move |cc| Ok(Box::new(gui::App::new(cc, admin)))))
}

#[cfg(windows)]
mod gui {
    use crate::catalog::*;
    use crate::ops::{self, Log, Lvl, Msg, Opts};
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

    pub struct App {
        admin: bool,
        os: String,
        sel: [bool; 7],
        opts: Opts,
        log: Vec<(Lvl, String)>,
        rx: Option<Receiver<Msg>>,
        status: String,
        confirm: Option<Pending>,
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
        pub fn new(cc: &eframe::CreationContext<'_>, admin: bool) -> Self {
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
            self.log.push((l, s.into()));
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
                        Msg::Line(l, s) => self.log.push((l, s)),
                        Msg::Done(s) => finished = Some(s),
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
                    Cat::Updates => format!("+ no-phone-home policies (DoNotConnect…, DisableWindowsUpdateAccess, WU pointed at {DEAD_WSUS}, DO mode 99); disable WaaSMedic/UpdateOrchestrator/WindowsUpdate tasks (as SYSTEM if admin is refused); WaaSMedicSvc, uhssvc, UsoSvc, wuauserv -> Disabled + stopped; take ownership of the WaaSMedicSvc service registry KEY (not files) and deny SYSTEM writes to it; firewall outbound block rules {FW_PREFIX}*"),
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
                .default_width(720.0)
                .show(ctx, |ui| {
                    ui.label(if is_apply { "These changes will be made (each one is logged old → new):" } else { "These tweaks will be reverted to recorded originals (or Windows defaults):" });
                    theme::card(ui, "", |ui| {
                        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                            for l in Self::describe(&cats, self.opts) {
                                ui.add(egui::Label::new(RichText::new(l).monospace()).wrap());
                            }
                        });
                    });
                    let mut ok = true;
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
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        let b = if is_apply { theme::primary_button("Confirm apply") } else {
                            egui::Button::new(RichText::new("Confirm revert").strong().color(k.err)).stroke(egui::Stroke::new(1.5_f32, k.err)).min_size(egui::vec2(160.0_f32, 30.0_f32))
                        };
                        if ui.add_enabled(ok, b).clicked() {
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
                            egui::ComboBox::from_id_salt("theme")
                                .selected_text(p.look.label())
                                .show_ui(ui, |ui| {
                                    for l in Look::ALL {
                                        ui.selectable_value(&mut p.look, l, l.label());
                                    }
                                })
                                .response
                                .on_hover_text("Theme: System follows Windows (falls back to Dark)");
                            ui.label(RichText::new("Theme").color(k.muted));
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
                .default_height(260.0)
                .min_height(120.0)
                .show(ctx, |ui| {
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Log").strong().size(14.5));
                        ui.label(RichText::new("every key / service / task touched: old → new").color(k.muted));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add_enabled(!busy, egui::Button::new("Clear")).clicked() {
                                self.log.clear();
                            }
                            if ui.button("Copy").clicked() {
                                let t: String = self.log.iter().map(|(l, s)| format!("[{}] {}\n", l.tag(), s)).collect();
                                ui.ctx().copy_text(t);
                            }
                            if ui.button("Open logs folder").clicked() {
                                let d = ops::root_dir().join("Logs");
                                let _ = std::fs::create_dir_all(&d);
                                let _ = std::process::Command::new("explorer.exe").arg(d).spawn();
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
                    egui::Frame::new()
                        .fill(k.log_fill)
                        .stroke(egui::Stroke::new(1.0_f32, k.card_stroke))
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            egui::ScrollArea::both().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                                for (l, s) in &self.log {
                                    ui.label(RichText::new(format!("[{:<5}] {}", l.tag(), s)).monospace().color(lvl_color(*l)));
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
            let busy = self.busy();
            let can_change = self.admin && !busy && self.confirm.is_none();
            let k = theme::t();

            self.header(ctx);
            self.status_bar(ctx);
            self.log_panel(ctx);

            egui::SidePanel::left("left").resizable(true).default_width(470.0).min_width(320.0).show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(6.0);
                    theme::card(ui, "Tweaks", |ui| {
                        for (i, c) in Cat::ALL.iter().enumerate() {
                            ui.add_enabled(!busy, egui::Checkbox::new(&mut self.sel[i], c.title()));
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
                    theme::card(ui, "Options", |ui| {
                        ui.checkbox(&mut self.opts.restore_point, "Create System Restore point before Apply (recommended)");
                        ui.checkbox(&mut self.opts.policy_only, "Updates: policy values only (leave services/tasks)");
                        ui.checkbox(&mut self.opts.keep_m365, "Copilot: keep Microsoft 365 (OfficeHub) app");
                        ui.checkbox(&mut self.opts.force_cleanup, "Cleanup: run even if a reboot is pending (not recommended)");
                        ui.separator();
                        let mut p = self.prefs;
                        ui.checkbox(&mut p.expert, "Expert mode").on_hover_text("OneDrive wizard: skip the welcome screen and explanations, show registry paths, attrib flags and counts. Safety checks and confirmations stay.");
                        ui.label(RichText::new(if p.expert { "Expert: compact wizard with technical details." } else { "Beginner (default): the wizard explains every step." }).small().color(k.muted));
                        self.set_prefs(ui.ctx(), p);
                        ui.separator();
                        ui.label(RichText::new(format!("About: {APP_TITLE}")).small().color(k.muted));
                    });
                });
            });

            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(6.0);
                    let sel = self.selected();
                    theme::card(ui, "Actions", |ui| {
                        ui.horizontal_wrapped(|ui| {
                            if ui.add_enabled(can_change && !sel.is_empty(), theme::primary_button("Apply selected…")).on_hover_text("Review the exact changes, then confirm").clicked() {
                                self.confirm = Some(Pending::Apply { cats: sel.clone(), all: false });
                            }
                            if ui.add_enabled(can_change, egui::Button::new("Apply all…").min_size(egui::vec2(120.0, 32.0))).clicked() {
                                self.confirm = Some(Pending::Apply { cats: Cat::ALL.to_vec(), all: true });
                            }
                            if ui.add_enabled(!busy, egui::Button::new(RichText::new("Verify (read-only)").color(k.info)).min_size(egui::vec2(140.0, 32.0))).on_hover_text("PASS / FAIL / WARN for every setting. Changes nothing.").clicked() {
                                self.start(ctx, "Verify", ops::verify);
                            }
                        });
                        ui.add_space(2.0);
                        ui.horizontal_wrapped(|ui| {
                            let rsel: Vec<Cat> = sel.iter().copied().filter(|c| *c != Cat::Cleanup).collect();
                            if ui.add_enabled(can_change && !rsel.is_empty(), egui::Button::new(RichText::new("Revert selected…").color(k.err))).on_hover_text("Restore recorded originals for the ticked tweaks").clicked() {
                                self.confirm = Some(Pending::Revert { cats: rsel });
                            }
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
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                if ui.add_enabled(can_change, egui::Button::new(RichText::new("Temporarily allow updates…").color(k.warn)).min_size(egui::vec2(200.0, 30.0))).on_hover_text("Lift only the Updates kill so Microsoft Store / WinGet can update apps (PowerToys, ChatGPT…). Copilot / Spotlight / Win11 block stay.").clicked() {
                                    self.confirm = Some(Pending::TempAllow);
                                }
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
                        ui.add(egui::Label::new(RichText::new("Step by step: make sure your files are on this PC, stop OneDrive syncing, then remove it. Your files are never deleted. Not part of Apply all.").color(k.muted)).wrap());
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
                            if ui.button("Copy PowerShell (selected)").clicked() {
                                let s = psgen::generate(&self.selected(), self.opts);
                                let n = s.lines().count();
                                ctx.copy_text(s);
                                self.push(Lvl::Info, format!("Copied generated PowerShell ({n} lines) for: {}. Paste into an elevated PowerShell.", self.selected().iter().map(|c| c.id()).collect::<Vec<_>>().join(", ")));
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
            theme::focus_ring(ctx);
        }
    }
}
