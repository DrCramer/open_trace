# OpenTrace — открытая лаборатория электрофореграмм

Веб-приложение для анализа STR-электрофореграмм (`.fsa`): калибровка по стандарту длин, вызов аллелей, контроль качества, таблицы и графики, проекты, пользователи и журнал аудита. Интерфейс повторяет рабочее пространство GeneMapper ID-X (`/idx/`). Расчётное ядро, веб-сервер и командная строка написаны на Rust и собираются в один бинарник `opentrace` (Python не нужен). Проект запускается через Docker Compose.

> **Тестовая сборка.** Это исследовательский проект, независимая реализация. Он не заменяет сертифицированное ПО и не проходил валидацию для рабочих лабораторных данных. Реальные образцы, проекты и резервные копии в GitHub не публикуются: каталог `runtime/` исключён из Git.

## Требования

- Linux (целевая платформа — Ubuntu Server) с Docker Engine и Docker Compose v2 (`docker compose`).
- Git и Bash.
- Свободный порт `8765` (меняется параметром `WEB_PORT`).
- Доступ сервера в интернет при первой сборке: образы Docker Hub и `crates.io`.
- Сборка Rust занимает несколько минут и временно требует около 2 ГБ места; последующие сборки используют кэш Docker.

## Быстрая установка

```bash
git clone https://github.com/DrCramer/open_trace.git
cd open_trace
./scripts/install.sh
```

Скрипт создаёт `.env`, каталоги `runtime/projects`, `runtime/security` и `runtime/backups`, собирает образ, запускает контейнер и ждёт состояния `healthy`. Повторный запуск безопасен: `.env` и данные сохраняются.

По умолчанию сайт доступен только на самом сервере: **http://localhost:8765/idx/**. Здесь `/idx/` — рабочее пространство (проекты, таблицы, графики), корневой адрес `/` — исследовательский просмотрщик одного файла.

Проверка:

```bash
docker compose ps
curl -f http://localhost:8765/api/health
docker compose logs --tail=50 opentrace
```

> Если репозиторий приватный, на сервере понадобится доступ к GitHub: deploy key (`ssh-keygen`, затем *Settings → Deploy keys* репозитория, клонирование по `git@github.com:DrCramer/open_trace.git`) либо personal access token. Для публичного репозитория ничего не нужно.

## Доступ из локальной сети

Укажите адрес сервера **до первой установки**:

```bash
WEB_BIND_IP=0.0.0.0 ALLOWED_HOSTS=localhost,127.0.0.1,192.168.1.16 ./scripts/install.sh
```

Замените `192.168.1.16` на IP или DNS-имя вашего сервера. `ALLOWED_HOSTS` — список имён (без порта), по которым сайт открывают в браузере; запросы с другим именем сервер отклоняет с кодом 403 (защита от DNS rebinding). Если `.env` уже создан, измените `WEB_BIND_IP` и `ALLOWED_HOSTS` в нём и выполните `docker compose up -d --force-recreate`.

HTTP-развёртывание рассчитано на доверенную сеть. Приложение не заменяет сетевую защиту: для доступа через интернет поставьте перед ним обратный прокси с HTTPS и аутентификацией (nginx, Caddy, Traefik), оставив `WEB_BIND_IP=127.0.0.1`, а в `ALLOWED_HOSTS` добавив публичное имя. Прокси должен передавать заголовок `Host` без изменений.

## Настройки и файлы данных

[.env.example](.env.example) перечисляет параметры. `scripts/install.sh` создаёт рабочий `.env` автоматически.

| Параметр | Назначение | По умолчанию |
| --- | --- | --- |
| `WEB_BIND_IP` | Адрес, на котором сайт слушает на сервере | `127.0.0.1` |
| `WEB_PORT` | Порт сайта на хосте | `8765` |
| `ALLOWED_HOSTS` | Имена, по которым сайт открывают в браузере | `localhost,127.0.0.1` |
| `TZ` | Часовой пояс контейнера | `Europe/Moscow` |
| `OPENTRACE_UID`, `OPENTRACE_GID` | Владелец `runtime/` (заполняет `install.sh`) | `1000` |

| Назначение | Путь внутри каталога проекта |
| --- | --- |
| Проекты и образцы | `runtime/projects/` |
| Пользователи, права, журнал аудита | `runtime/security/` |
| Резервные копии | `runtime/backups/` |
| Настройки | `.env` |

Оба каталога данных — bind mount внутри проекта. `runtime/` и `.env` исключены из Git и из контекста сборки образа. Настройки интерфейса (методы анализа, стандарты длин, настройки таблиц и графиков) хранятся в браузере (localStorage); перенос между браузерами — через экспорт и импорт в менеджере объектов.

Основные команды:

```bash
docker compose ps
docker compose logs --tail=100 opentrace
docker compose up -d
docker compose restart opentrace
docker compose down
```

`docker compose down` останавливает контейнер и сохраняет `runtime/`. Не удаляйте этот каталог при обновлении.

## Обновление через GitHub

Обновление выполняется из каталога проекта. Сначала проверьте локальные изменения:

```bash
cd ~/open_trace
git status --short
git branch --show-current
```

Для обычного обновления используйте скрипт:

```bash
./scripts/update_server.sh
```

Он останавливается, если есть локальные изменения; создаёт архив `runtime/backups/opentrace_<дата>.tar.gz`; выполняет `git pull --ff-only origin <ветка>`; пересобирает образ, перезапускает контейнер и ждёт `healthy`; удаляет неиспользуемые образы. `.env`, проекты и журнал аудита остаются на месте. Сборка использует кэш Docker: Rust пересобирается только при изменении `core/`.

Те же шаги вручную:

```bash
./scripts/backup.sh
git pull --ff-only origin main
docker compose up -d --build
docker compose ps
curl -f http://localhost:8765/api/health
```

Если `WEB_BIND_IP` не `127.0.0.1`, подставьте в `curl` один из адресов из `ALLOWED_HOSTS`. Не выполняйте `git reset --hard` для решения конфликта: сначала сохраните локальные изменения. Если Git сообщает о неправильном владельце каталога после копирования через `sudo`, верните каталог своему пользователю и запускайте Git без `sudo`.

После обновления настройки по умолчанию (методы, таблицы) в браузере, где они уже были загружены, не перезаписываются. Новые поставочные настройки появляются только в «чистом» браузере либо при импорте вручную.

Откат к предыдущей версии:

```bash
git log --oneline -5
git checkout <коммит>
docker compose up -d --build
```

Затем вернитесь на ветку: `git checkout main`.

## Резервная копия и перенос установки

```bash
./scripts/backup.sh
```

Скрипт кладёт `projects` и `security` в `runtime/backups/opentrace_<дата>.tar.gz`. Копируйте архив с сервера защищённым способом. Восстановление на новом сервере:

```bash
git clone https://github.com/DrCramer/open_trace.git && cd open_trace
mkdir -p runtime && tar -xzf /путь/к/opentrace_<дата>.tar.gz -C runtime
./scripts/install.sh
```

Восстанавливайте только в пустую установку: архив заменяет содержимое `runtime/projects` и `runtime/security`.

## Скорость анализа пачки

Сервер — многопоточный: каждый запрос обрабатывается в своём потоке, поэтому образцы, которые браузер отправляет параллельно (до 4 запросов одновременно), используют все ядра процессора. Расчёт (базовая линия, сглаживание, производная Савицкого — Голея, поиск пиков, сопоставление со стандартом длин, калибровка, вызов аллелей) выполняется скомпилированным кодом, без интерпретатора.

Результаты побитово совпадают с прежней реализацией на Python: числовые функции CPython (`lgamma`, `pow`, `sum`, `statistics.median`, разбор и печать чисел) воспроизведены явно.

## Разработка и тесты

```bash
cargo build --release --manifest-path core/opentrace-core/Cargo.toml --bin opentrace
cargo test --release --manifest-path core/opentrace-core/Cargo.toml
core/target/release/opentrace serve --port 8765
```

`cargo test` проверяет совпадение с записанными ответами эталонной реализации на Python (`core/opentrace-core/tests/golden`: алгоритмы, демо-фикстуры, HTTP-обмены, сессии командной строки), а также поведение сервера на сокетах.

Веб-интерфейс и поставочные данные лежат в `resources/`; сервер находит их через `OPENTRACE_RESOURCES`, каталог `resources/` рядом с бинарником или дерево исходников.

Сервер принимает параметры `--host`, `--port`, `--projects-dir`, `--security-dir`, `--allowed-host` (повторяемый) и соответствующие переменные окружения `OPENTRACE_HOST`, `OPENTRACE_ALLOWED_HOSTS`, `OPENTRACE_PROJECTS`, `OPENTRACE_SECURITY`. Остальные команды: `opentrace --help`-подобный список — `demo`, `inspect`, `export`, `size`, `str`, `str-run`, `str-demo`, `str-run-demo`, `controls`, `marker-quality`, `pullup-quality`, `import-kit`, `project …`.

Последняя версия с эталонной реализацией на Python — коммит ветки `feature/per-marker-gate-min-call-height` перед удалением `genemapper_open/` (см. `core/README.md`).

## Лицензия

MIT, см. [LICENSE](LICENSE).
