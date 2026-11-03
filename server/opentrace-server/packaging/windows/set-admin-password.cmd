@echo off
rem Sets the password of the built-in Administrator account and starts the OpenTrace Server service.
rem Run as administrator (the installer offers it on its last page). The server must be stopped while the
rem password is written; the service is stopped first if it runs.
setlocal
rem Needs administrator rights (data folder in ProgramData, service control): ask for them if missing.
net session >nul 2>&1
if errorlevel 1 (
  echo Requesting administrator rights ...
  if "%~1"=="" (
    powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
  ) else (
    powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -ArgumentList '\"%~1\"' -Verb RunAs"
  )
  exit /b
)
set "HERE=%~dp0"
set "CFG=%ProgramData%\OpenTrace\server.toml"
if not "%~1"=="" set "CFG=%~1"
sc.exe query OpenTraceServer | find "RUNNING" >nul && (
  echo Stopping OpenTraceServer ...
  sc.exe stop OpenTraceServer >nul
  timeout /t 5 /nobreak >nul
)
echo.
echo Password for the user "Administrator" of OpenTrace Server: 6 to 30 characters, typed twice, not shown.
"%HERE%opentrace-server.exe" set-password Administrator --config "%CFG%"
if errorlevel 1 (
  echo Password not set.
  pause
  exit /b 1
)
sc.exe start OpenTraceServer >nul
timeout /t 5 /nobreak >nul
sc.exe query OpenTraceServer | find "RUNNING" >nul
if errorlevel 1 (
  echo.
  echo The service did not start. Last lines of the log:
  powershell -NoProfile -Command "Get-Content -Tail 15 \"$env:ProgramData\OpenTrace\server\logs\server.log\" -ErrorAction SilentlyContinue"
  echo Is another opentrace-server already using port 8770 or this data folder? See %ProgramData%\OpenTrace\server.toml
  pause
  exit /b 1
)
echo.
echo OpenTrace Server started. Check: http://127.0.0.1:8770/api/v1/health
echo Log: %ProgramData%\OpenTrace\server\logs\server.log
pause
