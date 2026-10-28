; Good Gallery's installer (docs/specs/014-ci-release): installs for the current user, no admin rights needed, and the
; folder can be changed. Built by `cargo xtask dist`, which passes:
;   VERSION  the app's version (1.2.3)
;   SOURCE   the staged package folder (good-gallery.exe, helpers, licenses)
;   OUTFILE  the installer to write
;   ICON     the app icon (.ico)
; Uninstalling removes the program, its shortcut and its registry entry, never the data folder
; (%APPDATA%\good-gallery: index, settings, thumbnails, logs).

Unicode true
ManifestDPIAware true

!include "MUI2.nsh"
!include "FileFunc.nsh"

!ifndef VERSION | SOURCE | OUTFILE | ICON
  !error "Pass /DVERSION=… /DSOURCE=… /DOUTFILE=… /DICON=… (cargo xtask dist does)"
!endif

!define PRODUCT "Good Gallery"
!define EXE "good-gallery.exe"
!define UNINSTALLER "Uninstall Good Gallery.exe"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\GoodGallery"

Name "${PRODUCT}"
OutFile "${OUTFILE}"
; Not the Electron version's folder (good-gallery), so both can be installed side by side.
InstallDir "$LOCALAPPDATA\Programs\${PRODUCT}"
InstallDirRegKey HKCU "${UNINSTALL_KEY}" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma
BrandingText "${PRODUCT} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${PRODUCT} setup"
VIAddVersionKey "LegalCopyright" "GPL-3.0-only"

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Run ${PRODUCT}"

!define MUI_PAGE_CUSTOMFUNCTION_LEAVE OwnFolder
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; The app gets a folder of its own: a picked folder that already holds other files (and isn't an earlier
; installation) gets "Good Gallery" appended, so installing and uninstalling never touch files of others.
Function OwnFolder
  IfFileExists "$INSTDIR\${EXE}" done
  FindFirst $0 $1 "$INSTDIR\*.*"
  loop:
    StrCmp $1 "" close
    StrCmp $1 "." next
    StrCmp $1 ".." next
    FindClose $0
    StrCpy $INSTDIR "$INSTDIR\${PRODUCT}"
    Goto done
  next:
    FindNext $0 $1
    Goto loop
  close:
    FindClose $0
  done:
FunctionEnd

; Silent installs (/S /D=…) skip the directory page.
Function .onInit
  IfSilent 0 +2
    Call OwnFolder
FunctionEnd

Section "Install"
  SetOutPath "$INSTDIR"
  ; An update replaces the helper folders as a whole, so no file of an older version stays behind. The folder is the
  ; app's own (OwnFolder).
  RMDir /r "$INSTDIR\exiftool"
  RMDir /r "$INSTDIR\ffmpeg"
  RMDir /r "$INSTDIR\mpv"
  File /r "${SOURCE}\*.*"
  WriteUninstaller "$INSTDIR\${UNINSTALLER}"
  CreateShortcut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\${EXE}"

  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${EXE}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\${UNINSTALLER}"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\${UNINSTALLER}" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" $0
SectionEnd

Section "Uninstall"
  Delete "$SMPROGRAMS\${PRODUCT}.lnk"
  ; Only what the installer put there, in the app's own folder (OwnFolder).
  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\THIRD-PARTY-NOTICES.txt"
  RMDir /r "$INSTDIR\exiftool"
  RMDir /r "$INSTDIR\ffmpeg"
  RMDir /r "$INSTDIR\mpv"
  Delete "$INSTDIR\${UNINSTALLER}"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd
