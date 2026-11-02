# Базовый аудит: что есть в OpenTrace до desktop/server-платформы

Ветка `feature/desktop-server-platform` ответвлена от `origin/feature/per-marker-gate-min-call-height` @ `102f71a` (1 ноября 2026). Аудит проводился по коду этой ревизии, сборки и тесты запускались реально (Linux x86_64, rustc 1.97; Windows 11 x64, rustc 1.94 MSVC + VS Build Tools 2022).

## 1. Структура и сборка

| Часть | Где | Факт |
|---|---|---|
| Расчётное ядро, HTTP-сервер, CLI | `core/opentrace-core` (один crate, ~32 тыс. строк Rust, 64 модуля) | бинарник `opentrace` (`serve`, `str-run`, `project …`, …) |
| Веб-интерфейс ID-X | `resources/web/idx/*.js|html|css` (26 файлов, ~830 КБ, без сборщика и зависимостей) | раздаётся сервером из памяти; `/idx/` |
| Поставочные данные | `resources/data` (методы, настройки таблиц) | |
| Тесты | `core/opentrace-core/tests` + `tests/golden` (ответы Python-эталона, побитовое сравнение) | 9 тестовых бинарников, 53 программы-дампа в `examples/` |
| Развёртывание | `Dockerfile`, `docker-compose.yml`, `scripts/*.sh` | только Linux/Docker |

Workspace `core/Cargo.toml` и `core/Cargo.lock` используются Docker-сборкой с `--locked`; новые крейты (desktop, server) вынесены в отдельные workspace, чтобы не тянуть GUI-зависимости в серверный образ.

## 2. Baseline-прогоны (исходный коммит, без изменений)

| Платформа | Сборка `opentrace` | `cargo test --release` |
|---|---|---|
| Linux x86_64 (облачная среда) | ✅ | ✅ все 9 наборов зелёные (golden, http_golden, cli_golden, server_sockets и др.) |
| Windows 11 x64, MSVC (ваш ПК) | ❌ **не компилируется**: `localtime_r` в `idx_projects.rs`, `codis.rs` (только glibc) | — |

## 3. Несовместимости с Windows (найдены и проверены на реальной машине)

| # | Место | Проблема | Статус в ветке |
|---|---|---|---|
| W1 | `idx_projects.rs::strftime_now`, `codis.rs::now_stamp` | `libc::localtime_r` / свой `extern localtime_r` с glibc-раскладкой `struct tm` — ошибка компиляции под MSVC | ✅ `src/localtime.rs`: на Unix прежний `localtime_r`, на Windows `_localtime64_s` (UCRT) |
| W2 | `project_store.rs::new_uuid` | `/dev/urandom`; на Windows молча деградировал до UUID из времени | ✅ `getrandom` (BCryptGenRandom) |
| W3 | `server.rs`, `idx_projects.rs`, `security.rs` | `HOME` для путей по умолчанию (на Windows обычно не задан → `/` или `.`) | ✅ `src/paths.rs::home_dir`: `HOME`, затем `USERPROFILE` |
| W4 | git checkout с `core.autocrlf=true` | `resources/web/*` получают CRLF → `GET /idx` отличается от эталона | ✅ `.gitattributes` (LF для текстов, бинарные `*.gz`) |
| W5 | тестовые обвязки `ip_dump`, `http_golden`, `cli_golden` | пути с `\` в выводе, JSON-экранирование `\\` | ✅ нормализация только под `cfg(windows)`; на Linux вывод не меняется |
| **W6** | **`exp`/`pow`/`log` из системной libm** | **UCRT ≠ glibc на 1 ULP**: golden `morphology` (19 строк), `spike` (1 строка) | ✅ `libm_port.rs` — побитовый порт glibc (вариант FMA), см. §4; на Windows оба корпуса совпадают |
| **W9** | та же libm на Linux | результат glibc **зависит от процессора**: на CPU без FMA (старые Xeon, VM с моделью `qemu64`) glibc выбирает другой вариант `exp/log/pow` (тысячи различий на 10⁷ аргументов) | ✅ тот же порт: результат одинаков на любом CPU |
| **W7** | `idx_projects.rs::save_sample` | **потеря данных на NTFS/APFS**: id образцов `S1` и `s1` дают один файл (`S1.fsa` = `s1.fsa`), второй молча затирал первый (найдено golden-тестом `idx_projects` на Windows) | ✅ на Windows/macOS запись отклоняется с сообщением; Linux без изменений |
| W7b | папки проектов `Alpha`/`alpha` | на NTFS вторая папка получает суффикс `_2` (проверка `exists()` регистронезависима) — данные не теряются, отличается только имя папки в дампе | принято |
| W7c | id образца с символами, недопустимыми в именах файлов Windows (`\n`, `<>:"\|?*`, точка/пробел в конце, `NUL`, `COM1`…) | на Linux сохраняется, на Windows — ошибка ОС при записи; данные не теряются, пользователь видит отказ | принято; полное решение — то же кодирование имён файлов, что и для W7 |
| W8 | CLI на Windows | `[Errno 2] Не удается найти указанный файл` (код и текст Windows) вместо питоновского `[Errno 2] No such file or directory` | ✅ CLI выдаёт POSIX-текст на любой ОС |

Все math-вызовы собраны в `src/cmath.rs` (раньше — `extern "C"` в 4 модулях и `f64::ln/exp/exp_m1/sin` в 2 модулях). Поведение на Linux не изменилось: полный набор тестов ядра после рефакторинга проходит побитово (см. §6).

## 4. Численная переносимость (W6, W9) — решено

Ядро обещает побитовое совпадение с эталоном Python/glibc. Golden-тесты на Windows показали расхождения в 1 ULP (`morphology`: 19 строк, `spike`: 1 строка) — `exp`/`pow` Windows UCRT дают другой последний бит.

Исследование (харнесс на C против системной glibc 2.39, 10⁷ аргументов на функцию):

| Реализация | Различий с glibc на CPU с FMA |
|---|---|
| ARM optimized-routines, сборка **без** FMA | exp 6 826, log 47, pow 7 109 |
| ARM optimized-routines, сборка `-mfma -ffp-contract=fast` | **0 / 0 / 0** |
| Rust-крейт `libm` (musl) | 6 golden-корпусов расходятся |

Вывод: эталон соответствует варианту glibc, собранному с FMA-сжатием (его glibc выбирает на любом x86-64 с FMA). На машинах без FMA даже Linux-сервер дал бы другие биты (W9).

Решение — `core/opentrace-core/src/libm_port.rs`: `exp`, `log`, `pow` перенесены из ARM optimized-routines (тот же код, что в glibc ≥ 2.28; MIT OR Apache-2.0) **оператор в оператор по оптимизированному GIMPLE GCC** (`-fdump-tree-widening_mul`): каждое `.FMA(a,b,c)` — это `a.mul_add(b,c)` (одно округление на любой платформе). Таблицы (`libm_port_data.rs`) сгенерированы из C-структур побитово. `cmath.rs` использует порт по умолчанию; `expm1` и `sin` пока из C-библиотеки (`sin` нужен только генератору синтетических данных).

Проверка: `tests/libm_port.rs` — 0 различий с glibc на 10⁸ аргументов × 3 функции (Linux, FMA); SHA-256-отпечаток результатов на 10⁶ аргументов одинаков на Linux и Windows; полный golden-набор: Linux без изменений, Windows — `morphology` и `spike` совпадают с эталоном.

## 5. Архитектурные особенности текущей реализации

* `App::handle(&Request) -> Response` в `server.rs` не зависит от сокетов — это точка встраивания: десктоп и новый сервер вызывают его напрямую.
* Проекты: файловая библиотека `ProjectLibrary` (`<root>/<name>/project.json`, `samples/*.json.gz`, FSA внутри в base64 при сохранении через `/api/idx-project` `save_sample.fsa_b64`). Атомарная запись через временный файл + `rename`.
* Пользователи/права/аудит: `SecurityStore` (JSON-документ + журнал аудита в каталоге security), PBKDF2, блокировка после неудачных входов, электронные подписи. По умолчанию `require_login = false` (веб-режим без входа).
* Окна UI: Plot, Panel Manager, Report Manager, печать открываются `window.open` и работают через **синхронный** `window.opener.OT.*` (`plot.js` — 27 обращений). Это требует, чтобы дочернее окно было настоящим popup того же движка.
* Лабораторные справочники (методы `opentrace.methods.v1`, киты/панели/бины/статтеры `opentrace.kits.v1`, стандарты длин `opentrace.sizestd.v1`, матрицы, настройки графиков/таблиц) — в `localStorage` браузера. Источник истины не централизован.
* FSA для анализа отправляется **бинарным** телом `POST /api/analyze` (не base64) — хорошо. Сохранение проекта — base64 в JSON (`fsa_b64`) — узкое место памяти и трафика.
* `/api/str-run` с `slim` и `ladder_set` уже уменьшают ответы (сокращение событий pull-up до счётчика) — учтённый урок прошлых проблем с памятью.
* Ограничения тела: 64 МиБ для FSA, **1 ГиБ** для JSON (`MAX_JSON_BYTES`) — избыточно, сохранено ради совместимости.
* Сеть: HTTP/1.0 thread-per-connection, проверка `Host` (DNS rebinding), `X-Session-Token` в страницах, `Origin`-проверка. TLS нет (ожидается прокси).

## 6. Результаты после изменений этой ветки

| Проверка | Результат |
|---|---|
| `cargo test --release` ядра на Linux после `localtime`/`paths`/`cmath`/`.gitattributes`/W7 | ✅ все наборы, побитово |
| Сборка `opentrace` на Windows MSVC | ✅ (`BUILD_EXIT=0`, ваш ПК, 1 мин 22 с) |
| Golden на Windows (ПК заказчика) | ✅ после `libm_port` и исправления CLI; в корпусе `idx_projects` на регистронезависимой ФС не сравниваются сессии, эталонное дерево файлов которых невозможно на такой ФС: 5 из 46 (пары путей, различающихся только регистром, W7), на Windows ещё 1 (id образца с переводом строки, W7c). Список вычисляется из эталона и закреплён тестом; отказ сохранения проверяет отдельный тест `case_insensitive_fs` |

## 7. Узкие места и риски, учтённые в архитектуре

1. **Память на больших наборах**: UI держит Blob каждого FSA и полные `analysis` в памяти вкладки. Десктоп не добавляет копий в JS; кэш FSA и очередь синхронизации — в Rust, на диске, с лимитом размера.
2. **Base64 FSA при сохранении проекта** — API v1 принимает `fsa_sha256` вместо `fsa_b64` (файл уже лежит в контентном хранилище сервера).
3. **Два источника истины для стандартов** (localStorage разных ПК) — серверные версионированные стандарты и лабораторные профили (этап D, API готов; миграция localStorage — следующий шаг).
4. **Отсутствие блокировок** — lease/fencing на сервере; WEB-запись при чужой lease отклоняется.
5. **Численная переносимость** — §4.
