@echo off
rem Foreground start for first tests: OpenTrace Server on http://127.0.0.1:8770 (loopback only).
rem Data in %ProgramData%\OpenTrace\server unless OPENTRACE_SERVER_DATA_DIR is set.
setlocal
set "HERE=%~dp0"
if "%OPENTRACE_RESOURCES%"=="" set "OPENTRACE_RESOURCES=%HERE%resources"
if "%OPENTRACE_SERVER_TRANSPORT%"=="" set "OPENTRACE_SERVER_TRANSPORT=http"
"%HERE%opentrace-server.exe" serve %*
