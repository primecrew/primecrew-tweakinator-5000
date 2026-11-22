//! In-app Help: topic list, beginner-friendly articles (catalog-driven for tweaks),
//! circled-? marks, and the About popup.
use crate::catalog::{Cat, Level, REG, DEAD_WSUS, FW_PREFIX, FW_SERVICES, GODMODE_NAME, SECURITY_NOTE, TEMP_LIFT_REG, TEMP_LIFT_SERVICES, WIN11_BUILD};
use crate::theme;
use eframe::egui::{self, RichText};
use std::cell::Cell;

thread_local! {
    static WANT: Cell<Option<&'static str>> = const { Cell::new(None) };
}

/// Ask the UI to open Help on this topic (consumed once per frame in `App::update`).
pub fn open(id: &'static str) {
    WANT.with(|c| c.set(Some(id)));
}

pub fn take() -> Option<&'static str> {
    WANT.with(|c| c.take())
}

pub const ID_START: &str = "getting-started";
pub const ID_EXPERT: &str = "beginner-expert";
pub const ID_ACTIONS: &str = "apply-verify-revert";
pub const ID_CLEANUP: &str = "tweak-cleanup";
pub const ID_UPDATES: &str = "tweak-updates";
pub const ID_TEMP: &str = "temp-allow";
pub const ID_NOTIF: &str = "tweak-notifications";
pub const ID_WIN11: &str = "tweak-blockwin11";
pub const ID_COPILOT: &str = "tweak-copilot";
pub const ID_SPOT: &str = "tweak-spotlight";
pub const ID_GOD: &str = "tweak-godmode";
pub const ID_ONEDRIVE: &str = "onedrive";
pub const ID_OD_CHECK: &str = "onedrive-check";
pub const ID_OD_STOP: &str = "onedrive-stop";
pub const ID_OD_REMOVE: &str = "onedrive-remove";
pub const ID_THEMES: &str = "themes";
pub const ID_PS: &str = "powershell";
pub const ID_LOGS: &str = "logs";
pub const ID_UNINSTALL: &str = "uninstall";
pub const ID_LIMITS: &str = "limits";
pub const ID_OPTIONS: &str = "options";
pub const ID_LOG_GUIDE: &str = "understanding-the-log";
pub const ID_PREFLIGHT: &str = "preflight";
pub const ID_TEMPCLEAN: &str = "clean-temp-files";

pub fn id_for(c: Cat) -> &'static str {
    match c {
        Cat::Cleanup => ID_CLEANUP,
        Cat::Updates => ID_UPDATES,
        Cat::Notifications => ID_NOTIF,
        Cat::BlockWin11 => ID_WIN11,
        Cat::Copilot => ID_COPILOT,
        Cat::Spotlight => ID_SPOT,
        Cat::GodMode => ID_GOD,
        Cat::OneDrive => ID_ONEDRIVE,
    }
}

pub fn tip_for(c: Cat) -> &'static str {
    match c {
        Cat::Cleanup => "Deletes leftover Windows Update download files. Off by default. Does not change your update settings. Click for details.",
        Cat::Updates => "Turns Windows Update off so it cannot phone home or turn itself back on. This also blocks security patches. Click for details.",
        Cat::Notifications => "Stops Windows nagging you to restart or install updates. Click for details.",
        Cat::BlockWin11 => "Keeps this PC on Windows 10 22H2 and blocks the Windows 11 upgrade. Click for details.",
        Cat::Copilot => "Removes the Copilot app and stops it coming back. Click for details.",
        Cat::Spotlight => "Turns off lock-screen ads, tips, the news feed and widgets. Click for details.",
        Cat::GodMode => "Adds a folder on your Desktop that lists almost every Windows setting in one place. Click for details.",
        Cat::OneDrive => "A guided wizard to stop OneDrive and remove it. Your files are never deleted. Click for details.",
    }
}

struct Topic {
    id: &'static str,
    title: &'static str,
    /// Static intro; registry lists and a few generated bits are appended in `body()`.
    prose: &'static str,
    cat: Option<Cat>,
}

const TOPICS: &[Topic] = &[
    Topic { id: ID_START, title: "Getting started", prose: START, cat: None },
    Topic { id: ID_EXPERT, title: "Beginner and Expert mode", prose: EXPERT, cat: None },
    Topic { id: ID_ACTIONS, title: "Apply, Verify and Revert", prose: ACTIONS, cat: None },
    Topic { id: ID_PREFLIGHT, title: "Pre-flight check", prose: PREFLIGHT, cat: None },
    Topic { id: ID_OPTIONS, title: "Apply options", prose: OPTIONS, cat: None },
    Topic { id: ID_CLEANUP, title: "SoftwareDistribution cleanup", prose: CLEANUP, cat: Some(Cat::Cleanup) },
    Topic { id: ID_UPDATES, title: "Hard Windows Update kill", prose: UPDATES, cat: Some(Cat::Updates) },
    Topic { id: ID_TEMP, title: "Temporary allow and Restore update kill", prose: TEMP, cat: None },
    Topic { id: ID_NOTIF, title: "Update notifications", prose: NOTIF, cat: Some(Cat::Notifications) },
    Topic { id: ID_WIN11, title: "Block the Windows 11 upgrade", prose: WIN11, cat: Some(Cat::BlockWin11) },
    Topic { id: ID_COPILOT, title: "Remove Copilot", prose: COPILOT, cat: Some(Cat::Copilot) },
    Topic { id: ID_SPOT, title: "Spotlight, tips, news and widgets", prose: SPOT, cat: Some(Cat::Spotlight) },
    Topic { id: ID_GOD, title: "God Mode folder", prose: GOD, cat: Some(Cat::GodMode) },
    Topic { id: ID_TEMPCLEAN, title: "Clean temp files", prose: TEMPCLEAN, cat: None },
    Topic { id: ID_ONEDRIVE, title: "OneDrive removal wizard", prose: ONEDRIVE, cat: Some(Cat::OneDrive) },
    Topic { id: ID_OD_CHECK, title: "OneDrive: check files", prose: OD_CHECK, cat: None },
    Topic { id: ID_OD_STOP, title: "OneDrive: stop sync", prose: OD_STOP, cat: None },
    Topic { id: ID_OD_REMOVE, title: "OneDrive: remove and undo", prose: OD_REMOVE, cat: Some(Cat::OneDrive) },
    Topic { id: ID_THEMES, title: "Themes and text size", prose: THEMES, cat: None },
    Topic { id: ID_PS, title: "PowerShell fallback", prose: POWERSHELL, cat: None },
    Topic { id: ID_LOG_GUIDE, title: "Understanding the log", prose: LOG_GUIDE, cat: None },
    Topic { id: ID_LOGS, title: "Logs and saved state", prose: LOGS, cat: None },
    Topic { id: ID_UNINSTALL, title: "Uninstalling the app", prose: UNINSTALL, cat: None },
    Topic { id: ID_LIMITS, title: "Known limits and safety", prose: LIMITS, cat: None },
];

fn topic(id: &str) -> &'static Topic {
    TOPICS.iter().find(|t| t.id == id).unwrap_or(&TOPICS[0])
}

fn level_plain(l: Level) -> &'static str {
    match l {
        Level::Core => "always applied",
        Level::BestEffort => "written, but Verify may report WARN on this Windows edition",
        Level::Hard => "only when “Updates: policy values only” is off",
    }
}

fn registry_lines(cat: Cat) -> String {
    let items: Vec<_> = REG.iter().filter(|i| i.cat == cat).collect();
    if items.is_empty() {
        return String::new();
    }
    let mut s = String::from("\n\nThese registry values are set (this list comes from the same catalog the app uses, so it stays in sync):\n");
    for i in items {
        let mut extra = level_plain(i.level).to_string();
        if i.min_build >= WIN11_BUILD {
            extra.push_str(", Windows 11 only");
        }
        if i.covered_by.is_some() {
            extra.push_str(", Windows may lock this value — the matching policy is what counts");
        }
        s.push_str(&format!("\n• {}\\{} = {} ({}, {})", i.path, i.name, i.val.text(), i.val.kind(), extra));
    }
    s
}

fn body(id: &str) -> String {
    let t = topic(id);
    let mut s = t.prose.to_string();
    // Keep SECURITY_NOTE in sync with catalog.rs.
    if id == ID_UPDATES || id == ID_LIMITS {
        s = s.replace("{SECURITY_NOTE}", SECURITY_NOTE);
    }
    if id == ID_UPDATES {
        s = s.replace("{DEAD_WSUS}", DEAD_WSUS);
        s = s.replace("{FW_RULES}", &FW_SERVICES.iter().map(|n| format!("{FW_PREFIX}{n}")).collect::<Vec<_>>().join(", "));
    }
    if id == ID_TEMP {
        s = s.replace("{TEMP_REG}", &TEMP_LIFT_REG.join(", "));
        s = s.replace("{TEMP_SVC}", &TEMP_LIFT_SERVICES.join(", "));
    }
    if id == ID_GOD {
        s = s.replace("{GODMODE_NAME}", GODMODE_NAME);
    }
    if let Some(cat) = t.cat {
        s.push_str(&registry_lines(cat));
    }
    s
}

/// Circled question-mark help mark. Hover shows `tip`; click opens Help on `topic`.
/// In Expert mode the mark is a little smaller and uses the muted colour until hovered.
pub fn mark(ui: &mut egui::Ui, tip: &str, topic_id: &'static str, expert: bool) -> egui::Response {
    let k = theme::t();
    let d = if expert { 14.0 } else { 18.0 };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(d, d), egui::Sense::click());
    let hovered = resp.hovered() || resp.has_focus();
    let color = if hovered { k.info } else if expert { k.muted } else { k.info };
    let stroke_w = if k.high_contrast { 1.8 } else if expert { 1.0 } else { 1.25_f32 };
    let radius = (d * 0.40).max(5.0);
    let painter = ui.painter();
    painter.circle_stroke(rect.center(), radius, egui::Stroke::new(stroke_w, color));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "?",
        egui::FontId::proportional(if expert { 9.0 } else { 11.0 }),
        color,
    );
    let resp = resp.on_hover_text(tip);
    if resp.clicked() {
        open(topic_id);
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Help"));
    resp
}

/// Small “Help” text button that opens a topic.
pub fn link(ui: &mut egui::Ui, topic_id: &'static str) -> egui::Response {
    let k = theme::t();
    let resp = ui.add(egui::Button::new(RichText::new("Help").small().color(k.info)).frame(false));
    if resp.clicked() {
        open(topic_id);
    }
    resp.on_hover_text("Open the Help topic for this setting")
}

pub fn show_window(ctx: &egui::Context, open: &mut bool, selected: &mut &'static str) {
    if !*open {
        return;
    }
    let mut stay = true;
    egui::Window::new("Help")
        .open(&mut stay)
        .collapsible(false)
        .resizable(true)
        .default_width(920.0)
        .default_height(620.0)
        .min_width(560.0)
        .min_height(360.0)
        .show(ctx, |ui| {
            // SidePanel + CentralPanel inside a Window lets CentralPanel fill the viewport
            // instead of the window, so the article is laid out in the remaining space here.
            egui::SidePanel::left("help-topics")
                .resizable(true)
                .default_width(240.0)
                .min_width(160.0)
                .show_inside(ui, |ui| {
                    ui.label(RichText::new("Topics").strong());
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().id_salt("help-topic-list").auto_shrink([false, false]).show(ui, |ui| {
                        for t in TOPICS {
                            if ui.selectable_label(*selected == t.id, t.title).clicked() {
                                *selected = t.id;
                            }
                        }
                    });
                });
            let t = topic(selected);
            ui.vertical(|ui| {
                ui.label(RichText::new(t.title).heading().strong());
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(6.0);
                egui::ScrollArea::vertical().id_salt("help-body").auto_shrink([false, false]).show(ui, |ui| {
                    ui.add(egui::Label::new(body(t.id)).wrap());
                });
            });
        });
    if !stay {
        *open = false;
    }
}

pub fn show_about(ctx: &egui::Context, icon: &egui::TextureHandle, open: &mut bool) {
    if !*open {
        return;
    }
    let k = theme::t();
    let mut close = false;
    let mut stay = true;
    egui::Window::new("About PrimeCrew Tweakinator 5000")
        .open(&mut stay)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(440.0)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add(egui::Image::new(icon).fit_to_exact_size(egui::vec2(72.0, 72.0)));
                ui.add_space(8.0);
                ui.label(RichText::new("PrimeCrew Tweakinator 5000").heading().strong());
                ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(k.muted));
                ui.label(RichText::new("by PrimeCrew Software").color(k.muted));
                ui.add_space(8.0);
                ui.add(egui::Label::new("A conservative Windows 10 tweak toolkit: nothing is applied silently, and every change can be verified or reverted.").wrap());
                ui.add_space(6.0);
                ui.add(egui::Label::new(RichText::new("Free software. Use at your own risk — read Help ▸ Known limits. Licensed under GPL v3 or later.").small().color(k.muted)).wrap());
                ui.add_space(12.0);
                if ui.add(theme::primary_button("Close")).clicked() {
                    close = true;
                }
            });
        });
    if close || !stay {
        *open = false;
    }
}

// ---------------------------------------------------------------------------
// Plain-language articles. Technical lists for tweaks are appended from REG.
// ---------------------------------------------------------------------------

const START: &str = "\
PrimeCrew Tweakinator 5000 is a small, offline toolkit for Windows 10. It is meant to be shared with trusted friends, so it is deliberately cautious.

Nothing on your PC is changed until you tick the tweaks you want, click Apply, read the list of exact changes, and confirm. Verify is always read-only: it only reports whether each setting is in place. Revert puts recorded originals back.

The app asks for Administrator rights (a UAC prompt) when you start it. Apply, Revert, Temporary allow, and the OneDrive wizard need that. If the header says you are not elevated, right-click the exe and choose Run as administrator.

This app contains no network code and sends no telemetry.

A typical first use:

1. Leave Beginner mode on (Expert mode is off by default, under Options in the menu).
2. Tick only the tweaks you actually want. SoftwareDistribution cleanup is off by default — leave it off unless you know you need it.
3. Leave “Create System Restore point before Apply” ticked.
4. Click Apply selected…, read the list, and confirm. If you chose the Windows Update kill, you must also tick that you understand security patches will be blocked.
5. Before anything is written, Apply shows a pre-flight check of this PC (administrator, UAC, antivirus, a pending reboot, and whether this is Windows 10). It only reports; it never changes those settings.
6. After Apply, a short summary tells you how many changes were made, how many were already done, and whether anything actually failed. If there were no real problems, it says “Everything worked.” Reboot, then click Verify.
7. Click Verify (read-only) and read the log. PASS is good. FAIL means something did not stick. WARN is often expected on Windows 10 Pro for a few policies. In the log, a line that mentions “Access denied” and is tagged SET is normal — Windows resisted and the app handled it. Only red ERROR lines need attention.

Circled question marks next to settings show a short explanation when you hover them. Click a mark, or a Help link, to open the matching topic here. Press F1 any time to open this Help window.

If the header warns that the app is running as a different account than the person signed in to Windows, per-user tweaks (and the OneDrive wizard) would affect the wrong account. The wizard is locked in that case. Close the app and start it again while signed in as that person, using an administrator account for them.
";

const EXPERT: &str = "\
Beginner mode is the default, and it is the right choice for most people.

In Beginner mode the OneDrive wizard starts with a welcome screen, explains each step in plain language, and shows extra tips. Circled question marks next to settings are a little larger and easier to see.

Expert mode is a toggle under Options in the menu bar. It is off by default, and your choice is saved in C:\\ProgramData\\Win10Tweaks\\ui.json.

In Expert mode the OneDrive wizard skips the welcome screen and the long explanations, and shows technical detail such as registry paths, attrib flags and file counts. Circled question marks are still there, but they are smaller and quieter.

Expert mode does not weaken safety. The files check that gates the OneDrive wizard, every confirmation dialog, the account-mismatch lock, and Apply / Verify / Revert all stay exactly the same.
";

const ACTIONS: &str = "\
Apply, Verify and Revert are the three main actions. They only touch the tweaks you have ticked, except for “Apply all” / “Revert all”.

Apply
• Shows a confirmation window with a read-only pre-flight check of this PC, then a list of every change that will be made (registry values, plus extra steps such as services or tasks).
• When Apply finishes, Beginner mode shows a summary: changes made, already done, and real problems, plus what to do next (reboot, then Verify). If there were no real problems, it says “Everything worked.”
• By default a System Restore point is created first. If that box is ticked and the restore point cannot be created, Apply stops and nothing else is written.
• The Windows Update kill also requires an “I understand” tick, because it blocks security and ESU patches.
• Apply all lists every tweak (except the OneDrive wizard, which is never part of Apply all) and needs a second confirmation tick.
• Every change is logged as old → new, and originals are recorded once in state.json so Revert can put them back.

Verify
• Read-only. It changes nothing.
• Reports PASS, FAIL or WARN for every setting, including the hard update-kill checks (services, no-connect policies, the WaaSMedicSvc key lock, and firewall rules).
• Use it after Apply, after a Windows update, and any time you want to see whether something has come back.

Revert
• Restores the originals recorded in state.json (or, if nothing was recorded, removes the values this app wrote / returns services and tasks to stock Windows 10 22H2 defaults).
• Cleanup cannot be meaningfully reverted: deleted cache files stay deleted, so it is left out of Revert selected / Revert all.
• Revert cannot reinstall an app it removed (for example Copilot). Use the Microsoft Store for that, after Temporary allow if the update kill is still on.
• OneDrive is not part of Revert all. Use Undo inside the OneDrive wizard instead.

Nothing runs silently. If a confirm window is open, or a job is already running, Apply and Revert wait.
";

const OPTIONS: &str = "\
These options sit in the Options card (and Expert mode is also under Options in the menu). They affect the next Apply, or how the wizard behaves.

Create System Restore point before Apply (recommended)
• On by default. Apply creates a restore point first and aborts if that fails while the box is ticked. It does not touch your files.

Updates: policy values only (leave services/tasks)
• A conservative form of the update kill. It writes the basic policy values (no automatic updates, no auto-reboot, hide Check for updates, Store AutoDownload) but does not apply the no-phone-home policies, does not disable services or tasks, and does not add firewall rules. Use this only if you want a lighter touch. Verify skips the hard-kill checks in this mode.

Copilot: keep Microsoft 365 (OfficeHub) app
• By default, removing Copilot also removes the Microsoft 365 (OfficeHub) app. Tick this to leave that app installed.

Cleanup: run even if a reboot is pending (not recommended)
• SoftwareDistribution cleanup normally refuses to run if a reboot is pending or TrustedInstaller is busy. This override lets it run anyway. Leave it off unless you know why you need it.

Expert mode is described in “Beginner and Expert mode”.
";

const CLEANUP: &str = "\
SoftwareDistribution cleanup is optional and unticked by default. It is a disk-cleanup step, not an update setting.

What it does
• Stops the wuauserv and BITS services temporarily.
• Deletes only these items under C:\\Windows\\SoftwareDistribution: the Download folder’s contents, PostRebootEventCache.V2, and ReportingEvents.log.
• Restarts whichever of those services were running.

What it does not do
• It never touches DataStore (the Windows Update database).
• It does not change update policies, services’ start types, or the update kill.

Risks
• It refuses to run if a reboot is pending or TrustedInstaller is running, because interrupting an install can leave Windows Update in a bad state. The “run even if a reboot is pending” override exists but is not recommended.
• Deleted cache files cannot be brought back by Revert. That is why this tweak is left out of Revert selected and Revert all. Windows can download updates again later if updates are allowed.

When you might want it
• The Windows Update download cache has grown large, or a failed update left junk in Download.

How to undo it
• You can’t restore the deleted files. If you only wanted to free disk space, you are done. Your other tweaks are unchanged.
";

const UPDATES: &str = "\
“Kill Windows Update hard” turns Windows Update off so it cannot quietly phone home, re-enable itself, or keep nagging you. This is a strong change. Read the risks before you apply it.

Why you might want it
• You want this PC to stay as it is, without automatic OS updates, Store updates, or Windows Update turning itself back on.

What it changes (when “policy values only” is off)
• Policies that disable automatic updates, disable auto-reboot with a user logged on, hide Check for updates, and set Store AutoDownload to 2 (do not automatically download).
• No phone home: DoNotConnectToWindowsUpdateInternetLocations and DisableWindowsUpdateAccess. Microsoft documents that both of these only apply when an intranet update server is set, so WUServer and WUStatusServer are pointed at {DEAD_WSUS} (a local address that serves nothing) and UseWUServer is turned on. Delivery Optimization is set to mode 99.
• Services wuauserv, UsoSvc, WaaSMedicSvc and uhssvc (if installed) are set to Disabled and stopped. If a normal write is refused, the app takes ownership of the WaaSMedicSvc service registry key only — never a file or binary — and adds a deny entry so SYSTEM cannot change values on that one key. The original owner and permissions are saved so Revert can put them back.
• Scheduled tasks under WaaSMedic, UpdateOrchestrator and WindowsUpdate are disabled. If Windows refuses the change even for an administrator, a temporary SYSTEM helper task is used, then removed.
• Outbound firewall rules block those update services: {FW_RULES}.

If you tick “Updates: policy values only”, only the basic policy values are written. The no-phone-home policies, services, tasks, key lock and firewall rules are skipped.

Risks
• {SECURITY_NOTE}
• Side effects can include a blank Settings ▸ Windows Update page, no Features on Demand / .NET 3.5 installs, and no Microsoft Store app installs or updates until you use Temporary allow.
• A manual .msu or a repair install / in-place upgrade can undo the kill (it rewrites services, tasks and the WaaSMedicSvc key). The SYSTEM-deny entry on that key can also make a manual cumulative update fail. Revert the Updates tweak before you install updates by hand, then re-apply the kill if you still want it.
• This app is unsigned, so SmartScreen may warn you, and the toolkit has not been tested on a real Windows PC yet.

How to undo it
• Tick this tweak and click Revert selected… (or Revert all…). That restores recorded policies, service start types, the WaaSMedicSvc key owner and permissions, tasks, and it deletes the firewall rules.
• To update Store apps for a short time without a full revert, use Temporary allow updates, then Restore update kill.
";

const TEMP: &str = "\
Temporary allow updates lifts just enough of the hard update kill so the Microsoft Store (and WinGet’s msstore source) can update apps such as PowerToys or ChatGPT. It is not a full revert.

What it lifts
• The no-connect and dummy-server policies ({TEMP_REG}).
• The Win10Tweaks-NoWU-* firewall rules.
• The wuauserv and UsoSvc services ({TEMP_SVC}).

What stays in place
• Automatic OS updates stay off (NoAutoUpdate remains).
• WaaSMedic stays disabled and locked.
• Delivery Optimization mode, update tasks, and uhssvc are not lifted.
• Copilot, Spotlight and the Windows 11 block are not touched.

Durations
• Until you click Restore update kill, or 30, 60 or 120 minutes.
• A timed allow registers a one-shot SYSTEM task (\\Win10Tweaks\\Win10Tweaks-RestoreWUKill) that runs this app with --restore-wu-kill, even if the window is closed. When the app is installed, that task points at the installed exe. A portable run stages a copy at a fixed Program Files path so SYSTEM never runs a user-writable file.
• State is stored in C:\\ProgramData\\Win10Tweaks\\tempallow.json. A red banner and a countdown show while the allow is active.

Restore update kill
• Re-applies the Updates tweak the same way Apply does (policies, forced services including WaaSMedic, tasks, firewall, no-phone-home), then clears the timer task and any staged portable copy. It never deletes the installed exe.

Tips
• WinGet packages from the winget source often update without lifting the kill. Microsoft Store / msstore apps need Temporary allow.
• If a timer is still running, click Restore update kill before you uninstall the app, because the timer runs the installed exe.

How to undo it
• Click Restore update kill, or wait for a timed allow to expire (the app will restore it if it is open; otherwise the SYSTEM task will).
";

const NOTIF: &str = "\
This tweak stops Windows Update from nagging you with restart and update notifications. It does not by itself turn updates off — use the Windows Update kill for that.

What it changes
• Policies that set the update notification level so balloons and restart nags are suppressed.
• A related “suggested settings” reminder (Scoobe) is turned off.
• The USO_UxBroker scheduled task, which is used for some update UI prompts, is disabled.

Risks
• You may not see a reminder when an update (if updates are allowed) wants a restart. That is the point of the tweak.

How to undo it
• Tick this tweak and click Revert selected…. Recorded policy values and the USO_UxBroker task are restored.
";

const WIN11: &str = "\
This tweak pins the PC to Windows 10, version 22H2 (the final Windows 10 release) and blocks a Windows 11 upgrade offer.

What it changes
• TargetReleaseVersion is turned on, ProductVersion is set to Windows 10, and TargetReleaseVersionInfo is set to 22H2.
• DisableOSUpgrade is set in Windows Update policy and in the Store policy.
• AllowOSUpgrade is set to 0.

Why you might want it
• You want to stay on Windows 10 and not be offered or pushed to Windows 11.

Risks
• You will not receive a Windows 11 upgrade through Windows Update (which is the goal). This does not by itself block all Windows 10 quality updates — the hard update kill does that.

How to undo it
• Tick this tweak and click Revert selected…. The recorded policy values are restored.
";

const COPILOT: &str = "\
This tweak removes the Copilot app and sets policies so it is less likely to come back.

What it changes
• Policies that turn off Windows Copilot for this user and for the machine, hide the taskbar Copilot button, and turn off the Edge Copilot sidebar.
• Running Copilot processes are stopped.
• The Microsoft.Copilot app is removed for all users and as a provisioned package. Unless you tick “Copilot: keep Microsoft 365 (OfficeHub) app”, the Microsoft 365 (OfficeHub) app is removed the same way.
• A Deprovisioned key is written so a later Windows or Store update is less likely to put Copilot back.

Risks
• Copilot (and, unless you opted out, the OfficeHub app) will be gone. Revert cannot reinstall those apps — you would use the Microsoft Store, after Temporary allow if the update kill is on.
• A later cumulative or Store update can sometimes put values back. If that happens, run Verify and Apply again (it is safe to re-apply).

How to undo it
• Tick this tweak and click Revert selected… to restore the policy values. Then, if you want the app itself back, install it from the Microsoft Store.
";

const SPOT: &str = "\
This tweak turns off Windows Spotlight on the lock screen, suggestion tips, the news feed, and widgets.

What it changes
• Per-user Content Delivery Manager values that control rotating lock-screen pictures, fun facts, and suggested apps and tips.
• CloudContent policies for Spotlight and consumer features. Microsoft scopes several of these to Enterprise and Education. On Windows 10 Pro the Content Delivery Manager values do the real work, and Verify may report WARN on those CloudContent policies — that is expected.
• Policies and settings for news and interests, widgets (including the taskbar widgets button), and search highlights. The Windows 11-only widgets values (the taskbar widgets button and widgets on the lock screen) are skipped on Windows 10, with no error. News and interests is turned off by a policy; a related per-user taskbar value is often locked by Windows and is not treated as a failure.

Risks
• The lock screen will no longer show Spotlight pictures and tips. The news/widgets flyout and the widgets button are turned off.
• There is no documented registry switch for the Windows 10 lock-screen Weather / Daily Wonder cards (2025 and later). A one-time manual fix: Settings ▸ Personalization ▸ Lock screen → set Background to Windows spotlight → set the status/widget app to None → set Background back to Picture.
• Some values can come back after a cumulative update. Verify, then re-apply if needed.

How to undo it
• Tick this tweak and click Revert selected…. Recorded values are restored. You may also want to revisit Settings ▸ Personalization ▸ Lock screen.
";

const GOD: &str = "\
God Mode is a built-in Windows feature, not a hidden hack. This tweak just creates the special folder on the signed-in account’s Desktop.

What it does
• Creates a folder named {GODMODE_NAME} on the Desktop of the account that is running this app.
• Opening it shows a long, grouped list of Control Panel and settings links.

Risks
• This is low risk. It is only a folder with a special name. You can delete the folder in File Explorer any time.
• If the app is running as a different administrator than the person signed in to Windows, the folder is created on the elevated account’s Desktop, not theirs. The header warns you when that happens.

How to undo it
• Tick this tweak and click Revert selected…, or just delete the folder from the Desktop. Revert removes the folder this app created.
";

const ONEDRIVE: &str = "\
The OneDrive removal wizard is a separate, guided flow. It is never part of Apply all or Revert all. Your files are never deleted. The OneDrive folder stays on this PC as an ordinary folder.

Use it when you want to stop using OneDrive on this PC and take the app off, without losing the files that are already here.

The wizard has three steps:

1. Check files — a read-only scan. It looks for known folders (Desktop, Documents, Pictures, and also Music, Videos, Downloads) that still point into OneDrive, and for cloud-only files (Windows attributes that mean “content is not on this PC”). Next stays off while a known folder is inside OneDrive or any file is cloud-only, unless you explicitly skip the check (with a warning and a required tick). A mismatch between the elevated account and the signed-in user can never be overridden; the wizard is locked.
2. Stop sync — you unlink this PC in OneDrive yourself. Then, after a confirm, the app closes OneDrive, removes its “run at sign-in” entry, and disables OneDrive scheduled tasks. All of that is recorded for Undo.
3. Remove — after a warning and a required tick: optional restore point, uninstall the app, write block policies, optionally hide the Explorer side-bar entry and stop new user accounts getting OneDrive. Then a finish screen explains Undo and asks you to restart.

Helpers on the check step: Open OneDrive, open the folder, Download all files to this PC (asks OneDrive to keep a local copy of every file), and Copy my folders back (copy only, never deletes; name conflicts are skipped and logged).

See the next topics for a step-by-step walkthrough, and “OneDrive: remove and undo” for how to put things back.

Tips
• Make sure you know your Microsoft account password before you start, in case you later want files that only live at OneDrive.com.
• Run the app as the person who uses the PC. If the header says it is running as a different account, the wizard stays locked on purpose.
";

const OD_CHECK: &str = "\
Step 1 makes sure your files are actually on this PC before anything else happens. Checking changes nothing.

What the scan looks at
• Whether OneDrive is installed and running, and which account is linked.
• Your OneDrive folders.
• Whether Desktop, Documents, Pictures (and Music, Videos, Downloads) still point into OneDrive.
• How many files are cloud-only — placeholders whose content is only online. The app recognises the Windows attributes for “offline”, “recall on open” and “recall on data access”.
• OneNote-style web-link files, which live only online. If you use OneNote, open each notebook and use File ▸ Export to save a copy here.

What you can do
• Open OneDrive (started as you, not as Administrator) and open your OneDrive folder in File Explorer.
• Download all files to this PC — after you confirm, the app asks OneDrive to “Always keep on this device” (attrib +P -U on each OneDrive folder) and then watches the cloud-only count. You need internet, free disk space, and OneDrive running.
• Copy my folders back — after you confirm, it copies Desktop / Documents / Pictures (and any other known folder still inside OneDrive) out to your normal user folders. It never moves or deletes anything from OneDrive. Files with the same name already in the destination are skipped and listed in the log. Folders that still have cloud-only files are skipped; download those first.
• The best way to take Desktop / Documents / Pictures out of OneDrive is often OneDrive’s own “Manage backup”: cloud icon near the clock ▸ gear ▸ Settings ▸ Sync and backup ▸ Manage backup, then choose to keep the files on this PC. Come back and click Check again.

The gate
• Next: Stop sync stays off while a known folder is still inside OneDrive or any file is still cloud-only.
• Beginners who are sure their files are safe somewhere else can skip that files check only through a separate warning with a required tick.
• If the app is running as the wrong Windows account, that is a hard block and cannot be skipped.

When the scan says all files are on this PC and no known folder is inside OneDrive, you can go on.
";

const OD_STOP: &str = "\
Step 2 stops OneDrive syncing on this PC. Your files stay exactly where they are.

Unlink this PC (you do this in OneDrive)
1. Click the OneDrive cloud icon near the clock (or press Open OneDrive in the wizard).
2. Click the gear, then Settings.
3. Open the Account tab and click Unlink this PC, then Unlink account.
4. Close the OneDrive sign-in window that appears. Don’t sign in again.

Unlinking only disconnects this PC. Your files stay on this PC and online at OneDrive.com. Click Check again afterwards. You can go on even if an account still shows as linked; removing OneDrive works either way.

Stop OneDrive now
• After a confirm (and an optional restore point), the app closes OneDrive, removes its “start when I sign in” entry, and disables scheduled tasks whose names start with OneDrive. Every change is recorded so Undo can put it back.
• Your files are not touched.

Then click Next: Remove, or Back if you want to return to the files check.
";

const OD_REMOVE: &str = "\
Step 3 uninstalls OneDrive and turns on Windows settings that stop it syncing or coming back. Your files in the OneDrive folder are not deleted.

You will be asked to confirm, and you must tick that you understand and that your files are safe.

What Remove does
• Optionally creates a System Restore point first.
• Closes OneDrive, removes the Run entry, and disables OneDrive tasks (the same as Stop, in case that was skipped).
• Uninstalls the app. It uses the registered uninstaller if present (per-user or all-users), otherwise the OneDriveSetup.exe in your user folder, then Program Files with /allusers, then the copy in System32 or SysWOW64, and only then winget as a last resort.
• Writes block policies so OneDrive cannot sync or quietly back up your folders again (Prevent the usage of OneDrive for file storage, block Known Folder Move opt-in, and prevent network traffic before sign-in, plus DisablePersonalSync for this user).
• Removes a leftover OneDriveSetup Run entry if present.
• Optionally stops new user accounts on this PC from getting OneDrive (edits the Default user hive, then unloads it).
• Optionally hides OneDrive from the File Explorer side bar.
• Every change is saved so Undo can put it back.

Afterward
• Please restart the PC to finish.
• The OneDrive folder is now an ordinary folder. You can delete it yourself later, once you are sure you have everything.
• To use OneDrive again later: click Undo OneDrive removal in this wizard, then install OneDrive from Microsoft and sign in. Undo restores every recorded setting, startup entry, task, side-bar entry and default-user value. It does not reinstall the OneDrive app, and it does not move folders back into OneDrive.

Verify (read-only) in the wizard footer shows the current OneDrive state in the log and changes nothing.
";

const THEMES: &str = "\
You can change the look of the app from the View menu, under Theme:

• System — follows Windows, and falls back to Dark if Windows does not say.
• Dark
• Light
• High contrast — black backgrounds, white text, yellow accents, thicker focus rings.

Text size (75% to 300%) is in the header, and also via Ctrl + / − / 0 or Ctrl+wheel. The log has its own font size control. The theme, text size, log size and Expert mode are saved in C:\\ProgramData\\Win10Tweaks\\ui.json so they come back next time.
";

const POWERSHELL: &str = "\
If you cannot use the GUI, the same tweaks are available as PowerShell.

From this app
• File ▸ Copy PowerShell script (or the button in the PowerShell fallback card) puts a self-contained script for the currently ticked tweaks on the clipboard. Paste it into an elevated PowerShell window. This pasted script does not record originals, so it is a poor way to undo — use the app’s Revert, or the exported kit’s Revert-Tweaks.ps1.
• Copy Apply-All-Tweaks.ps1 copies the kit’s apply script. That script expects the scripts\\ folder beside it; use Export PS kit if you need the whole set.
• Export PS kit next to exe writes the full kit (apply / verify / revert, plus the individual scripts, including Temporary allow and Remove-OneDrive) into a Win10Tweaks-PowerShell folder.

From the kit
• Double-click Apply-All-Tweaks.cmd for a UAC prompt, or run the scripts from an elevated PowerShell window. -WhatIf is a dry run.
• scripts\\Set-UpdateWindow.ps1 -Allow / -Restore is Temporary allow / Restore kill (the script has no timer).
• scripts\\Remove-OneDrive.ps1 is the wizard’s script equivalent (a read-only check by default).

The PowerShell kit and the GUI share the same state.json format and the same setting paths.
";

const TEMPCLEAN: &str = "\
Clean temp files frees disk space by deleting leftover temporary files. Programs and installers put files in a temp folder while they work and often forget to delete them. Over months this can add up to gigabytes.

It is its own card on the main window. It is not a tweak, and it is never part of Apply all.

How it works
1. Click Scan temp files. This is read-only. It counts the files that could be cleaned and how much space they use, for example “4,812 files, 2.3 GB can be cleaned”.
2. Click Clean temp files…, read the confirm window, tick “I understand deleted temp files cannot be restored”, and click Delete temp files.
3. Files that another program is using right now (mouse or keyboard software, Teams, a browser, an installer that is still running) cannot be deleted while Windows is running. They are left alone and counted. That is normal and is not an error.
4. If some files were in use, you can click Finish cleanup at next restart…. The app hands just those files to Windows' own delete-at-restart list. Windows deletes them very early in the next startup, before services and programs start and grab them again. Nothing happens until you restart.
5. After the restart, the card shows how many of those files were cleared and how much space came back. Click Dismiss when you have read it.

What is cleaned
• By default (Beginner and Expert mode): your own temp folder (%TMP% / %TEMP%, normally C:\\Users\\<you>\\AppData\\Local\\Temp) and the Windows temp folder, C:\\Windows\\Temp.
• Your %TMP% / %TEMP% is cleaned only when it is the standard AppData\\Local\\Temp folder. If it points to a custom folder (for example D:\\Scratch or C:\\Temp), it is left alone and the card shows a “Not used” note.
• Expert mode can also add the temp folders of other user accounts on this PC, or leave out C:\\Windows\\Temp.

Safety rules
• Deleted temp files cannot be restored. They do not go to the Recycle Bin, and there is no Revert. The confirm window says so and needs a tick.
• Files and folders changed in the last 24 hours are left alone, so a half-finished install or update is not disturbed. Expert mode can change this to 1 hour, 3 days, or 7 days.
• Only files inside the temp folders are deleted. Shortcuts, symbolic links and junctions are never followed and never deleted, so nothing outside the temp folder can be reached.
• A folder is only used if its name is Temp (or Tmp) and it is not the top of a drive, and your own temp folder only if it is the standard AppData\\Local\\Temp. Anything else gets a “Not used” note and is left alone.
• The folder this app is running from is never touched (for example, if you opened the exe straight from a zip file).
• Empty folders left behind are removed, except recently changed ones.
• The scan is only a preview. Clean looks at the folders again and applies the same rules at the moment it runs.

Finish cleanup at next restart
• Uses MoveFileEx with MOVEFILE_DELAY_UNTIL_REBOOT, which adds entries to HKLM\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\PendingFileRenameOperations. This needs administrator rights.
• Each file is checked again first: still there, not a link, and inside a temp folder.
• What was scheduled is recorded in C:\\ProgramData\\Win10Tweaks\\tempclean-pending.json, so the card can report the result after the restart. When you click Dismiss it is kept as tempclean-last.json.
• Until you restart, the pre-flight check says “Windows has file renames waiting for a reboot”. That is expected, and it means the SoftwareDistribution cleanup tweak waits until after the restart.
• Cancel scheduled cleanup… removes only this app's entries from that list. Entries from Windows Update or other programs are left exactly as they are.
• A file a program opens again very early in startup can survive. The card counts those as “still there”.

In the log
• Each folder gets one line: how many files were deleted and how much space was freed, plus how many were in use and left alone.
• In Beginner mode the names of files that were in use are written to the log file only. Expert mode also shows them on screen, with the Windows reason (for example, “sharing violation”).
";

const LOGS: &str = "\
Everything the app writes is logged, and a small amount of state is saved so Revert and the wizard can undo their work.

In Beginner mode a short legend sits at the top of the on-screen log: “'Access denied' followed by SET means Windows resisted and the app handled it. Only red ERROR lines need attention.” See “Understanding the log” for a fuller explanation.

Locations (all under C:\\ProgramData\\Win10Tweaks\\)
• Logs\\ — one log file per Apply, Verify, Revert, Temporary allow, and wizard job. File ▸ Open logs folder (or the button on the log panel) opens this folder. You can also Copy or Clear the on-screen log; that does not delete the files.
• state.json — recorded originals and other undo information. Shared with the PowerShell kit.
• ui.json — theme, text size, log size, and Expert mode.
• tempallow.json — whether a temporary update allow is active, and when it expires.
• tempclean-pending.json — temp files scheduled for deletion at the next restart (kept as tempclean-last.json after you dismiss the result).

Scheduled task and firewall rule names still use the Win10Tweaks-* prefix, so this release can manage tweaks applied by earlier builds.

Uninstalling the app does not delete this folder. That is on purpose: Revert and your logs remain available if you install the app again, and your tweaks are left as they were.
";

const UNINSTALL: &str = "\
Uninstalling this app does not undo any tweaks you applied. It also does not delete C:\\ProgramData\\Win10Tweaks (logs, recorded originals, and backups stay).

Before you uninstall
• If you want your previous Windows settings back, run the app and click Revert first (and use Undo in the OneDrive wizard if you used that).
• If a Temporary allow timer is still running, click Restore update kill first. The timer’s SYSTEM task runs the installed exe; uninstalling without restoring can leave that task pointing at a missing file.

How to uninstall
• Use Add/Remove Programs, or uninstall.exe in the install folder (C:\\Program Files\\PrimeCrew Tweakinator 5000\\ for new installs; installs from 1.5.2 or earlier stay in C:\\Program Files\\PrimeCrew Software\\PrimeCrew Tweakinator 5000\\).
• That removes the app, its Start menu shortcut, and the Add/Remove Programs entry. The PrimeCrew Software Start menu folder is removed only if it is empty. For an older install in Program Files\\PrimeCrew Software, that parent folder is removed only if it is empty.

The portable zip has no uninstaller: delete the folder you unzipped. The same rule applies — deleting the exe does not revert tweaks. If a portable temporary-allow timer is still running, click Restore update kill first.
";

const LIMITS: &str = "\
Please read these before you rely on the hard update kill or share the app.

• {SECURITY_NOTE} Applying the Updates kill requires an explicit “I understand” tick.

• After Apply, click Verify, and keep the default restore-point option on.

• The exe is unsigned, so SmartScreen may say “unknown publisher”. Use More info ▸ Run anyway only if you trust the copy you were given.

• A manual .msu, or a repair install / in-place upgrade, can undo the Updates kill because it rewrites services, tasks and the WaaSMedicSvc key permissions. The SYSTEM-deny entry on that key can also make a manual cumulative update fail. Revert the Updates tweak before you install updates by hand, then re-apply the kill if you still want it. Run Verify afterwards.

• Run the app as the account that uses the PC. Per-user settings go to the account that elevates. If you elevate with a different admin account, the header shows a warning and the OneDrive wizard is locked.

• Revert cannot reinstall removed apps. Cleanup cannot bring back deleted cache files, and Clean temp files cannot bring back deleted temp files. Typical resets after a cumulative or Store update: Copilot or Spotlight values come back — Verify, then re-apply.

• Temporary allow is the supported way to update Store apps while the kill is in place. Click Restore update kill when you are done, and before uninstalling if a timer is still running.
";

const PREFLIGHT: &str = "\
Before Apply writes anything, the confirmation window shows a read-only pre-flight check of this PC. The same checklist also appears on the main window, under This PC. Nothing in this check changes Windows settings. This app never changes User Account Control.

What it looks at

Administrator rights
• Is this app running as administrator? Apply, Revert, Temporary allow, and the OneDrive wizard need that. If it is not elevated, use Restart as administrator — Windows will show a UAC prompt. The app already asks for administrator rights when it starts; this button is a fallback if that did not happen.
• Is the person signed in to Windows an administrator? If that account is a standard user, the UAC prompt will ask for an administrator’s password. Per-user tweaks would then go to that administrator’s account, not to the person sitting at the PC.

User Account Control (info only)
• The app reports the current UAC level (for example, the default “prompt for consent for non-Windows programs”). The UAC level does not change what this app can do once it is already running as administrator. The app never turns UAC up or down.

Antivirus
• Microsoft Defender Tamper Protection, if it is on, does not block the tweaks this app makes. That is just information.
• A third-party antivirus can sometimes block or quarantine this app or the changes it writes. If one is detected, consider adding PrimeCrewTweakinator5000.exe to that product’s allow list (sometimes called exclusions or whitelist) before you Apply. The app never changes your antivirus settings.

Wrong Windows account
• If the elevated account is not the person signed in to Windows, per-user tweaks (HKCU, the Desktop, and the OneDrive wizard) would affect the wrong profile. The OneDrive wizard stays locked in that case. Close the app and start it again while signed in as that person, with an administrator account for them.

Pending reboot
• If Windows is waiting for a reboot (for example after an update), finish that reboot first. A pending reboot can make service and file changes fail, and SoftwareDistribution cleanup will refuse to run unless you override it.

Windows version
• This toolkit is built for Windows 10. If it is running on Windows 11, you will see a warning. You can still continue, but some settings differ, and Windows 11-only values are used only there.

Every item has a Help link. If something in the list looks wrong, read the matching topic and fix it yourself — the check will not change it for you.
";

const LOG_GUIDE: &str = "\
The log records every change as old → new. Colours and tags tell you what happened.

Tags
• SET (green) — a change was made, or Windows refused at first and the app still completed the change another way.
• SKIP (grey) — already the way it should be, not present, or not applicable on this PC (for example a Windows 11-only setting on Windows 10).
• INFO (blue) — context, not a problem.
• WARN (amber) — worth reading, but not a failed change. A few policies only apply on Enterprise / Education, so WARN can be expected on Windows 10 Pro.
• ERROR or FAIL (red) — something that should have worked did not. These are the lines that need your attention.

“Access denied” is often normal
Windows protects some update tasks and the WaaSMedic service. The app is designed to handle that:
• A task line that says it was disabled because Windows protects the task (using the built-in SYSTEM helper) is a successful SET, not a failure.
• A service line that says it was set via the registry because Windows protects the service is also a successful SET. That one takes effect after a reboot.
• If you still see the raw “Access is denied” text, it is extra technical detail (always in the log file; in Expert mode it is also shown on screen, quieter).

The same is true for a leftover Store app entry that is already gone (SKIP: old leftover entry, nothing to remove), and for a News and interests setting that Windows locks (SKIP: the policy already turns it off).

In Beginner mode a short legend sits at the top of the log: “'Access denied' followed by SET means Windows resisted and the app handled it. Only red ERROR lines need attention.”

When Apply finishes, Beginner mode also shows a summary with counts of changes made, already done, and real problems. If the real-problem count is 0, it says “Everything worked.” For any real error it gives a one-line explanation. Reboot, then click Verify.

Log files live in C:\\ProgramData\\Win10Tweaks\\Logs. File ▸ Open logs folder opens that folder. The on-screen Copy button copies what you see (in Expert mode that includes the technical detail).
";

