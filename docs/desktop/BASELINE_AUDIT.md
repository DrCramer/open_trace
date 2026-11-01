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
| **W6** | **`exp`/`pow`/`log`/`sin` из системной libm** | **UCRT ≠ glibc на 1 ULP**: golden `morphology` (19 строк), `spike` (1 строка) | ⚠️ **открыто**, см. §4 |

Все math-вызовы собраны в `src/cmath.rs` (раньше — `extern "C"` в 4 модулях и `f64::ln/exp/exp_m1/sin` в 2 модулях). Поведение на Linux не изменилось: полный набор тестов ядра после рефакторинга проходит побитово (см. §6).

## 4. Численная переносимость (главный риск Windows-клиента)

Ядро обещает побитовое совпадение с эталоном Python/glibc. Golden-тесты на Windows показали:

* `morphology`: 19 различающихся строк, пример `height = 0x4086f8a45c6da8f7` (Linux) vs `…f8` (Windows) — 1 ULP;
* `spike`: высота модели пика и `excess_area_ratio` — 1 ULP.

Причина: `exp`/`pow` в Windows UCRT дают другой последний бит, чем glibc, на части аргументов. Практическое значение ничтожно (~1e-13 RFU), но **гарантия «Windows-десктоп = Linux-сервер бит в бит» сейчас не выполняется**, и в редком пограничном случае порог (например, `excess_area_ratio` у границы) теоретически может сработать по-разному.

Проверенная альтернатива — опция `portable-math` (чистый Rust `libm`, порт musl): на Linux расходится с glibc уже в **6** корпусах (`calibration`, `morphology`, `post_sizing_quality`, `restore`, `spike`, `sizing_value`), то есть хуже. Поэтому опция выключена и помечена как экспериментальная.

Решение (этап E в ROADMAP): побитовый порт `exp`/`log`/`pow` из glibc ≥ 2.28 (код ARM optimized-routines, лицензия MIT/Apache-2.0) в Rust с тем же порядком операций, без FMA, с проверкой на Linux (golden + 10⁸ случайных аргументов против glibc) и включением на всех платформах. До этого: десктоп показывает версию математики (`math_backend`) рядом с версией алгоритма, а в `ID_X_PLOT_PARITY.md`/документации указано, что результаты Windows-клиента не заявлены как побитово равные серверным.

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
| `cargo test --release` ядра на Linux после `localtime`/`paths`/`cmath`/`.gitattributes` | см. `DEVELOPMENT.md` §«Проверки» (зелёные, побитово) |
| Сборка `opentrace` на Windows MSVC | ✅ (`BUILD_EXIT=0`, ваш ПК, 1 мин 22 с) |
| Golden на Windows | остаются расхождения W6 (1 ULP); CRLF/пути устранены — подтверждение повторным прогоном в CI |

## 7. Узкие места и риски, учтённые в архитектуре

1. **Память на больших наборах**: UI держит Blob каждого FSA и полные `analysis` в памяти вкладки. Десктоп не добавляет копий в JS; кэш FSA и очередь синхронизации — в Rust, на диске, с лимитом размера.
2. **Base64 FSA при сохранении проекта** — API v1 принимает `fsa_sha256` вместо `fsa_b64` (файл уже лежит в контентном хранилище сервера).
3. **Два источника истины для стандартов** (localStorage разных ПК) — серверные версионированные стандарты и лабораторные профили (этап D, API готов; миграция localStorage — следующий шаг).
4. **Отсутствие блокировок** — lease/fencing на сервере; WEB-запись при чужой lease отклоняется.
5. **Численная переносимость** — §4.
