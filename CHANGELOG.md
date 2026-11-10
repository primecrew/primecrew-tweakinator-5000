# Changelog

User-facing history for PrimeCrew Tweakinator 5000.

## 1.1.0

- Version numbers in the app (window title, header, and Options › About), in the exe version info (`FileVersion` / `ProductVersion`), and in the portable zip name (`PrimeCrewTweakinator5000-v1.1.0.zip`).
- Source published to a private GitHub repository. Built binaries are attached to Releases, not committed.

## 1.0.0 (unversioned at the time)

The first builds shipped without a version number in the app or the zip name. In order:

- Initial PowerShell kit and portable Rust (eframe/egui) GUI: Apply / Verify / Revert, UAC self-elevation (`requireAdministrator`), confirmation before apply, System Restore point on by default, and a copy-to-clipboard / export PowerShell fallback.
- UI restyle: System, Dark, Light, and High contrast themes, plus text size 75–300%.
- Hard Windows Update kill (policies, no phone home, forced services including a locked WaaSMedicSvc key, protected tasks, outbound firewall rules) plus Temporary allow updates and Restore kill (timed or until restored).
- Guided OneDrive removal wizard (files local first, stop sync, uninstall, block; files are never deleted), with beginner explanations and warnings, and an Expert mode toggle (default off) in Options.
- App renamed to PrimeCrew Tweakinator 5000, with the app icon on the exe and in the header. Machine-specific name references removed so the app can be shared with friends.
