; PrimeCrew Tweakinator 5000 Windows installer (NSIS 3, cross-built on Linux with makensis).
; Identity values come from scripts/build-release.sh (-D...).
;
; - 64-bit only, requires administrator rights
; - Installs to C:\Program Files\PrimeCrew Software\PrimeCrew Tweakinator 5000\
; - Start Menu shortcut in the "PrimeCrew Software" folder (all users)
; - Listed in Add/Remove Programs (HKLM Uninstall\APP_ID)
; - Uninstall does NOT revert tweaks and does NOT delete C:\ProgramData\Win10Tweaks
; - On upgrade, previous shortcuts are removed first so they do not duplicate
; - On uninstall, the Start menu folder is removed only if empty; the parent
;   Program Files\PrimeCrew Software folder is left in place if other apps are there
; - Shared PrimeCrew look: the first page and the Ready to Install page say either
;   "New installation" (editable "Install to:" path + Browse) or
;   "Updating existing installation: <app> v<old> → v<new>" with a read-only "Installed in:" path.
;   An upgrade (found via the uninstall key's DisplayVersion + InstallLocation, and the exe still
;   being there) is never moved: the existing path is always used, even if /D= is given.
;   The finish page says "Updated to v<new> in <path>" or "Installed v<new> to <path>".
; Silent: setup.exe /S [/DESKTOP] [/D=path]    uninstall.exe /S

Unicode true
Target amd64-unicode
!ifndef APP_NAME
  !error "Build with scripts/build-release.sh"
!endif

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "x64.nsh"
!include "nsDialogs.nsh"
!include "WinMessages.nsh"

Name "${APP_NAME}"
OutFile "${OUTFILE}"
InstallDir "$PROGRAMFILES64\${PUBLISHER}\${APP_NAME}"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
BrandingText "${PUBLISHER}"
ShowInstDetails show
ShowUninstDetails show

VIProductVersion "${VERSION4}"
VIAddVersionKey "ProductName" "${APP_NAME}"
VIAddVersionKey "FileDescription" "${APP_NAME} setup"
VIAddVersionKey "FileVersion" "${APP_VERSION}"
VIAddVersionKey "ProductVersion" "${APP_VERSION}"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "LegalCopyright" "${PUBLISHER}"

!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}"
!define SM_FOLDER "$SMPROGRAMS\${PUBLISHER}"
!define SM_LINK "$SMPROGRAMS\${PUBLISHER}\${APP_NAME}.lnk"

Var WantDesktop
Var Upgrade           ; 1 = existing install found (always updated in place)
Var InstalledVersion  ; DisplayVersion of the existing install
Var ExistingDir       ; InstallLocation of the existing install (never changed)
Var Banner            ; "New installation" or "Updating existing installation: … v<old> → v<new>"
Var FinishTitle
Var FinishMsg         ; "Installed v<new> to <path>" or "Updated to v<new> in <path>"
Var DirEdit
Var BannerFont

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_ABORTWARNING
!define MUI_COMPONENTSPAGE_SMALLDESC
!define MUI_FINISHPAGE_TITLE "$FinishTitle"
!define MUI_FINISHPAGE_TEXT "$FinishMsg$\r$\n$\r$\nStart menu: ${PUBLISHER} › ${APP_NAME}$\r$\n$\r$\nTweaks you already applied were not changed. Click Finish to close Setup."
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APP_NAME} now"
!define MUI_UNWELCOMEPAGE_TEXT "This will remove ${APP_NAME} from this PC.$\r$\n$\r$\nUninstalling does not revert any tweaks you applied, and does not delete logs, recorded originals, or backups under C:\ProgramData\Win10Tweaks. Revert stays an in-app action. If you want your previous Windows settings back, run the app and click Revert before uninstalling.$\r$\n$\r$\nClick Next to continue."

Page custom LocationPage LocationLeave
!insertmacro MUI_PAGE_COMPONENTS
Page custom ReadyPage
!insertmacro MUI_PAGE_INSTFILES
!define MUI_PAGE_CUSTOMFUNCTION_PRE SetFinishText
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_WELCOME
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; ------------------------------------------------------------------ install
Section "${APP_NAME} (required)" SecApp
  SectionIn RO
  SetShellVarContext all
  SetRegView 64
  ; Never move an existing install.
  ${If} $Upgrade == 1
    StrCpy $INSTDIR $ExistingDir
  ${EndIf}
  SetOutPath "$INSTDIR"
  File "/oname=${APP_EXE}" "${EXE_PATH}"
  File "/oname=README.md" "${SRC_DIR}\README.md"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Upgrade: drop previous shortcuts (Start menu, ungrouped leftover, desktop) so they do not duplicate.
  Delete "${SM_LINK}"
  Delete "$SMPROGRAMS\${APP_NAME}.lnk"
  Delete "$DESKTOP\${APP_NAME}.lnk"

  ${GetSize} "$INSTDIR" "/S=0K" $3 $4 $5
  IntFmt $3 "0x%08X" $3
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKLM "${UNINST_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\${APP_EXE},0"
  WriteRegStr HKLM "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKLM "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoRepair" 1
  WriteRegDWORD HKLM "${UNINST_KEY}" "EstimatedSize" $3
  WriteRegDWORD HKLM "${UNINST_KEY}" "VersionMajor" ${VERSION_MAJOR}
  WriteRegDWORD HKLM "${UNINST_KEY}" "VersionMinor" ${VERSION_MINOR}
SectionEnd

Section "Start Menu shortcut" SecStart
  SectionIn RO
  SetShellVarContext all
  CreateDirectory "${SM_FOLDER}"
  CreateShortcut "${SM_LINK}" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0
SectionEnd

Section /o "Desktop shortcut" SecDesktop
  SetShellVarContext all
  CreateShortcut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0
SectionEnd

Function .onInit
  SetRegView 64
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "${APP_NAME} requires 64-bit Windows."
    Abort
  ${EndIf}
  StrCpy $WantDesktop 0
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/DESKTOP" $R1
  ${IfNot} ${Errors}
    StrCpy $WantDesktop 1
  ${EndIf}
  ${If} $INSTDIR == ""
    StrCpy $INSTDIR "$PROGRAMFILES64\${PUBLISHER}\${APP_NAME}"
  ${EndIf}
  Call DetectExistingInstall
  Call SetTexts
  ${If} $WantDesktop == 1
    SectionGetFlags ${SecDesktop} $0
    IntOp $0 $0 | ${SF_SELECTED}
    SectionSetFlags ${SecDesktop} $0
  ${EndIf}
FunctionEnd

; ------------------------------------------------------------------ upgrade detection + shared pages

Function TrimDir
  ; In/out: $0 — drop a trailing backslash
  Push $1
  Push $2
  StrLen $1 $0
  ${If} $1 > 3
    IntOp $1 $1 - 1
    StrCpy $2 $0 1 $1
    ${If} $2 == "\"
      StrCpy $0 $0 $1
    ${EndIf}
  ${EndIf}
  Pop $2
  Pop $1
FunctionEnd

Function DetectExistingInstall
  ; Existing install = HKLM uninstall key has InstallLocation and the exe is still there.
  StrCpy $Upgrade 0
  StrCpy $InstalledVersion ""
  StrCpy $ExistingDir ""
  ClearErrors
  ReadRegStr $0 HKLM "${UNINST_KEY}" "InstallLocation"
  ${If} $0 != ""
    Call TrimDir
    ${If} ${FileExists} "$0\${APP_EXE}"
      ReadRegStr $1 HKLM "${UNINST_KEY}" "DisplayVersion"
      ${If} $1 == ""
        StrCpy $1 "?"
      ${EndIf}
      StrCpy $Upgrade 1
      StrCpy $InstalledVersion $1
      StrCpy $ExistingDir $0
      StrCpy $INSTDIR $0
    ${EndIf}
  ${EndIf}
FunctionEnd

Function SetTexts
  ${If} $Upgrade == 1
    StrCpy $Banner "Updating existing installation: ${APP_NAME} v$InstalledVersion → v${APP_VERSION}"
  ${Else}
    StrCpy $Banner "New installation"
  ${EndIf}
FunctionEnd

Function SetFinishText
  ${If} $Upgrade == 1
    StrCpy $FinishTitle "Update complete"
    StrCpy $FinishMsg "Updated to v${APP_VERSION} in $INSTDIR"
  ${Else}
    StrCpy $FinishTitle "Installation complete"
    StrCpy $FinishMsg "Installed v${APP_VERSION} to $INSTDIR"
  ${EndIf}
FunctionEnd

; Banner (bold, larger) + "Installed in:" read-only path, or "Install to:" editable path + Browse.
; $R9 = 1 to make the path read-only even on a fresh install (Ready page). Leaves the path edit in $DirEdit.
Function MakeBannerAndPath
  CreateFont $BannerFont "$(^Font)" 11 700
  ${NSD_CreateLabel} 0 0 100% 30u "$Banner"
  Pop $0
  SendMessage $0 ${WM_SETFONT} $BannerFont 1
  ${If} $Upgrade == 1
    ${NSD_CreateLabel} 0 34u 100% 10u "Installed in:"
    Pop $0
    ${NSD_CreateText} 0 46u 100% 12u "$INSTDIR"
    Pop $DirEdit
    SendMessage $DirEdit ${EM_SETREADONLY} 1 0
    EnableWindow $DirEdit 0
  ${Else}
    ${NSD_CreateLabel} 0 34u 100% 10u "Install to:"
    Pop $0
    ${If} $R9 == 1
      ${NSD_CreateText} 0 46u 100% 12u "$INSTDIR"
      Pop $DirEdit
      SendMessage $DirEdit ${EM_SETREADONLY} 1 0
      EnableWindow $DirEdit 0
    ${Else}
      ${NSD_CreateDirRequest} 0 46u -62u 12u "$INSTDIR"
      Pop $DirEdit
      ${NSD_CreateBrowseButton} -58u 45u 58u 14u "B&rowse..."
      Pop $0
      ${NSD_OnClick} $0 BrowseClicked
    ${EndIf}
  ${EndIf}
FunctionEnd

Function BrowseClicked
  ${NSD_GetText} $DirEdit $0
  nsDialogs::SelectFolderDialog "Choose the folder to install ${APP_NAME} in:" "$0"
  Pop $0
  ${If} $0 != error
    ${NSD_SetText} $DirEdit "$0"
  ${EndIf}
FunctionEnd

Function LocationPage
  ${If} $Upgrade == 1
    !insertmacro MUI_HEADER_TEXT "Updating existing installation" "${APP_NAME} v$InstalledVersion → v${APP_VERSION}"
  ${Else}
    !insertmacro MUI_HEADER_TEXT "New installation" "${APP_NAME} v${APP_VERSION}"
  ${EndIf}
  nsDialogs::Create 1018
  Pop $1
  StrCpy $R9 0
  Call MakeBannerAndPath
  ${If} $Upgrade == 1
    StrCpy $2 "Setup will update ${APP_NAME} in place. The folder stays where it is.$\r$\n$\r$\nTweaks you already applied are not changed, and your logs and recorded originals under C:\ProgramData\Win10Tweaks are kept."
  ${Else}
    StrCpy $2 "This installs ${APP_NAME} for everyone on this 64-bit PC (administrator rights required). The Start menu entry goes in the ${PUBLISHER} folder.$\r$\n$\r$\nUninstalling later does not revert any tweaks you applied and does not delete logs or recorded originals under C:\ProgramData\Win10Tweaks. If you want your previous Windows settings back, run Revert in the app first."
  ${EndIf}
  ${NSD_CreateLabel} 0 70u 100% 60u "$2"
  Pop $0
  nsDialogs::Show
FunctionEnd

Function LocationLeave
  ${If} $Upgrade == 1
    StrCpy $INSTDIR $ExistingDir
    Return
  ${EndIf}
  ${NSD_GetText} $DirEdit $0
  Call TrimDir
  StrCpy $1 $0 2 1
  ${If} $0 == ""
  ${OrIf} $1 != ":\"
    MessageBox MB_OK|MB_ICONEXCLAMATION "Please choose a full folder path, for example $PROGRAMFILES64\${PUBLISHER}\${APP_NAME}."
    Abort
  ${EndIf}
  StrCpy $INSTDIR $0
FunctionEnd

Function ReadyPage
  ${If} $Upgrade == 1
    !insertmacro MUI_HEADER_TEXT "Ready to Install" "Setup is ready to update ${APP_NAME}."
  ${Else}
    !insertmacro MUI_HEADER_TEXT "Ready to Install" "Setup is ready to install ${APP_NAME}."
  ${EndIf}
  nsDialogs::Create 1018
  Pop $1
  StrCpy $R9 1
  Call MakeBannerAndPath
  StrCpy $2 "Start menu: ${PUBLISHER} › ${APP_NAME}"
  ${If} ${SectionIsSelected} ${SecDesktop}
    StrCpy $2 "$2$\r$\nDesktop shortcut: yes"
  ${Else}
    StrCpy $2 "$2$\r$\nDesktop shortcut: no"
  ${EndIf}
  ${If} $Upgrade == 1
    StrCpy $2 "$2$\r$\n$\r$\nClick Update to continue."
  ${Else}
    StrCpy $2 "$2$\r$\n$\r$\nClick Install to continue."
  ${EndIf}
  ${NSD_CreateLabel} 0 70u 100% 50u "$2"
  Pop $0
  GetDlgItem $0 $HWNDPARENT 1
  ${If} $Upgrade == 1
    SendMessage $0 ${WM_SETTEXT} 0 "STR:&Update"
  ${Else}
    SendMessage $0 ${WM_SETTEXT} 0 "STR:&Install"
  ${EndIf}
  nsDialogs::Show
FunctionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecApp} "The program, README, and uninstaller. Tweaks you have already applied are not changed."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecStart} "A shortcut in the ${PUBLISHER} Start menu folder."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "A shortcut on the desktop."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

; ------------------------------------------------------------------ uninstaller
; Does not revert tweaks and does not touch C:\ProgramData\Win10Tweaks.
Section "un.${APP_NAME}" UnSecApp
  SetShellVarContext all
  SetRegView 64
  Delete "${SM_LINK}"
  Delete "$SMPROGRAMS\${APP_NAME}.lnk"
  Delete "$DESKTOP\${APP_NAME}.lnk"
  ; Remove the publisher Start menu folder only if it is now empty.
  RMDir "${SM_FOLDER}"
  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  ; Remove the parent Program Files\PrimeCrew Software folder only if it is empty
  ; (other PrimeCrew Software apps may be installed there).
  RMDir "$PROGRAMFILES64\${PUBLISHER}"
  DeleteRegKey HKLM "${UNINST_KEY}"
SectionEnd

Function un.onInit
  SetRegView 64
  SetShellVarContext all
FunctionEnd
