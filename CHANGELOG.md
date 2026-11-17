# Changelog

User-facing history for PrimeCrew Tweakinator 5000.

## 1.5.1

- Clean temp files now covers both usual places by default, in Beginner and Expert mode: your own temp folder (`%TMP%` / `%TEMP%`, normally `AppData\Local\Temp`) and `C:\Windows\Temp`. Other user accounts' temp folders stay Expert-only, and Expert mode can leave out `C:\Windows\Temp`.
- Your `%TMP%` / `%TEMP%` is cleaned only when it resolves to the standard `<profile>\AppData\Local\Temp`. If it points to a custom folder, it is left alone and the card and confirm window show a “Not used” note.

## 1.5.0

- New **Clean temp files** card (its own card, never part of Apply all, no Revert). **Scan temp files** is read-only and shows how many files and how much space can be cleaned. Beginner mode cleans only your own `%TEMP%`. Expert mode can add `C:\Windows\Temp` and other users' temp folders, and change the age limit (1 hour, 24 hours, 3 days, 7 days).
- **Clean temp files…** asks for confirmation with a required tick: deleted temp files cannot be restored (no Recycle Bin, no undo). Files changed in the last 24 hours are left alone by default. Files in use by other programs are skipped quietly and counted, not reported as errors; their names and the Windows reason are in the log file, and on screen in Expert mode. Empty folders left behind are removed.
- Safety rules: only paths strictly inside the temp folders are touched; symbolic links, junctions and other reparse points are never followed or deleted; a folder is used only if it is named Temp / Tmp and is not the top of a drive; the folder the app runs from is never touched; Clean checks everything again as it runs (the scan is only a preview).
- Optional **Finish cleanup at next restart…** adds only the files that were in use to Windows' delete-at-restart list (`MoveFileExW` + `MOVEFILE_DELAY_UNTIL_REBOOT`). What was scheduled is recorded in `C:\ProgramData\Win10Tweaks\tempclean-pending.json`; after the restart the card shows how many files were cleared and how much space came back. **Cancel scheduled cleanup…** removes only this app's entries and leaves entries from Windows or other programs untouched.
- Help topic: Clean temp files, plus a circled-? tip on the card. Logs and saved state, and Known limits, mention the new file and that temp cleanup cannot be undone.

## 1.4.0

- Pre-flight check (read-only) before Apply, also shown on the main window under This PC: administrator rights, whether the signed-in account is an administrator, the UAC level (info only — never changed), Microsoft Defender Tamper Protection and any third-party antivirus, a mismatch between the elevated and signed-in accounts, a pending reboot, and whether this is Windows 10 or 11. If the app is not elevated, a Restart as administrator button asks Windows for a UAC prompt. Each item has a Help link.
- After Apply, Beginner mode shows a summary dialog with counts of changes made, already done (skipped), and real problems, plus next steps (reboot, then Verify). If there were no real problems, it says “Everything worked.” Each real error gets a one-line explanation.
- Friendlier logs: a Beginner-mode legend explains that “Access denied” followed by SET means Windows resisted and the app handled it, and that only red ERROR lines need attention. Protected-task and WaaSMedicSvc lines use plain wording; the raw technical text is in the log file and, in Expert mode, on screen in a quieter style.
- False errors cleaned up: a leftover Store app version that is already gone (HRESULT 0x80073CF1, “Package was not found”) is logged as SKIP, not ERROR. Removing an app for the current user when the all-users remove fails is logged as a successful change with a calm note. The News and interests per-user taskbar value, which Windows often locks, is SKIP when the EnableFeeds policy is already off, and Verify checks that policy instead. Windows 11-only values (the taskbar widgets button, and widgets on the lock screen) are skipped on Windows 10 with no attempt and no error, and Verify does not fail on them.
- About says “Free for personal use. Use at your own risk — read Help ▸ Known limits,” plus the MIT license line. An MIT LICENSE file (copyright PrimeCrew Software) is in the repository.
- Help topics: Understanding the log, and Pre-flight check.

## 1.3.0

- Classic menu bar across the top: **File** (Copy PowerShell script, Open logs folder, Exit), **View** (Theme: System, Dark, Light, High contrast), **Options** (Expert mode, default off), and **Help** (Help contents / F1, About).
- In-app Help window (resizable, topic list plus article) covering every setting and feature in plain language. Tweak topics include a registry list generated from the same catalog the app uses. A Help link on each tweak and in the OneDrive wizard opens the matching topic.
- Circled question-mark tips next to settings, actions and wizard steps: hover for a short explanation, click to open Help. Shown in Beginner mode, and more quietly in Expert mode.
- About PrimeCrew Tweakinator 5000 popup under Help (icon, name, version, PrimeCrew Software, a one-line description, private-build / MIT note). About is no longer listed under Options.

## 1.2.0

- Rebranded to PrimeCrew Tweakinator 5000. Publisher / CompanyName is PrimeCrew Software.
- The exe is now `PrimeCrewTweakinator5000.exe`.
- Full x64 installer (`PrimeCrewTweakinator5000-v1.2.0-x64-Setup.exe`) installs to `C:\Program Files\PrimeCrew Software\PrimeCrew Tweakinator 5000\`, puts a Start menu shortcut in the PrimeCrew Software folder, and registers an Add/Remove Programs entry (Publisher PrimeCrew Software, DisplayVersion 1.2.0, app icon, uninstaller). It requires administrator rights and is 64-bit only. An upgrade removes previous shortcuts first so they do not duplicate. Uninstalling does not revert applied tweaks and does not delete `C:\ProgramData\Win10Tweaks`. The PrimeCrew Software Start menu folder is removed only if it is empty; the parent Program Files folder is left in place if other apps are there.
- Portable zip `PrimeCrewTweakinator5000-v1.2.0.zip` ships alongside the installer in every release.
- Temporary-allow restore timer: when running from the installed location, the SYSTEM task points at the installed exe; a portable run still stages a copy at the previous fixed path so a timer created by an older build still fires. Restore update kill clears the task and the staged copy, and never deletes the installed exe.

## 1.1.0

- Version numbers in the app (window title, header, and Options › About), in the exe version info (`FileVersion` / `ProductVersion`), and in the portable zip name.
- Source published to GitHub. Built binaries are attached to Releases, not committed.

## 1.0.0 (unversioned at the time)

The first builds shipped without a version number in the app or the zip name. In order:

- Initial PowerShell kit and portable Rust (eframe/egui) GUI: Apply / Verify / Revert, UAC self-elevation (`requireAdministrator`), confirmation before apply, System Restore point on by default, and a copy-to-clipboard / export PowerShell fallback.
- UI restyle: System, Dark, Light, and High contrast themes, plus text size 75–300%.
- Hard Windows Update kill (policies, no phone home, forced services including a locked WaaSMedicSvc key, protected tasks, outbound firewall rules) plus Temporary allow updates and Restore kill (timed or until restored).
- Guided OneDrive removal wizard (files local first, stop sync, uninstall, block; files are never deleted), with beginner explanations and warnings, and an Expert mode toggle (default off) in Options.
- App previously released under an earlier name, with the app icon on the exe and in the header. Machine-specific name references removed so the app can be shared with friends.
