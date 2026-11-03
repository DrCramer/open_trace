; OpenTrace Server - Windows installer (NSIS 3, Unicode).
;   makensis /DVERSION=0.1.0 /DSRC=<folder with opentrace-server.exe and resources\> /DOUT=<setup.exe> opentrace-server.nsi
; Installs into "Program Files\OpenTrace Server", writes %ProgramData%\OpenTrace\server.toml once (never overwritten),
; registers the Windows service OpenTraceServer (NT SERVICE\OpenTraceServer, automatic start, restart on failure) and
; leaves it stopped until the Administrator password is set (last page or set-admin-password.cmd).
; Upgrade: the running service is stopped, files replaced, service started again. Uninstall removes the service and
; the program files; laboratory data in %ProgramData%\OpenTrace stays (delete it yourself after a final backup).
; Silent: setup.exe /S (password: set OPENTRACE_NEW_PASSWORD for set-admin-password.cmd or run set-password).
Unicode true
!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!ifndef SRC
  !define SRC "."
!endif
!ifndef OUT
  !define OUT "OpenTrace-Server-${VERSION}-setup.exe"
!endif
!define SVC "OpenTraceServer"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\OpenTraceServer"

Name "OpenTrace Server ${VERSION}"
OutFile "${OUT}"
InstallDir "$PROGRAMFILES64\OpenTrace Server"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
ShowInstDetails show

!include "MUI2.nsh"
!include "x64.nsh"
!include "LogicLib.nsh"

!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_TEXT "Задать пароль администратора и запустить службу"
!define MUI_FINISHPAGE_RUN_FUNCTION RunSetPassword
!define MUI_FINISHPAGE_TEXT "OpenTrace Server установлен как служба Windows «${SVC}» (остановлена до задания пароля администратора).$\r$\n$\r$\nКонфигурация: %ProgramData%\OpenTrace\server.toml$\r$\nДанные: %ProgramData%\OpenTrace\server$\r$\n$\r$\nИсследовательское ПО. Не валидировано для диагностики и судебно-экспертной работы."
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Russian"
!insertmacro MUI_LANGUAGE "English"

Var WasRunning

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "OpenTrace Server requires 64-bit Windows."
    Abort
  ${EndIf}
  SetRegView 64
FunctionEnd

Function RunSetPassword
  Exec '"$SYSDIR\cmd.exe" /c ""$INSTDIR\set-admin-password.cmd""'
FunctionEnd

Section "OpenTrace Server" SecMain
  SetShellVarContext all
  StrCpy $WasRunning "0"
  ; Upgrade: stop a running service before replacing the executable.
  nsExec::Exec '"$SYSDIR\cmd.exe" /c ""$SYSDIR\sc.exe" query ${SVC} | "$SYSDIR\find.exe" "RUNNING" >nul"'
  Pop $0
  ${If} $0 == 0
    StrCpy $WasRunning "1"
    DetailPrint "Stopping ${SVC} ..."
    nsExec::Exec '"$SYSDIR\sc.exe" stop ${SVC}'
    Pop $0
    Sleep 4000
  ${EndIf}

  SetOutPath "$INSTDIR"
  File "${SRC}\opentrace-server.exe"
  File "${SRC}\set-admin-password.cmd"
  File "${SRC}\run-server.cmd"
  File "${SRC}\opentrace-server.example.toml"
  File "${SRC}\server.windows.toml"
  SetOutPath "$INSTDIR\resources"
  File /r "${SRC}\resources\*.*"
  SetOutPath "$INSTDIR"

  ; Configuration: written once, kept on upgrade.
  CreateDirectory "$APPDATA\OpenTrace"
  CreateDirectory "$APPDATA\OpenTrace\server"
  ${IfNot} ${FileExists} "$APPDATA\OpenTrace\server.toml"
    CopyFiles /SILENT "$INSTDIR\server.windows.toml" "$APPDATA\OpenTrace\server.toml"
    DetailPrint "Configuration written: $APPDATA\OpenTrace\server.toml"
  ${Else}
    DetailPrint "Configuration kept: $APPDATA\OpenTrace\server.toml"
  ${EndIf}

  nsExec::ExecToLog '"$INSTDIR\opentrace-server.exe" check-config --config "$APPDATA\OpenTrace\server.toml"'
  Pop $0
  ${If} $0 != 0
    MessageBox MB_ICONEXCLAMATION "check-config reported a problem with $APPDATA\OpenTrace\server.toml (see details). The service is registered but will not start until the configuration is fixed." /SD IDOK
  ${EndIf}

  ; Service: registered once (the path of the exe stays the same on upgrade).
  nsExec::ExecToStack '"$SYSDIR\sc.exe" query ${SVC}'
  Pop $0
  Pop $1
  ${If} $0 != 0
    nsExec::ExecToLog '"$INSTDIR\opentrace-server.exe" service-install --config "$APPDATA\OpenTrace\server.toml"'
    Pop $0
    ${If} $0 != 0
      MessageBox MB_ICONSTOP "Registering the Windows service failed (see details)." /SD IDOK
      SetErrorLevel 2
    ${EndIf}
  ${ElseIf} $WasRunning == "1"
    nsExec::Exec '"$SYSDIR\sc.exe" start ${SVC}'
    Pop $0
    DetailPrint "Service started again"
  ${EndIf}

  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayName" "OpenTrace Server"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINST_KEY}" "Publisher" "OpenTrace"
  WriteRegStr HKLM "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKLM "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoRepair" 1
SectionEnd

Function un.onInit
  SetRegView 64
FunctionEnd

Section "Uninstall"
  SetShellVarContext all
  nsExec::ExecToLog '"$INSTDIR\opentrace-server.exe" service-uninstall'
  Pop $0
  Delete "$INSTDIR\opentrace-server.exe"
  Delete "$INSTDIR\set-admin-password.cmd"
  Delete "$INSTDIR\run-server.cmd"
  Delete "$INSTDIR\opentrace-server.example.toml"
  Delete "$INSTDIR\server.windows.toml"
  RMDir /r "$INSTDIR\resources"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKLM "${UNINST_KEY}"
  DetailPrint "Laboratory data in $APPDATA\OpenTrace was kept."
SectionEnd

