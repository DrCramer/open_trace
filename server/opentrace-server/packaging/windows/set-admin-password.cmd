@echo off
rem Sets the password of the built-in Administrator account and starts the OpenTrace Server service.
rem Run as administrator (the installer offers it on its last page). The server must be stopped while the
rem password is written; the service is stopped first if it runs.
setlocal
set "HERE=%~dp0"
set "CFG=%ProgramData%\OpenTrace\server.toml"
if not "%~1"=="" set "CFG=%~1"
sc.exe query OpenTraceServer | find "RUNNING" >nul && (
  echo Stopping OpenTraceServer ...
  sc.exe stop OpenTraceServer >nul
  timeout /t 5 /nobreak >nul
)
echo.
echo Password for the user "Administrator" of OpenTrace Server (input is shown as typed):
"%HERE%opentrace-server.exe" set-password Administrator --config "%CFG%"
if errorlevel 1 (
  echo Password not set.
  pause
  exit /b 1
)
sc.exe start OpenTraceServer >nul
echo.
echo OpenTrace Server started. Check: http://127.0.0.1:8770/api/v1/health
echo Log: %ProgramData%\OpenTrace\server\logs\server.log
pause
