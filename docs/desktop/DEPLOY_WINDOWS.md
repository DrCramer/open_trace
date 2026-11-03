# OpenTrace Desktop и OpenTrace Server на Windows

Windows 10/11 x64 (клиент) и Windows Server 2019+/Windows 10/11 x64 (сервер). Docker не нужен.

> Исследовательское ПО. Не валидировано для диагностики и судебно-экспертной работы. Численные результаты на Windows совпадают с Linux побитово на golden-корпусах (порт `exp`/`log`/`pow` glibc, см. `BASELINE_AUDIT.md`, W6); имена образцов, различающиеся только регистром, на Windows не сохраняются (W7).

## 1. Инструменты сборки

1. Visual Studio Build Tools 2022 с компонентом «Разработка классических приложений на C++» (MSVC + Windows SDK).
2. Rust: https://rustup.rs → `rustup-init.exe` (toolchain `stable-x86_64-pc-windows-msvc`).
3. Git for Windows; Node.js 18+ (только для сборки установщика десктопа).
4. WebView2 Runtime (в Windows 11 уже есть; на Windows 10 установщик десктопа скачает его сам). **Нужна актуальная версия**: с WebView2 Runtime 131 (образ GitHub windows-2022) окна графиков/отчётов не получают содержимое приложения; со 142 работает. Обычно Evergreen обновляется сам; если в организации обновления WebView2 заблокированы, обновите его вручную. При такой ошибке десктоп закрывает пустое окно и сообщает версию WebView2.

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

### Установщик (рекомендуется)

Артефакт `opentrace-server-Windows` запуска GitHub Actions содержит `OpenTrace-Server-<версия>-x64-setup.exe` (NSIS; сценарий `server/opentrace-server/packaging/windows/opentrace-server.nsi`). Запуск от администратора:

* программа — в `C:\Program Files\OpenTrace Server` (exe, `resources\`, скрипты);
* конфигурация — `C:\ProgramData\OpenTrace\server.toml`, создаётся один раз из `server.windows.toml` и **не перезаписывается** при обновлении (по умолчанию: только этот компьютер, `127.0.0.1:8770`, TLS через обратный прокси; для HTTP в изолированной сети см. комментарии в файле и §4 DEPLOY_LINUX);
* данные — `C:\ProgramData\OpenTrace\server`, копии — `C:\ProgramData\OpenTrace\backups`;
* служба `OpenTraceServer` регистрируется (`service-install`: `NT SERVICE\OpenTraceServer`, автозапуск, перезапуск при сбое) и **остаётся остановленной**, пока не задан пароль администратора: последняя страница установщика запускает `set-admin-password.cmd` (пароль `Administrator`, затем запуск службы). Позже — тот же файл из папки программы от администратора.

Обновление — тот же установщик поверх: работающая служба останавливается, файлы заменяются, служба запускается снова; конфигурация, пользователи и данные сохраняются. Удаление — «Приложения» Windows или `uninstall.exe`: служба и файлы программы удаляются, `C:\ProgramData\OpenTrace` остаётся (удалите сами после последней резервной копии). Тихий режим: `setup.exe /S`, удаление `uninstall.exe /S`.

Проверено в CI (windows-2022) при каждом запуске: тихая установка → служба зарегистрирована и остановлена → пароль → запуск, health и вход → установка поверх работающей службы (правка в `server.toml` сохранилась, вход тем же паролем) → тихое удаление (служба и программа удалены, база и конфигурация на месте). Установщик не подписан: SmartScreen покажет предупреждение (подпись — вместе с подписью десктопа).

### Ручная установка

Готовая папка без сборки: артефакт `opentrace-server-Windows` запуска GitHub Actions содержит `opentrace-server-windows\` (exe, `resources\`, пример конфигурации, скрипты из `packaging\windows`). Скопируйте её в `C:\Program Files\OpenTrace Server` и продолжайте с настройки `server.toml` ниже. Сборка из исходников:

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

### Служба Windows

`opentrace-server.exe` сам регистрируется в диспетчере служб (SCM). В PowerShell **от администратора**:

```powershell
& "$dst\opentrace-server.exe" service-install --config C:\ProgramData\OpenTrace\server\server.toml
sc.exe start OpenTraceServer
Invoke-RestMethod http://127.0.0.1:8770/api/v1/health
Get-Content C:\ProgramData\OpenTrace\server\logs\server.log -Tail 20
```

Что делает `service-install`: служба `OpenTraceServer` (имя меняется `--name`), автозапуск, запуск под виртуальной учётной записью `NT SERVICE\OpenTraceServer` (без пароля, без прав администратора), перезапуск при сбое через 10 с / 60 с / 5 мин (в том числе при выходе с ошибкой), права «изменение» только на `data_dir` (через `icacls`; на файл конфигурации — чтение, если он вне `data_dir`). Остановка `sc.exe stop OpenTraceServer` завершает текущие запросы и выходит штатно. Журнал — `<data_dir>\logs\server.log`; если не прочитана конфигурация — `opentrace-server-service.log` в `%TEMP%` учётной записи службы (у виртуальной учётной записи обычно `C:\Windows\ServiceProfiles\OpenTraceServer\AppData\Local\Temp`) и запись в журнале событий «System» о неудачном запуске.

Удаление (каталог данных не трогается): `& "$dst\opentrace-server.exe" service-uninstall`. Обновление: `sc.exe stop`, замена exe и `resources`, `sc.exe start` (путь к exe записан в службе — не меняйте каталог установки без переустановки службы).

`service-run` запускает только SCM; из консоли команда сразу завершается с сообщением. Для пробного запуска используйте `serve`.

Проверено в CI (GitHub windows-2022, PowerShell от администратора) на каждом запуске: `service-install` (учётная запись `NT SERVICE\<имя>`, автозапуск, перезапуск 10 с / 60 с / 5 мин), `sc.exe start` → `/api/v1/health` и вход администратора, принудительное завершение процесса → SCM перезапускает службу, `sc.exe stop` → в журнале «stop requested by the Service Control Manager», `service-uninstall` → служба удалена, `opentrace.sqlite3` на месте. На сервере с доменными групповыми политиками выполните ту же пробу до внедрения.

Альтернатива без регистрации службы — задача планировщика (`packaging\windows\install-scheduled-task.ps1`, запуск под `NT AUTHORITY\LOCAL SERVICE`); удаление: `Unregister-ScheduledTask -TaskName "OpenTrace Server" -Confirm:$false`.

### HTTPS на Windows

Варианты: Caddy для Windows (тот же `Caddyfile`), IIS + Application Request Routing (обратный прокси на `http://127.0.0.1:8770`) с сертификатом внутреннего CA. Брандмауэр открывайте только для порта прокси (443), не для 8770.

### Резервное копирование

```powershell
& "$dst\opentrace-server.exe" backup --config C:\ProgramData\OpenTrace\server\server.toml --out D:\Backups\OpenTrace
& "$dst\opentrace-server.exe" verify-backup D:\Backups\OpenTrace\opentrace-backup-<дата>
```

Восстановление — в пустой каталог при остановленной службе (`sc.exe stop OpenTraceServer`), команда `restore <каталог>`; затем укажите новый `data_dir` в конфигурации.

## 4. Что проверено на Windows в этой итерации

Windows 11 x64, rustc 1.94 MSVC, WebView2 142, обычный (не повышенный) пользователь.

| Что | Результат |
|---|---|
| Сборка ядра MSVC, `cargo test --release` ядра | ✅ все наборы, включая побитовые golden (после порта glibc-математики; в `idx_projects` не сравниваются 6 из 46 сессий, эталон которых невозможен на NTFS — W7/W7c) |
| Отпечаток `libm_port` (10⁶ аргументов exp/log/pow) | ✅ совпадает с Linux |
| OpenTrace Server: 19 модульных + 8 API-тестов, release-бинарник | ✅ |
| Server: `service-run` из консоли / `service-install` без прав администратора | ✅ понятный отказ (код 1); **регистрация и запуск службы не проверены** — нужен повышенный PowerShell |
| Desktop: сборка и самотест в WebView2 | ✅ PASS |
| Desktop + Server по HTTP, два клиента (выгрузка и дедупликация FSA) | ✅ PASS |
| Установщики NSIS (4,7 МБ) и MSI (6,7 МБ) | ✅ собраны; NSIS: тихая установка для текущего пользователя → самотест установленного exe (ресурсы из пакета, дерево исходников скрыто) PASS → тихое удаление; MSI не устанавливался (нужны права администратора) |
| Пакетный анализ 1/48/96 синтетических FSA (`bench_batch`) | ✅ см. `ROADMAP.md` |
| 2 монитора, печать, подпись установщиков | не проверено |
