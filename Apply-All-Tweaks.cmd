@echo off
rem Double-click: shows a UAC prompt, then runs Apply-All-Tweaks.ps1 elevated in a window that stays open.
rem Extra arguments are passed through, e.g.  Apply-All-Tweaks.cmd -WhatIf
set "PS1=%~dp0Apply-All-Tweaks.ps1"
powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process PowerShell -Verb RunAs -ArgumentList ('-NoProfile -NoExit -ExecutionPolicy Bypass -File \"%PS1%\" %*')"
