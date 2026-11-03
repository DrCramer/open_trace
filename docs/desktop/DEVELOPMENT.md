# Разработка: Desktop, Server, ядро

## Рабочая копия и ветки

* Ветка: `feature/desktop-server-platform`, ответвлена от `origin/feature/per-marker-gate-min-call-height` @ `102f71a`. Исходная ветка и `main` не изменяются; синхронизация с исходной веткой — только отдельным осознанным merge после проверки тестов.
* Рекомендуемая изоляция — отдельный worktree рядом с основной копией:

```bash
git fetch origin
git worktree add ../open_trace_desktop feature/desktop-server-platform
cd ../open_trace_desktop
git branch --unset-upstream 2>/dev/null; git config push.default current   # чтобы `git push` не ушёл в исходную ветку
```

## Три независимых Cargo workspace

| Каталог | Что | Команды |
|---|---|---|
| `core/` | ядро + `opentrace` (Docker-образ, `--locked`) | `cargo test --release --manifest-path core/opentrace-core/Cargo.toml` |
| `server/opentrace-server/` | OpenTrace Server | `cargo test --manifest-path server/opentrace-server/Cargo.toml` |
| `desktop/opentrace-desktop/src-tauri/` | OpenTrace Desktop | `cargo test --manifest-path desktop/opentrace-desktop/src-tauri/Cargo.toml` |

Отдельные workspace сделаны намеренно: GUI-зависимости (Tauri, WebKitGTK) и axum не попадают в `core/Cargo.lock`, который использует Docker-сборка прежнего сервиса.

## Ядро: правила

* Математику не копировать; desktop и server вызывают `opentrace_core` (`server::App::handle`, `server_analyze::analyze`, …).
* Все трансцендентные функции — через `core/opentrace-core/src/cmath.rs`. Не использовать `f64::ln/exp/powf/sin` напрямую: это обходит единую точку и ломает переносимость.
* Любое изменение, влияющее на числа, должно проходить golden-тесты побитово на Linux **и** Windows. По умолчанию `exp`/`log`/`pow`/`expm1` — `libm_port.rs` (порт glibc 2.39, вариант с FMA: ARM optimized-routines и fdlibm); `sin`, `frexp`, `ldexp` — из системной libm (`frexp`/`ldexp` точны, `sin` анализу не важен, см. `cmath.rs`). Не переносить в крейт код под LGPL (в том числе `sin` из glibc). Признак `system-libm` возвращает всё к системной libm (только для сравнения), `portable-math` — экспериментальный крейт `libm` (расходится с эталоном в 6 корпусах), в сборки не включать.
* Проверка порта: `LIBM_PORT_CASES=100000000 cargo test --release --test libm_port` (Linux x86-64 с FMA: побитовое сравнение с glibc; 10⁸ случаев — 0 расхождений) и отпечаток SHA-256 на 10⁶ случаев (`identical_results_on_every_platform`) — один и тот же на Linux и Windows.
* Таблицы `libm_port_data.rs` сгенерированы из исходников glibc (`sysdeps/ieee754/dbl-64/e_{exp,log,pow}_data.c`) и не редактируются вручную. Места, где GCC сливает умножение и сложение в FMA, взяты из GIMPLE-дампа (`-O2 -mfma -fdump-tree-widening_mul`) и записаны в Rust как `a.mul_add(b, c)`; любое «упрощение» формул ломает побитовое совпадение.
* Файловые системы без учёта регистра (Windows, macOS): сохранение образца, id которого отличается от существующего только регистром, отклоняется (W7). Golden-корпус `idx_projects` на таких системах пропускает сессии с такими именами (в т.ч. из импортируемых пакетов) и сравнивает остальные побитово; отдельный тест `case_insensitive_fs.rs` проверяет сам отказ.
* Время и пути — `localtime.rs`, `paths.rs` (никаких `libc::localtime_r`, `HOME`, `/dev/urandom` напрямую).

## Desktop

Запуск в режиме разработки (UI берётся из `resources/web` дерева исходников):

```bash
cargo run --manifest-path desktop/opentrace-desktop/src-tauri/Cargo.toml
```

Переменные окружения:

| Переменная | Назначение |
|---|---|
| `OPENTRACE_DESKTOP_DATA_DIR` | каталог данных вместо `%LOCALAPPDATA%`/`~/.local/share` (изолированный профиль) |
| `OPENTRACE_DESKTOP_SELFTEST=<file.json>` | самотест в реальном webview, отчёт в файл, выход с кодом 0/1 |
| `OPENTRACE_RESOURCES` | каталог `web/` + `data/` (иначе ресурсы пакета или дерево исходников) |

Самотест на Linux без дисплея:

```bash
OPENTRACE_DESKTOP_SELFTEST=$PWD/selftest.json xvfb-run -a desktop/opentrace-desktop/src-tauri/target/debug/opentrace-desktop
cat selftest.json   # "pass": true
```

Что проверяет самотест: загрузка `/idx/` по схеме `opentrace`, `window.OT`, `localStorage`, `/api/health`, **анализ синтетического ABIF ядром** (`/api/analyze`, бинарное тело, статус 200, каналы и пики), загрузка примера проекта, `OT.displayPlots()` → нативное окно `plot-N` через `window.open`, в нём `window.opener.OT.plotPayload()` и непустой canvas, состояние очереди синхронизации.

Отладка webview: в debug-сборке доступны инструменты разработчика (ПКМ → Inspect / F12 в WebView2).

Файлы десктопа:

| Файл | Роль |
|---|---|
| `src/bridge.rs` | схема `opentrace://`: ядро и `/desktop/api/*` |
| `src/windows.rs` | главное окно, `window.open` → нативные окна, навигация, загрузки |
| `src/remote.rs` | клиент API v1 (TLS ОС, токен в хранилище ОС) |
| `src/sync.rs` | кэш FSA и фоновая отправка |
| `src/pool.rs` | пул потоков анализа |
| `src/workspace.rs` | проекты на сервере (`project_source = "server"`): lease, heartbeat, только чтение, сохранение с ревизией и fencing-токеном, FSA по `sha256` |
| `src/provenance.rs` | журнал анализов сеанса → записи происхождения при сохранении в серверный проект |
| `src/labprofile.rs` | активный лабораторный профиль: проверка хешей, ожидаемые настройки, сравнение с отправленными |
| `ui/shim.js` | внедряется во все окна: синхронизация после анализа, статус сервера и проекта, баннер «только чтение», закрытие |
| `ui/connection.*` | страница «Сервер организации» (адрес, вход, источник проектов) |
| `ui/standards.*` | «Лабораторные стандарты»: список версий, импорт локальных справочников как черновиков, отправка/утверждение, профили |
| `ui/selftest.js` | сценарий самотеста |

Самотест с сервером (дополнительно: вход, режим «проекты на сервере», сохранение и повторное открытие проекта, совпадение SHA-256 стандарта у клиента и сервера):

```bash
OPENTRACE_SELFTEST_SERVER_URL=http://127.0.0.1:8770 OPENTRACE_SELFTEST_USER='Casework Analyst' \
OPENTRACE_SELFTEST_PASSWORD=... OPENTRACE_DESKTOP_SELFTEST=$PWD/selftest.json \
  xvfb-run -a desktop/opentrace-desktop/src-tauri/target/debug/opentrace-desktop
```

В режиме самотеста токен не сохраняется в хранилище учётных данных ОС.

Нагрузочный режим самотеста — `OPENTRACE_DESKTOP_SELFTEST_BATCH=N` (1…500): N выдуманных файлов по 10 000 сканов (`demo::synthetic_run_fsa`) проходят через собственные функции UI «Добавить образцы» и «Анализ» (4 запроса одновременно), затем все открываются в окне графиков. В отчёте: время анализа, время разбора JSON на главном потоке, объём ответов, память (Linux — RSS дерева процессов WebKit, WebView2 — куча JS), готовность окна графиков. С сервером и `OPENTRACE_SELFTEST_PROFILE=<имя>:<версия>` пакет ещё активирует утверждённый профиль, сохраняется как серверный проект, и для каждого образца требуется запись происхождения со ссылкой на профиль. Пример подготовки сервера: создать и утвердить метод, стандарт длин, набор, бины и профиль двумя пользователями (автор и утверждающий), затем:

```bash
OPENTRACE_SELFTEST_SERVER_URL=http://127.0.0.1:18771 OPENTRACE_SELFTEST_USER='Casework Analyst' OPENTRACE_SELFTEST_PASSWORD=... \
OPENTRACE_SELFTEST_PROFILE='Lab GF:1' OPENTRACE_DESKTOP_SELFTEST_BATCH=8 OPENTRACE_DESKTOP_SELFTEST=$PWD/st.json \
  xvfb-run -a desktop/opentrace-desktop/src-tauri/target/debug/opentrace-desktop
```

Ядро без webview: `cargo run --release --example bench_batch -- 1 48 96` (в `core/`).

Генотипирование через интерфейс — `OPENTRACE_DESKTOP_SELFTEST_GENOTYPE=N` (1…96): синтетический набор (24 маркера, `demo::synthetic_kit_texts`) импортируется тем же кодом, что «Менеджер панелей», добавляются аллельная лестница и N образцов, «Анализ» выполняет калибровку размеров, лестницу и генотипирование, затем каждый вызов сравнивается с аллелями, заложенными в файлы. С сервером и профилем (`desktop/opentrace-desktop/tests/e2e/setup_lab_profile.py URL PASSWORD resources/data/shipped_methods.json kit.json`, где `kit.json` — вывод `cargo run --example synthetic_kit_store` в `core/`) профиль активируется до сборки набора, проект сохраняется на сервер, и для лестницы и каждого образца требуется запись происхождения со ссылкой на профиль. Без интерфейса тот же путь проверяет `cargo run --release --example synthetic_genotype -- 8` (8 образцов × 24 маркера). Всё это — выдуманные сигналы, не лабораторные данные.

### Диагностика зависания на Windows

Если окно стало белым и «Не отвечает», снимите дамп процесса до того, как закрывать его (PowerShell от того же пользователя, права администратора не нужны):

```powershell
$sig='using System;using System.Runtime.InteropServices;using Microsoft.Win32.SafeHandles;public static class MD{[DllImport("dbghelp.dll",SetLastError=true)]public static extern bool MiniDumpWriteDump(IntPtr h,uint pid,SafeFileHandle f,uint t,IntPtr e,IntPtr u,IntPtr c);}'
Add-Type -TypeDefinition $sig
$p = Get-Process opentrace-desktop
$f = [IO.File]::Create("$env:USERPROFILE\Desktop\opentrace-hang.dmp")
[MD]::MiniDumpWriteDump($p.Handle, [uint32]$p.Id, $f.SafeFileHandle, 0x1045, [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero); $f.Close()
```

В дампе нет данных проектов, кроме того, что было в памяти процесса (могут быть фрагменты FSA и генотипов) — передавайте его как служебную информацию. Разбор: `minidump-stackwalk` (crate `minidump-stackwalk`) с символами из `dump_syms` для `opentrace_desktop.pdb` (артефакт CI `opentrace-desktop-Windows-symbols` той же сборки) и для системных DLL, скопированных с той же машины (`C:\Windows\System32\ntdll.dll` и др.): без символов системных DLL стек главного потока не раскручивается.

## Server

```bash
cargo run --manifest-path server/opentrace-server/Cargo.toml -- serve       # 127.0.0.1:8770, transport=proxy
OPENTRACE_SERVER_DATA_DIR=/tmp/ots OPENTRACE_NEW_PASSWORD='secret-pw-1' \
  cargo run --manifest-path server/opentrace-server/Cargo.toml -- set-password Administrator
```

Тесты (`cargo test`): модульные (конфигурация, миграции и неизменяемость, lease: гонка 8 соединений, истечение, fencing, принудительный сброс, перезапуск; хранилище FSA; стандарты и профили) и интеграционные через настоящий маршрутизатор (`tests/api.rs`): вход/выход/лимит, два редактора, конфликт ревизий, отказ WEB-записи при чужой lease, принудительный сброс администратором, FSA-дедупликация и проверка хеша, привязка FSA и `save_sample` по `fsa_sha256`, жизненный цикл стандартов и профилей, provenance, перезапуск с сохранением lease, резервная копия и восстановление. `tests/crash.rs` (Unix): настоящий процесс сервера убивается SIGKILL во время параллельной выгрузки FSA и записи проекта под блокировкой; после перезапуска проверяются целостность SQLite, соответствие каждого FSA своему SHA-256, отсутствие незавершённых файлов и продолжение работы.

Два пользователя веб-интерфейса на одном проекте (настоящий Chromium через Playwright, веб-интерфейс без изменений): второй не может затереть изменения первого, «Сохранить как» и сохранение после повторного открытия работают:

```bash
OT_URL=http://127.0.0.1:8770 OT_PASSWORD=... node server/opentrace-server/tests/e2e/two_web_users.cjs   # PASS
```

Ручная проверка API:

```bash
T=$(curl -s -X POST localhost:8770/api/v1/auth/login -H 'content-type: application/json' \
     -d '{"username":"Administrator","password":"secret-pw-1"}' | jq -r .token)
curl -s localhost:8770/api/v1/me -H "authorization: Bearer $T"
curl -s -X POST localhost:8770/api/v1/projects/Demo/lease -H "authorization: Bearer $T"
SHA=$(sha256sum sample.fsa | cut -d' ' -f1)
curl -s -X PUT --data-binary @sample.fsa localhost:8770/api/v1/fsa/$SHA -H "authorization: Bearer $T"
```

## Проверки, выполненные при создании ветки

См. раздел «Состояние» в `ROADMAP.md` — там перечислено, какие команды реально запускались, на какой платформе и с каким результатом.

## Данные и Git

Не добавлять в репозиторий реальные FSA, проекты, базы, резервные копии, токены, сертификаты, каталоги `runtime/`, `fsa-cache/`, `target/`. Тесты используют только синтетический ABIF (`opentrace_core::demo::demo_bytes`) и временные каталоги.
