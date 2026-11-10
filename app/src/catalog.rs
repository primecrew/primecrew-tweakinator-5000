//! Single source of truth for every value the app writes. Mirrors scripts\TweakCommon.ps1 Get-TweakManifest
//! (same path strings, so state.json is shared with the PowerShell kit).

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Cat {
    Cleanup,
    Updates,
    Notifications,
    BlockWin11,
    Copilot,
    Spotlight,
    GodMode,
    /// OneDrive removal wizard only. Deliberately NOT in Cat::ALL: Apply all / Revert all never touch OneDrive.
    OneDrive,
}

impl Cat {
    pub const ALL: [Cat; 7] = [
        Cat::Cleanup,
        Cat::Updates,
        Cat::Notifications,
        Cat::BlockWin11,
        Cat::Copilot,
        Cat::Spotlight,
        Cat::GodMode,
    ];
    /// Name used in state.json "Category" (matches the PowerShell kit).
    pub fn id(self) -> &'static str {
        match self {
            Cat::Cleanup => "Cleanup",
            Cat::Updates => "Updates",
            Cat::Notifications => "Notifications",
            Cat::BlockWin11 => "BlockWin11",
            Cat::Copilot => "Copilot",
            Cat::Spotlight => "Spotlight",
            Cat::GodMode => "GodMode",
            Cat::OneDrive => "OneDrive",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Cat::Cleanup => "SoftwareDistribution cleanup (Download, PostRebootEventCache.V2, ReportingEvents.log)",
            Cat::Updates => "Kill Windows Update hard (policies + no-connect + services incl. WaaSMedic + tasks + firewall)",
            Cat::Notifications => "Disable Windows Update notifications / restart nags",
            Cat::BlockWin11 => "Block Windows 11 upgrade (pin Windows 10 22H2)",
            Cat::Copilot => "Remove Copilot and keep it gone",
            Cat::Spotlight => "Remove Spotlight, lock-screen tips, news feed, widgets",
            Cat::GodMode => "Create God Mode folder on Desktop",
            Cat::OneDrive => "OneDrive removal (wizard only)",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Val {
    D(u32),
    S(&'static str),
}

impl Val {
    pub fn kind(self) -> &'static str {
        match self {
            Val::D(_) => "DWord",
            Val::S(_) => "String",
        }
    }
    pub fn text(self) -> String {
        match self {
            Val::D(v) => v.to_string(),
            Val::S(s) => s.to_string(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    /// Expected to work on Windows 10 Pro 22H2.
    Core,
    /// Written, but Microsoft scopes it to another edition/OS (Verify reports WARN, not FAIL).
    BestEffort,
    /// Updates "hard kill" layer: written only when 'policy only' is OFF (Verify skips it in policy-only mode).
    Hard,
}

#[derive(Clone, Copy, Debug)]
pub struct RegItem {
    pub cat: Cat,
    pub path: &'static str,
    pub name: &'static str,
    pub val: Val,
    pub level: Level,
}

const WU: &str = r"HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate";
const AU: &str = r"HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU";
const STORE: &str = r"HKLM:\SOFTWARE\Policies\Microsoft\WindowsStore";
const CDM: &str = r"HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";
const CCU: &str = r"HKCU:\Software\Policies\Microsoft\Windows\CloudContent";
const CCM: &str = r"HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent";
const ADV: &str = r"HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const DSH: &str = r"HKLM:\SOFTWARE\Policies\Microsoft\Dsh";

macro_rules! r {
    ($c:ident, $p:expr, $n:expr, $v:expr, $l:ident) => {
        RegItem { cat: Cat::$c, path: $p, name: $n, val: $v, level: Level::$l }
    };
}

pub const REG: &[RegItem] = &[
    // Updates
    r!(Updates, AU, "NoAutoUpdate", Val::D(1), Core),
    r!(Updates, AU, "AUOptions", Val::D(1), Core),
    r!(Updates, AU, "NoAutoRebootWithLoggedOnUsers", Val::D(1), Core),
    r!(Updates, WU, "SetDisableUXWUAccess", Val::D(1), Core),
    r!(Updates, STORE, "AutoDownload", Val::D(2), Core),
    // Updates hard layer (no phone home). DoNotConnect / DisableWindowsUpdateAccess only apply when an intranet
    // update service is configured (Microsoft Learn), so WU is pointed at a local address that serves nothing.
    r!(Updates, WU, "DoNotConnectToWindowsUpdateInternetLocations", Val::D(1), Hard),
    r!(Updates, WU, "DisableWindowsUpdateAccess", Val::D(1), Hard),
    r!(Updates, WU, "WUServer", Val::S(DEAD_WSUS), Hard),
    r!(Updates, WU, "WUStatusServer", Val::S(DEAD_WSUS), Hard),
    r!(Updates, AU, "UseWUServer", Val::D(1), Hard),
    r!(Updates, r"HKLM:\SOFTWARE\Policies\Microsoft\Windows\DeliveryOptimization", "DODownloadMode", Val::D(99), Hard),
    // Notifications
    r!(Notifications, WU, "SetUpdateNotificationLevel", Val::D(1), Core),
    r!(Notifications, WU, "UpdateNotificationLevel", Val::D(2), Core),
    r!(Notifications, r"HKLM:\SOFTWARE\Microsoft\WindowsUpdate\UX\Settings", "RestartNotificationsAllowed2", Val::D(0), BestEffort),
    r!(Notifications, r"HKCU:\Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement", "ScoobeSystemSettingEnabled", Val::D(0), Core),
    // Block Windows 11
    r!(BlockWin11, WU, "TargetReleaseVersion", Val::D(1), Core),
    r!(BlockWin11, WU, "ProductVersion", Val::S("Windows 10"), Core),
    r!(BlockWin11, WU, "TargetReleaseVersionInfo", Val::S("22H2"), Core),
    r!(BlockWin11, WU, "DisableOSUpgrade", Val::D(1), Core),
    r!(BlockWin11, r"HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\OSUpgrade", "AllowOSUpgrade", Val::D(0), Core),
    r!(BlockWin11, STORE, "DisableOSUpgrade", Val::D(1), Core),
    // Copilot
    r!(Copilot, r"HKCU:\Software\Policies\Microsoft\Windows\WindowsCopilot", "TurnOffWindowsCopilot", Val::D(1), Core),
    r!(Copilot, r"HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsCopilot", "TurnOffWindowsCopilot", Val::D(1), Core),
    r!(Copilot, ADV, "ShowCopilotButton", Val::D(0), Core),
    r!(Copilot, r"HKLM:\SOFTWARE\Policies\Microsoft\Edge", "HubsSidebarEnabled", Val::D(0), Core),
    // Spotlight / suggestions (ContentDeliveryManager, per user)
    r!(Spotlight, CDM, "RotatingLockScreenEnabled", Val::D(0), Core),
    r!(Spotlight, CDM, "RotatingLockScreenOverlayEnabled", Val::D(0), Core),
    r!(Spotlight, CDM, "SubscribedContent-338387Enabled", Val::D(0), Core),
    r!(Spotlight, CDM, "SubscribedContent-338388Enabled", Val::D(0), Core),
    r!(Spotlight, CDM, "SubscribedContent-338389Enabled", Val::D(0), Core),
    r!(Spotlight, CDM, "SubscribedContent-338393Enabled", Val::D(0), Core),
    r!(Spotlight, CDM, "SubscribedContent-353694Enabled", Val::D(0), Core),
    r!(Spotlight, CDM, "SubscribedContent-353696Enabled", Val::D(0), Core),
    r!(Spotlight, CDM, "SubscribedContent-310093Enabled", Val::D(0), Core),
    r!(Spotlight, CDM, "SystemPaneSuggestionsEnabled", Val::D(0), Core),
    r!(Spotlight, CDM, "SoftLandingEnabled", Val::D(0), Core),
    r!(Spotlight, CDM, "SilentInstalledAppsEnabled", Val::D(0), Core),
    r!(Spotlight, CDM, "PreInstalledAppsEnabled", Val::D(0), Core),
    r!(Spotlight, CDM, "OemPreInstalledAppsEnabled", Val::D(0), Core),
    // CloudContent policies (Enterprise/Education-scoped per Microsoft)
    r!(Spotlight, CCU, "DisableWindowsSpotlightFeatures", Val::D(1), BestEffort),
    r!(Spotlight, CCU, "ConfigureWindowsSpotlight", Val::D(2), BestEffort),
    r!(Spotlight, CCU, "DisableWindowsSpotlightOnActionCenter", Val::D(1), BestEffort),
    r!(Spotlight, CCU, "DisableWindowsSpotlightOnSettings", Val::D(1), BestEffort),
    r!(Spotlight, CCU, "DisableWindowsSpotlightWindowsWelcomeExperience", Val::D(1), BestEffort),
    r!(Spotlight, CCU, "DisableThirdPartySuggestions", Val::D(1), BestEffort),
    r!(Spotlight, CCU, "DisableTailoredExperiencesWithDiagnosticData", Val::D(1), BestEffort),
    r!(Spotlight, CCM, "DisableWindowsConsumerFeatures", Val::D(1), BestEffort),
    r!(Spotlight, CCM, "DisableSoftLanding", Val::D(1), BestEffort),
    r!(Spotlight, CCM, "DisableCloudOptimizedContent", Val::D(1), BestEffort),
    // News and interests / widgets / search highlights
    r!(Spotlight, r"HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Feeds", "EnableFeeds", Val::D(0), Core),
    r!(Spotlight, r"HKCU:\Software\Microsoft\Windows\CurrentVersion\Feeds", "ShellFeedsTaskbarViewMode", Val::D(2), BestEffort),
    r!(Spotlight, DSH, "AllowNewsAndInterests", Val::D(0), BestEffort),
    r!(Spotlight, DSH, "DisableWidgetsOnLockScreen", Val::D(1), BestEffort),
    r!(Spotlight, ADV, "TaskbarDa", Val::D(0), BestEffort),
    r!(Spotlight, r"HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Search", "EnableDynamicContentInWSB", Val::D(0), Core),
    // OneDrive blocks (written only by the wizard's Remove step; Microsoft Learn: OneDrive GPOs + Policy CSP System/DisableOneDriveFileSync)
    r!(OneDrive, OD_WINPOL, "DisableFileSyncNGSC", Val::D(1), Core),
    r!(OneDrive, OD_POL_CU, "DisablePersonalSync", Val::D(1), Core),
    r!(OneDrive, OD_POL_LM, "KFMBlockOptIn", Val::D(1), Core),
    r!(OneDrive, OD_POL_LM, "PreventNetworkTrafficPreUserSignIn", Val::D(1), Core),
];

// ---------------- OneDrive wizard ----------------
/// "Prevent the usage of OneDrive for file storage" (SkyDrive.admx): no sync, hidden from the Explorer nav pane, no file picker.
pub const OD_WINPOL: &str = r"HKLM:\SOFTWARE\Policies\Microsoft\Windows\OneDrive";
/// OneDrive.admx machine policies (KFMBlockOptIn, PreventNetworkTrafficPreUserSignIn).
pub const OD_POL_LM: &str = r"HKLM:\SOFTWARE\Policies\Microsoft\OneDrive";
/// OneDrive.admx user policies (DisablePersonalSync lives HERE, not under HKCU\Software\Microsoft\OneDrive).
pub const OD_POL_CU: &str = r"HKCU:\SOFTWARE\Policies\Microsoft\OneDrive";
pub const OD_RUN_CU: &str = r"HKCU:\Software\Microsoft\Windows\CurrentVersion\Run";
/// Mount point used while the Default-user hive (C:\Users\Default\NTUSER.DAT) is loaded; unloaded right after.
pub const OD_DEFAULT_HIVE: &str = r"HKU\Win10Tweaks_DefaultUser";
pub const OD_NAV_CLSID: &str = "{018D5C66-4533-4307-9B53-224DE2ED1FE6}";
pub const OD_NAV_VALUE: &str = "System.IsPinnedToNameSpaceTree";
/// Every place the OneDrive nav-pane CLSID can live (per-user install = HKCU, per-machine = HKLM; 64- and 32-bit views).
pub const OD_NAV_KEYS: &[&str] = &[
    r"HKCU:\Software\Classes\CLSID\{018D5C66-4533-4307-9B53-224DE2ED1FE6}",
    r"HKCU:\Software\Classes\WOW6432Node\CLSID\{018D5C66-4533-4307-9B53-224DE2ED1FE6}",
    r"HKLM:\SOFTWARE\Classes\CLSID\{018D5C66-4533-4307-9B53-224DE2ED1FE6}",
    r"HKLM:\SOFTWARE\Classes\WOW6432Node\CLSID\{018D5C66-4533-4307-9B53-224DE2ED1FE6}",
];
/// Uninstall registrations written by OneDriveSetup (per-user HKCU; per-machine HKLM 64-bit or WOW6432Node).
pub const OD_UNINSTALL_KEYS: &[(&str, bool)] = &[
    (r"HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\OneDriveSetup.exe", false),
    (r"HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\OneDriveSetup.exe", true),
    (r"HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\OneDriveSetup.exe", true),
];
pub const USER_SHELL_FOLDERS: &str = r"HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders";
/// File attributes that mean "content is not on this PC" (cloud-only placeholder).
pub const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
pub const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x0004_0000;
pub const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
pub const CLOUD_ONLY_MASK: u32 = FILE_ATTRIBUTE_OFFLINE | FILE_ATTRIBUTE_RECALL_ON_OPEN | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS;

/// (service, stock Win10 22H2 Start used only when no original was recorded, stock DelayedAutostart)
/// uhssvc = Microsoft Update Health Service (KB4023057); skipped when not installed.
pub const SERVICES: &[(&str, u32, Option<u32>)] = &[
    ("WaaSMedicSvc", 3, None),
    ("uhssvc", 2, Some(1)),
    ("UsoSvc", 2, Some(1)),
    ("wuauserv", 3, None),
];

/// Local, non-routable "intranet update server": nothing listens there, so WU has nowhere to connect.
pub const DEAD_WSUS: &str = "http://127.0.0.1:8530";
/// Service keys whose ACL we take over (owner -> Administrators, + deny SYSTEM "Set Value") so Windows can't flip Start back.
/// Original owner + SDDL are recorded in state.json and restored by Revert.
pub const LOCK_SERVICES: &[&str] = &["WaaSMedicSvc"];
pub const LOCK_DENY_ACE: &str = "(D;;0x2;;;SY)";
pub const ADMIN_FULL_ACE: &str = "(A;;KA;;;BA)";
/// Outbound block rules, one per service; Revert / Temporarily allow delete every rule with this name prefix.
pub const FW_PREFIX: &str = "Win10Tweaks-NoWU-";
pub const FW_SERVICES: &[&str] = &["wuauserv", "UsoSvc", "WaaSMedicSvc", "uhssvc"];
/// Temporarily allow updates lifts only these (Store / WinGet msstore need wuauserv + UsoSvc + WU endpoints).
/// NoAutoUpdate / AUOptions / SetDisableUXWUAccess, WaaSMedicSvc, uhssvc, tasks and DO mode stay as they are.
pub const TEMP_LIFT_REG: &[&str] = &["DoNotConnectToWindowsUpdateInternetLocations", "DisableWindowsUpdateAccess", "WUServer", "WUStatusServer", "UseWUServer"];
pub const TEMP_LIFT_SERVICES: &[&str] = &["wuauserv", "UsoSvc"];
pub const RESTORE_TASK_PATH: &str = r"\Win10Tweaks\";
pub const RESTORE_TASK_NAME: &str = "Win10Tweaks-RestoreWUKill";
pub const RESTORE_FLAG: &str = "--restore-wu-kill";

pub const UPDATE_TASK_FOLDERS: &[&str] = &[
    r"\Microsoft\Windows\WaaSMedic\",
    r"\Microsoft\Windows\UpdateOrchestrator\",
    r"\Microsoft\Windows\WindowsUpdate\",
];

pub const COPILOT_PROCS: &[&str] = &["Copilot.exe", "Microsoft.Copilot.exe", "M365Copilot.exe"];
pub const PKG_COPILOT: (&str, &str) = ("Microsoft.Copilot", "Microsoft.Copilot_8wekyb3d8bbwe");
pub const PKG_M365: (&str, &str) = ("Microsoft.MicrosoftOfficeHub", "Microsoft.MicrosoftOfficeHub_8wekyb3d8bbwe");
pub const DEPROV_ROOT: &str = r"HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Appx\AppxAllUserStore\Deprovisioned";

pub const GODMODE_NAME: &str = "God Mode.{ED7BA470-8E54-465E-825C-99712043E01C}";

pub const SECURITY_NOTE: &str = "Windows 10 is past end of support (Oct 14, 2025); blocking Windows Update also blocks security and consumer-ESU patches (ESU runs to Oct 12, 2027), Microsoft Store app installs/updates, and Defender definitions via Windows Update, leaving this PC exposed to unpatched vulnerabilities.";
