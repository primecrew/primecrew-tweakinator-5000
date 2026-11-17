// "Clean temp files" card and its confirm dialogs (child of the gui module).
// Not part of Apply all. Scan is read-only; Clean needs an explicit "cannot be restored" tick;
// the optional restart pass only adds our own entries to Windows' delete-at-restart list.
use super::{App, Pending};
use crate::help;
use crate::tempclean::{self as tc, age_label, fmt_bytes, fmt_n, CleanOpts, Report, AGE_CHOICES};
use crate::theme;
use eframe::egui::{self, RichText};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TcAct {
    Clean,
    Schedule,
    CancelSchedule,
}

pub const TIP: &str = "Deletes leftover temporary files. Scan first to see how much space you would get back. Files changed in the last 24 hours and files in use are left alone. Deleted temp files cannot be restored. Not part of Apply all. Click for details.";

impl App {
    pub(super) fn tc_opts_now(&self) -> CleanOpts {
        self.tc_opts.effective(self.prefs.expert)
    }

    fn tc_scan_now(&self) -> Option<Report> {
        self.tc_scan.lock().ok().and_then(|g| g.clone())
    }

    fn tc_result_now(&self) -> Option<Report> {
        self.tc_result.lock().ok().and_then(|g| g.clone())
    }

    fn tc_start_scan(&mut self, ctx: &egui::Context) {
        let slot = self.tc_scan.clone();
        let o = self.tc_opts_now();
        let expert = self.prefs.expert;
        if let Ok(mut g) = self.tc_result.lock() {
            *g = None;
        }
        self.start(ctx, "TempScan", move |log| {
            let r = tc::scan(log, o, expert);
            if let Ok(mut g) = slot.lock() {
                *g = Some(r);
            }
        });
    }

    fn tc_start_clean(&mut self, ctx: &egui::Context) {
        let slot = self.tc_result.clone();
        let scan = self.tc_scan.clone();
        let o = self.tc_opts_now();
        let expert = self.prefs.expert;
        self.tc_scheduled = false;
        self.start(ctx, "TempClean", move |log| {
            let r = tc::clean(log, o, expert);
            if let Ok(mut g) = slot.lock() {
                *g = Some(r);
            }
            // The scan numbers are out of date now; scan again to see what is left.
            if let Ok(mut g) = scan.lock() {
                *g = None;
            }
        });
    }

    pub(super) fn tc_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let k = theme::t();
        let expert = self.prefs.expert;
        let busy = self.busy();
        let o = self.tc_opts_now();
        theme::card(ui, "Clean temp files", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::Label::new(RichText::new("Frees disk space by deleting leftover temporary files. Scan first; nothing is deleted until you confirm. Not part of Apply all.").color(k.muted)).wrap());
                help::mark(ui, TIP, help::ID_TEMPCLEAN, expert);
                help::link(ui, help::ID_TEMPCLEAN);
            });
            if let Some((me, who)) = &self.mismatch {
                ui.add(egui::Label::new(RichText::new(format!("Note: “Your temp folder” is the one for {me} (the account this app runs as), not {who}.")).color(k.warn)).wrap());
            }
            if expert {
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    ui.add_enabled(!busy, egui::Checkbox::new(&mut self.tc_opts.include_windows, "Also C:\\Windows\\Temp"));
                    ui.add_enabled(!busy, egui::Checkbox::new(&mut self.tc_opts.include_other_users, "Also other users' temp folders"));
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Leave alone files changed in the last");
                    ui.add_enabled_ui(!busy, |ui| {
                        egui::ComboBox::from_id_salt("tc-age")
                            .selected_text(age_label(self.tc_opts.min_age_hours))
                            .show_ui(ui, |ui| {
                                for h in AGE_CHOICES {
                                    let label = if h == tc::DEFAULT_MIN_AGE_HOURS { format!("{} (default)", age_label(h)) } else { age_label(h) };
                                    ui.selectable_value(&mut self.tc_opts.min_age_hours, h, label);
                                }
                            });
                    });
                    help::mark(ui, "A shorter time frees a little more space, but can break an install or update that is still running. 24 hours is the safe default.", help::ID_TEMPCLEAN, expert);
                });
            }
            ui.add_space(4.0);

            let scan = self.tc_scan_now().filter(|s| s.opts == o);
            let result = self.tc_result_now();
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(!busy && self.confirm.is_none(), egui::Button::new(RichText::new("Scan temp files").color(k.info)).min_size(egui::vec2(150.0, 30.0))).on_hover_text("Read-only: counts files and size. Deletes nothing.").clicked() {
                    self.tc_start_scan(ctx);
                }
                let can = scan.as_ref().is_some_and(|s| s.total().eligible_files > 0);
                if ui
                    .add_enabled(can && self.admin && !busy && self.confirm.is_none(), theme::primary_button("Clean temp files…"))
                    .on_hover_text(if scan.is_none() { "Scan first" } else { "Review and confirm before anything is deleted" })
                    .on_disabled_hover_text(if scan.is_none() { "Click Scan temp files first." } else if !self.admin { "Needs administrator rights." } else { "Nothing to clean." })
                    .clicked()
                {
                    self.tc_ack = false;
                    self.confirm = Some(Pending::Tc(TcAct::Clean));
                }
            });

            if let Some(s) = &scan {
                let t = s.total();
                ui.add_space(4.0);
                if t.eligible_files == 0 {
                    ui.label(RichText::new("Nothing to clean right now.").strong().color(k.ok));
                } else {
                    ui.label(RichText::new(format!("{} files, {} can be cleaned", fmt_n(t.eligible_files), fmt_bytes(t.eligible_bytes))).strong().color(k.info));
                }
                for r in &s.roots {
                    let line = if expert {
                        format!("• {} — {}: {} files, {}", r.root.label, r.root.shown(), fmt_n(r.stats.eligible_files), fmt_bytes(r.stats.eligible_bytes))
                    } else {
                        format!("• {}: {}", r.root.label, r.root.shown())
                    };
                    ui.add(egui::Label::new(RichText::new(line).small().color(k.muted)).wrap());
                }
                if t.recent_files > 0 {
                    ui.add(egui::Label::new(RichText::new(format!("Left alone: {} newer files ({}) changed in the last {}.", fmt_n(t.recent_files), fmt_bytes(t.recent_bytes), age_label(s.opts.min_age_hours))).small().color(k.muted)).wrap());
                }
                if expert && (t.links > 0 || t.unreadable > 0 || t.excluded > 0) {
                    ui.add(egui::Label::new(RichText::new(format!("Not followed: {} links / junctions · could not read: {} · app folder items: {}", t.links, t.unreadable, t.excluded)).small().monospace().color(k.muted)).wrap());
                }
                for (label, why) in &s.refused {
                    ui.add(egui::Label::new(RichText::new(format!("Not used: {label} — {why}")).small().color(k.warn)).wrap());
                }
                ui.label(RichText::new(format!("Scanned {}", s.when)).small().color(k.muted));
            } else if self.tc_scan_now().is_some() {
                ui.label(RichText::new("Options changed — click Scan temp files again.").small().color(k.muted));
            }

            if let Some(r) = &result {
                let t = r.total();
                let lf = t.locked_files();
                ui.add_space(6.0);
                ui.separator();
                if r.preview_only {
                    ui.label(RichText::new("Preview build: nothing was deleted.").color(k.muted));
                }
                ui.label(RichText::new(format!("Done: {} files deleted, {} freed.", fmt_n(t.deleted_files), fmt_bytes(t.deleted_bytes))).strong().color(k.ok));
                if lf > 0 {
                    ui.add(egui::Label::new(RichText::new(format!("{} files ({}) are in use by other programs and were left alone. That is normal.", fmt_n(lf), fmt_bytes(t.locked_bytes))).color(k.muted)).wrap());
                    if !self.tc_scheduled {
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .add_enabled(self.admin && !busy && self.confirm.is_none(), egui::Button::new(RichText::new("Finish cleanup at next restart…").color(k.info)).min_size(egui::vec2(220.0, 30.0)))
                                .on_hover_text("Asks Windows to delete these early in the next startup, before programs start. Nothing happens until you restart.")
                                .clicked()
                            {
                                self.confirm = Some(Pending::Tc(TcAct::Schedule));
                            }
                            help::mark(ui, "Uses Windows' own delete-at-restart list. Windows deletes these files very early in the next startup, before services and programs (like mouse or chat software) open them. You can cancel before you restart.", help::ID_TEMPCLEAN, expert);
                        });
                    }
                }
            }

            if let Some(p) = self.tc_pending.clone() {
                ui.add_space(6.0);
                ui.separator();
                if p.ran {
                    ui.label(RichText::new("Restart cleanup finished").strong().color(k.ok));
                    ui.add(egui::Label::new(format!("{} of {} files cleared at restart, {} recovered.", fmt_n(p.cleared_files), fmt_n(p.files), fmt_bytes(p.cleared_bytes))).wrap());
                    if p.still_there > 0 {
                        ui.add(egui::Label::new(RichText::new(format!("{} files were still in use (or were made again by a program) and are still there. You can scan and clean again later.", fmt_n(p.still_there))).small().color(k.muted)).wrap());
                    }
                    ui.label(RichText::new(format!("Scheduled {}", p.scheduled_at)).small().color(k.muted));
                    if ui.add_enabled(!busy, egui::Button::new("Dismiss")).clicked() {
                        tc::dismiss_pending();
                        self.tc_pending = tc::pending_status();
                    }
                } else {
                    ui.label(RichText::new("Cleanup scheduled for next restart").strong().color(k.info));
                    ui.add(egui::Label::new(format!("{} items ({}) will be deleted the next time Windows starts. Scheduled {}.", fmt_n(p.items), fmt_bytes(p.bytes), p.scheduled_at)).wrap());
                    ui.add(egui::Label::new(RichText::new("Until then, the pre-flight check will say a reboot is pending. That is expected.").small().color(k.muted)).wrap());
                    if ui
                        .add_enabled(self.admin && !busy && self.confirm.is_none(), egui::Button::new(RichText::new("Cancel scheduled cleanup…").color(k.err)))
                        .on_hover_text("Removes only this app's entries from Windows' delete-at-restart list.")
                        .clicked()
                    {
                        self.confirm = Some(Pending::Tc(TcAct::CancelSchedule));
                    }
                }
            }
        });
    }

    pub(super) fn tc_confirm(&mut self, ctx: &egui::Context, act: TcAct) {
        let k = theme::t();
        let expert = self.prefs.expert;
        let (mut go, mut close) = (false, false);
        match act {
            TcAct::Clean => {
                let Some(s) = self.tc_scan_now() else {
                    self.confirm = None;
                    return;
                };
                let t = s.total();
                egui::Window::new("Clean temp files")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .default_width(620.0)
                    .show(ctx, |ui| {
                        theme::card(ui, "", |ui| {
                            ui.add(egui::Label::new(RichText::new("Deleted temp files cannot be restored.").heading().strong().color(k.warn)).wrap());
                            ui.add(egui::Label::new("They do not go to the Recycle Bin, and there is no Revert for this. Temp files are leftovers that programs no longer need, so this is normally safe.").wrap());
                        });
                        theme::card(ui, "", |ui| {
                            ui.label(RichText::new(format!("About {} files ({}) will be deleted from:", fmt_n(t.eligible_files), fmt_bytes(t.eligible_bytes))).strong());
                            for r in &s.roots {
                                let line = format!("• {} — {}", r.root.label, r.root.shown());
                                ui.add(egui::Label::new(if expert { RichText::new(line).monospace() } else { RichText::new(line) }).wrap());
                            }
                            ui.add_space(4.0);
                            ui.add(egui::Label::new(RichText::new(format!("• Files changed in the last {} are left alone.", age_label(s.opts.min_age_hours))).color(k.muted)).wrap());
                            ui.add(egui::Label::new(RichText::new("• Files in use by other programs are skipped. You can finish those at the next restart.").color(k.muted)).wrap());
                            ui.add(egui::Label::new(RichText::new("• Only files inside these temp folders are touched. Shortcuts, links and junctions are never followed.").color(k.muted)).wrap());
                            ui.add(egui::Label::new(RichText::new("• Empty folders left behind are removed too.").color(k.muted)).wrap());
                        });
                        ui.checkbox(&mut self.tc_ack, RichText::new("I understand deleted temp files cannot be restored").strong());
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            if ui.add_enabled(self.tc_ack && self.admin, theme::danger_button("Delete temp files")).clicked() {
                                go = true;
                            }
                            if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(90.0, 30.0))).clicked() {
                                close = true;
                            }
                        });
                    });
                if go {
                    self.tc_start_clean(ctx);
                    close = true;
                }
            }
            TcAct::Schedule => {
                let locked = self.tc_result_now().map(|r| r.total().locked).unwrap_or_default();
                let files = locked.iter().filter(|l| !l.is_dir).count();
                let bytes: u64 = locked.iter().map(|l| l.bytes).sum();
                egui::Window::new("Finish cleanup at next restart")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .default_width(600.0)
                    .show(ctx, |ui| {
                        ui.add(egui::Label::new(RichText::new(format!("{} files in use ({}) will be added to Windows' delete-at-restart list.", fmt_n(files), fmt_bytes(bytes))).strong()).wrap());
                        ui.add(egui::Label::new("Windows deletes them very early in the next startup, before services and programs start. Nothing happens until you restart. Like the first pass, these files cannot be restored once Windows deletes them.").wrap());
                        ui.add(egui::Label::new(RichText::new("Until you restart, the pre-flight check will say a reboot is pending. You can cancel from this card before you restart.").color(k.muted)).wrap());
                        if expert {
                            ui.add(egui::Label::new(RichText::new("MoveFileExW(path, NULL, MOVEFILE_DELAY_UNTIL_REBOOT) → HKLM\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\PendingFileRenameOperations. Record: C:\\ProgramData\\Win10Tweaks\\tempclean-pending.json").small().monospace().color(k.muted)).wrap());
                        }
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            if ui.add_enabled(self.admin && files > 0, theme::primary_button("Schedule for next restart")).clicked() {
                                go = true;
                            }
                            if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(90.0, 30.0))).clicked() {
                                close = true;
                            }
                        });
                    });
                if go {
                    self.tc_scheduled = true;
                    self.start(ctx, "TempSchedule", move |log| tc::schedule(log, &locked, expert));
                    close = true;
                }
            }
            TcAct::CancelSchedule => {
                egui::Window::new("Cancel scheduled cleanup")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .default_width(560.0)
                    .show(ctx, |ui| {
                        ui.add(egui::Label::new("This removes only this app's entries from Windows' delete-at-restart list. Entries that Windows or other programs added (for example, an update finishing) are left exactly as they are.").wrap());
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            if ui.add_enabled(self.admin, theme::primary_button("Cancel scheduled cleanup")).clicked() {
                                go = true;
                            }
                            if ui.add(egui::Button::new("Keep it").min_size(egui::vec2(90.0, 30.0))).clicked() {
                                close = true;
                            }
                        });
                    });
                if go {
                    self.start(ctx, "TempCancelSchedule", tc::cancel_pending);
                    close = true;
                }
            }
        }
        if close {
            self.confirm = None;
            self.tc_ack = false;
        }
    }
}

/// Made-up numbers for the documentation screenshots (Linux preview build only shows these in --screenshot mode).
pub fn shot_samples(after_clean: bool) -> (Option<Report>, Option<Report>) {
    use std::path::PathBuf;
    use tc::{Locked, Root, RootKind, RootResult, Stats};
    let root = PathBuf::from(r"C:\Users\You\AppData\Local\Temp");
    let r = Root { kind: RootKind::CurrentUser, label: "Your temp folder (%TEMP%)".into(), path: root.clone() };
    let scan = Report {
        cleaned: false,
        opts: CleanOpts::default(),
        when: "2026-11-17 09:58".into(),
        roots: vec![RootResult {
            root: r.clone(),
            stats: Stats { eligible_files: 4812, eligible_bytes: 2_469_606_195, recent_files: 37, recent_bytes: 12_582_912, ..Default::default() },
        }],
        refused: vec![],
        preview_only: false,
    };
    if !after_clean {
        return (Some(scan), None);
    }
    let locked = ["LogiOptionsPlus\\logi_crashpad.lock", "LogiOptionsPlus\\agent.log", "MSTeams\\cache.db", "chrome_installer.log"]
        .iter()
        .enumerate()
        .map(|(i, n)| Locked { path: root.join(n), root: root.clone(), bytes: 1_048_576 * (i as u64 + 1) * 3, is_dir: false, reason: "in use by another program (sharing violation, os error 32)".into() })
        .collect::<Vec<_>>();
    let locked_bytes = locked.iter().map(|l| l.bytes).sum();
    let done = Report {
        cleaned: true,
        roots: vec![RootResult {
            root: r,
            stats: Stats { eligible_files: 4812, deleted_files: 4808, deleted_bytes: 2_439_606_195, deleted_dirs: 211, locked, locked_bytes, ..Default::default() },
        }],
        ..scan
    };
    (None, Some(done))
}
