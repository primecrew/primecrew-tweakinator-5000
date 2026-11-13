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

Name "${APP_NAME}"
OutFile "${OUTFILE}"
InstallDir "$PROGRAMFILES64\${PUBLISHER}\${APP_NAME}"
InstallDirRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" "InstallLocation"
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

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_ABORTWARNING
!define MUI_COMPONENTSPAGE_SMALLDESC
!define MUI_WELCOMEPAGE_TEXT "This installs ${APP_NAME} ${APP_VERSION} for everyone on this 64-bit PC (administrator rights required).$\r$\n$\r$\nDefault location: $PROGRAMFILES64\${PUBLISHER}\${APP_NAME}$\r$\nStart menu: ${PUBLISHER}\${APP_NAME}$\r$\n$\r$\nUninstalling later does not revert any tweaks you applied and does not delete logs or recorded originals under C:\ProgramData\Win10Tweaks. If you want your previous Windows settings back, run Revert in the app first."
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APP_NAME} now"
!define MUI_UNWELCOMEPAGE_TEXT "This will remove ${APP_NAME} from this PC.$\r$\n$\r$\nUninstalling does not revert any tweaks you applied, and does not delete logs, recorded originals, or backups under C:\ProgramData\Win10Tweaks. Revert stays an in-app action. If you want your previous Windows settings back, run the app and click Revert before uninstalling.$\r$\n$\r$\nClick Next to continue."

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
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
  ${If} $WantDesktop == 1
    SectionGetFlags ${SecDesktop} $0
    IntOp $0 $0 | ${SF_SELECTED}
    SectionSetFlags ${SecDesktop} $0
  ${EndIf}
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
