<#
.SYNOPSIS
  Registers OpenTrace Server to start at boot as a Windows scheduled task running under NT AUTHORITY\LOCAL SERVICE.
  (A native Windows Service with SCM integration is planned; this uses only built-in Windows tools.)
.DESCRIPTION
  Run in an elevated PowerShell. Expects opentrace-server.exe and the resources\ folder in -InstallDir and the
  configuration in -ConfigFile. Grants LOCAL SERVICE write access only to -DataDir.
.EXAMPLE
  .\install-scheduled-task.ps1 -InstallDir "C:\Program Files\OpenTrace Server" -DataDir "C:\ProgramData\OpenTrace\server"
#>
param(
    [string]$InstallDir = "C:\Program Files\OpenTrace Server",
    [string]$DataDir = "C:\ProgramData\OpenTrace\server",
    [string]$ConfigFile = "C:\ProgramData\OpenTrace\server\server.toml",
    [string]$TaskName = "OpenTrace Server"
)
$ErrorActionPreference = "Stop"
$exe = Join-Path $InstallDir "opentrace-server.exe"
if (-not (Test-Path $exe)) { throw "Not found: $exe" }
if (-not (Test-Path $ConfigFile)) { throw "Not found: $ConfigFile (copy opentrace-server.example.toml and edit data_dir/listen)" }
New-Item -ItemType Directory -Force -Path $DataDir | Out-Null
# LOCAL SERVICE: modify rights on the data directory only.
& icacls $DataDir /grant "*S-1-5-19:(OI)(CI)M" | Out-Null

& $exe check-config --config $ConfigFile
if ($LASTEXITCODE -ne 0) { throw "Configuration check failed" }

$action = New-ScheduledTaskAction -Execute $exe -Argument "serve --config `"$ConfigFile`"" -WorkingDirectory $InstallDir
$trigger = New-ScheduledTaskTrigger -AtStartup
$principal = New-ScheduledTaskPrincipal -UserId "NT AUTHORITY\LOCAL SERVICE" -LogonType ServiceAccount -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -RestartCount 999 `
    -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew
Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
Start-ScheduledTask -TaskName $TaskName
Write-Host "Registered and started '$TaskName'. Check: Get-ScheduledTask -TaskName '$TaskName' | Get-ScheduledTaskInfo"
