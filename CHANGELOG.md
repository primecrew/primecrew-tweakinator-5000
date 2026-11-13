# Changelog

User-facing history for PrimeCrew Tweakinator 5000.

## 1.2.0

- Rebranded to PrimeCrew Tweakinator 5000. Publisher / CompanyName is PrimeCrew Software.
- The exe is now `PrimeCrewTweakinator5000.exe`.
- Full x64 installer (`PrimeCrewTweakinator5000-v1.2.0-x64-Setup.exe`) installs to `C:\Program Files\PrimeCrew Software\PrimeCrew Tweakinator 5000\`, puts a Start menu shortcut in the PrimeCrew Software folder, and registers an Add/Remove Programs entry (Publisher PrimeCrew Software, DisplayVersion 1.2.0, app icon, uninstaller). It requires administrator rights and is 64-bit only. An upgrade removes previous shortcuts first so they do not duplicate. Uninstalling does not revert applied tweaks and does not delete `C:\ProgramData\Win10Tweaks`. The PrimeCrew Software Start menu folder is removed only if it is empty; the parent Program Files folder is left in place if other apps are there.
- Portable zip `PrimeCrewTweakinator5000-v1.2.0.zip` ships alongside the installer in every release.
- Temporary-allow restore timer: when running from the installed location, the SYSTEM task points at the installed exe; a portable run still stages a copy at the previous fixed path so a timer created by an older build still fires. Restore update kill clears the task and the staged copy, and never deletes the installed exe.

## 1.1.0

- Version numbers in the app (window title, header, and Options › About), in the exe version info (`FileVersion` / `ProductVersion`), and in the portable zip name.
- Source published to a private GitHub repository. Built binaries are attached to Releases, not committed.

## 1.0.0 (unversioned at the time)

The first builds shipped without a version number in the app or the zip name. In order:

- Initial PowerShell kit and portable Rust (eframe/egui) GUI: Apply / Verify / Revert, UAC self-elevation (`requireAdministrator`), confirmation before apply, System Restore point on by default, and a copy-to-clipboard / export PowerShell fallback.
- UI restyle: System, Dark, Light, and High contrast themes, plus text size 75–300%.
- Hard Windows Update kill (policies, no phone home, forced services including a locked WaaSMedicSvc key, protected tasks, outbound firewall rules) plus Temporary allow updates and Restore kill (timed or until restored).
- Guided OneDrive removal wizard (files local first, stop sync, uninstall, block; files are never deleted), with beginner explanations and warnings, and an Expert mode toggle (default off) in Options.
- App previously released under an earlier name, with the app icon on the exe and in the header. Machine-specific name references removed so the app can be shared with friends.
