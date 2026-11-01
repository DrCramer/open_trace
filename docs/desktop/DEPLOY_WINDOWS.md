# OpenTrace Desktop и OpenTrace Server на Windows

Windows 10/11 x64 (клиент) и Windows Server 2019+/Windows 10/11 x64 (сервер). Docker не нужен.

> Исследовательское ПО. Не валидировано для диагностики и судебно-экспертной работы. Результаты анализа на Windows пока могут отличаться от Linux в последнем бите для части моделей пиков (см. `BASELINE_AUDIT.md`, W6).

## 1. Инструменты сборки

1. Visual Studio Build Tools 2022 с компонентом «Разработка классических приложений на C++» (MSVC + Windows SDK).
2. Rust: https://rustup.rs → `rustup-init.exe` (toolchain `stable-x86_64-pc-windows-msvc`).
3. Git for Windows; Node.js 18+ (только для сборки установщика десктопа).
4. WebView2 Runtime (в Windows 11 уже есть; на Windows 10 установщик десктопа скачает его сам).

Проверка в PowerShell:

```powershell
rustc --version; cargo --version; git --version; node --version
```

## 2. Desktop

```powershell
git clone https://github.com/DrCramer/open_trace.git
cd open_trace
git checkout feature/desktop-server-platform
# быстрый запуск без установщика:
cargo build --release --manifest-path desktop\opentrace-desktop\src-tauri\Cargo.toml
.\desktop\opentrace-desktop\src-tauri\target\release\opentrace-desktop.exe
# установщики (NSIS .exe и MSI) в desktop\opentrace-desktop\src-tauri\target\release\bundle\
cd desktop\opentrace-desktop
npm install
npx tauri build --bundles nsis msi
```

NSIS-установщик ставит приложение для текущего пользователя (без прав администратора). Данные: `%LOCALAPPDATA%\org.opentrace.desktop\` (проекты, security, `fsa-cache`), настройки: `%APPDATA%\org.opentrace.desktop\desktop.json`, токен сервера — Диспетчер учётных данных Windows (`org.opentrace.desktop`). Ресурсы интерфейса — рядом с exe (`resources\web`, `resources\data`).

Самопроверка (анализ синтетического FSA ядром, пример проекта, окно графиков через `window.open` с доступом к `window.opener`):

```powershell
$env:OPENTRACE_DESKTOP_SELFTEST = "$PWD\selftest.json"
$p = Start-Process .\desktop\opentrace-desktop\src-tauri\target\release\opentrace-desktop.exe -PassThru -Wait
Get-Content selftest.json; "exit code: $($p.ExitCode)"
Remove-Item Env:\OPENTRACE_DESKTOP_SELFTEST
```

Самотест использует временный каталог данных и сам закрывает приложение (`"pass": true`, код 0).

Подключение к серверу: в строке состояния главного окна щёлкните ячейку «Сервер…» → адрес `https://…` → «Проверить соединение» → вход. Корпоративный CA берётся из хранилища сертификатов Windows; отдельный PEM можно указать в поле «Сертификат CA».

## 3. Server

```powershell
cargo build --release --locked --manifest-path server\opentrace-server\Cargo.toml
$dst = "C:\Program Files\OpenTrace Server"
New-Item -ItemType Directory -Force $dst | Out-Null
Copy-Item server\opentrace-server\target\release\opentrace-server.exe $dst
Copy-Item -Recurse -Force resources "$dst\resources"
Copy-Item server\opentrace-server\packaging\windows\run-server.cmd $dst
New-Item -ItemType Directory -Force C:\ProgramData\OpenTrace\server | Out-Null
Copy-Item server\opentrace-server\packaging\opentrace-server.example.toml C:\ProgramData\OpenTrace\server\server.toml
notepad C:\ProgramData\OpenTrace\server\server.toml   # data_dir = 'C:\ProgramData\OpenTrace\server' (одинарные кавычки в TOML для путей с \)
```

Первый администратор и утверждающий (сервер остановлен):

```powershell
$env:OPENTRACE_RESOURCES = "$dst\resources"
& "$dst\opentrace-server.exe" set-password Administrator --config C:\ProgramData\OpenTrace\server\server.toml
& "$dst\opentrace-server.exe" grant-approver "Lab Director" --config C:\ProgramData\OpenTrace\server\server.toml
```

Пробный запуск в окне консоли (Ctrl+C — остановка):

```powershell
& "$dst\opentrace-server.exe" serve --config C:\ProgramData\OpenTrace\server\server.toml
Invoke-RestMethod http://127.0.0.1:8770/api/v1/health
```

### Автозапуск

Полноценная служба Windows (SCM) — этап F. Сейчас: задача планировщика при загрузке под `NT AUTHORITY\LOCAL SERVICE` (только встроенные средства; права на запись только в каталог данных). В PowerShell от администратора:

```powershell
cd server\opentrace-server\packaging\windows
.\install-scheduled-task.ps1 -InstallDir "C:\Program Files\OpenTrace Server" -DataDir "C:\ProgramData\OpenTrace\server" -ConfigFile "C:\ProgramData\OpenTrace\server\server.toml"
Get-ScheduledTask -TaskName "OpenTrace Server" | Get-ScheduledTaskInfo
```

Скрипт в этой итерации **не запускался** на реальной машине (требует прав администратора); перед использованием в лаборатории проверьте на тестовом сервере. Удаление: `Unregister-ScheduledTask -TaskName "OpenTrace Server" -Confirm:$false`.

### HTTPS на Windows

Варианты: Caddy для Windows (тот же `Caddyfile`), IIS + Application Request Routing (обратный прокси на `http://127.0.0.1:8770`) с сертификатом внутреннего CA. Брандмауэр открывайте только для порта прокси (443), не для 8770.

### Резервное копирование

```powershell
& "$dst\opentrace-server.exe" backup --config C:\ProgramData\OpenTrace\server\server.toml --out D:\Backups\OpenTrace
& "$dst\opentrace-server.exe" verify-backup D:\Backups\OpenTrace\opentrace-backup-<дата>
```

Восстановление — в пустой каталог при остановленной задаче (`Stop-ScheduledTask`), команда `restore <каталог>`; затем укажите новый `data_dir` в конфигурации.

## 4. Что проверено на Windows в этой итерации

| Что | Результат |
|---|---|
| Сборка `opentrace` (ядро) MSVC | ✅ после исправлений W1–W3 |
| `cargo test` ядра | ⚠️ HTTP-golden ✅; известные различия W6 (1 ULP), W7 (защита от перезаписи), W8 (текст ошибки ОС) |
| OpenTrace Server: 19 модульных + 8 API-тестов, release-бинарник | ✅ |
| Desktop: сборка и самотест в WebView2 | ✅ PASS |
| Desktop + Server по HTTP, два клиента (выгрузка и дедупликация FSA) | ✅ PASS |
| Установщики NSIS/MSI, задача планировщика, 2 монитора, печать | не проверено |
