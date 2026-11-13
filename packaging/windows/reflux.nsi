; Reflux's per-user Windows setup (no admin): %LOCALAPPDATA%\Programs\Reflux, a Start menu
; shortcut and an uninstaller. `desktop::updater` runs a newer setup with /S to update in place:
; the setup waits for the running app to exit, installs over it and starts it again.
; makensis /DVERSION=0.1.0 /DSRC=<dir with reflux.exe and fonts\> /DOUT=<setup.exe> reflux.nsi

Unicode true
!include "MUI2.nsh"

Name "Reflux"
OutFile "${OUT}"
InstallDir "$LOCALAPPDATA\Programs\Reflux"
RequestExecutionLevel user
SetCompressor /SOLID lzma

!define UNINST "Software\Microsoft\Windows\CurrentVersion\Uninstall\Reflux"

!define MUI_FINISHPAGE_RUN "$INSTDIR\reflux.exe"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section
  SetOutPath "$INSTDIR"
  ; The app (an update's restart) may still be exiting: wait up to 10 s for its executable.
  StrCpy $0 0
  wait:
    ClearErrors
    Delete "$INSTDIR\reflux.exe"
    IfErrors 0 free
    IntOp $0 $0 + 1
    IntCmp $0 40 free
    Sleep 250
    Goto wait
  free:
  File "${SRC}\reflux.exe"
  RMDir /r "$INSTDIR\fonts"
  File /r "${SRC}\fonts"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateShortcut "$SMPROGRAMS\Reflux.lnk" "$INSTDIR\reflux.exe"
  WriteRegStr HKCU "${UNINST}" "DisplayName" "Reflux"
  WriteRegStr HKCU "${UNINST}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST}" "Publisher" "Reflux"
  WriteRegStr HKCU "${UNINST}" "DisplayIcon" "$INSTDIR\reflux.exe"
  WriteRegStr HKCU "${UNINST}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKCU "${UNINST}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST}" "NoRepair" 1
  ; A silent run is an update: start the app again.
  IfSilent 0 +2
    Exec '"$INSTDIR\reflux.exe"'
SectionEnd

Section "Uninstall"
  Delete "$SMPROGRAMS\Reflux.lnk"
  RMDir /r "$INSTDIR\fonts"
  Delete "$INSTDIR\reflux.exe"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "${UNINST}"
  DeleteRegKey HKCU "Software\Classes\fluxer"
SectionEnd
